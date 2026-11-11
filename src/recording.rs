//! Meaning-based, lossless segmentation of completed Hex transcripts.
use crate::agent::{Usage, brief, prompt, turn, wire};
use crate::{model::Session, organizer::CloudConfig};
use openrouter_rs::{OpenRouterClient, api::chat::Message, types::Role};
use serde::Deserialize;
use std::fmt::Write;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

/// A recording shorter than this holds one thought: not worth a round trip.
const BRIEF: usize = 8;

pub struct Request {
    pub id: u64,
    pub text: String,
    pub context: Session,
}
pub struct Response {
    pub id: u64,
    pub text: String,
    pub result: Result<Vec<String>, String>,
}
pub struct Worker {
    pub send: Sender<Request>,
    pub receive: Receiver<Response>,
}
impl Worker {
    pub fn start(config: CloudConfig, wake: impl Fn() + Send + 'static) -> Self {
        let (send, requests) = mpsc::channel::<Request>();
        let (responses, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            // One client is one connection pool: recordings arrive seconds apart.
            let client = config.client();
            // Every recording matters: unlike map revisions, never coalesce recordings.
            while let Ok(request) = requests.recv() {
                let result = match (&runtime, &client) {
                    (Ok(rt), Ok(client)) => rt.block_on(sift(
                        client,
                        &config,
                        &request.text,
                        &request.context,
                        &mut Usage::default(),
                    )),
                    _ => Err("Could not start recording organizer".into()),
                };
                if responses
                    .send(Response {
                        id: request.id,
                        text: request.text,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
                wake();
            }
        });
        Self { send, receive }
    }
}

// Number non-whitespace tokens; the model chooses boundaries, never rewrites text.
fn tokens(text: &str) -> Vec<(usize, &str)> {
    let mut offset = 0;
    text.split_whitespace()
        .map(|word| {
            let start = offset + text[offset..].find(word).unwrap();
            offset = start + word.len();
            (start, word)
        })
        .collect()
}
#[derive(Deserialize)]
struct Heard {
    thoughts: Vec<Unit>,
}
#[derive(Deserialize)]
struct Unit {
    end: usize,
}

pub fn parse(raw: &str, text: &str) -> Result<Vec<String>, String> {
    let heard: Heard = wire::first(raw).ok_or("Invalid thought boundaries")?;
    let words = tokens(text);
    if words.is_empty() || heard.thoughts.is_empty() || heard.thoughts.len() > 100 {
        return Err("Incomplete thought boundaries".into());
    }
    let mut ends: Vec<usize> = heard.thoughts.iter().map(|unit| unit.end).collect();
    // The last thought ends on the final word: a count a few words out was a miscount.
    let last = ends.last_mut().unwrap();
    if *last + 3 >= words.len() {
        *last = words.len();
    }
    if ends.last() != Some(&words.len()) {
        return Err("Incomplete thought boundaries".into());
    }
    let mut previous = 0;
    let mut chunks = Vec::new();
    for end in ends {
        if end <= previous || end > words.len() {
            return Err("Overlapping thought boundaries".into());
        }
        let start_byte = if previous == 0 { 0 } else { words[previous].0 };
        let end_byte = if end == words.len() {
            text.len()
        } else {
            words[end].0
        };
        chunks.push(text[start_byte..end_byte].to_owned());
        previous = end;
    }
    Ok(chunks)
}

/// Ask the listener where each thought in a recording ends. Its answer, unread.
pub async fn hear(
    client: &OpenRouterClient,
    config: &CloudConfig,
    text: &str,
    context: &Session,
    usage: &mut Usage,
) -> Result<String, String> {
    let words = tokens(text);
    // The map as an outline, not the session: this runs on every few seconds of speech.
    let mut input = brief::listen(context);
    let _ = write!(input, "\nRECORDING\n{}\n\nWORDS\n", text.trim());
    for (index, (_, word)) in words.iter().enumerate() {
        let _ = write!(input, "{}:{word} ", index + 1);
    }
    let request = turn::structured(
        &config.assist_model,
        &config.assist_effort,
        "latency",
        "thoughts",
        prompt::listener_schema(),
        vec![
            Message::new(Role::System, prompt::LISTENER),
            Message::new(Role::User, input.trim_end()),
        ],
        (800 + words.len()).min(12_000) as u32,
    )?;
    turn::call(client, &request, Duration::from_secs(30), usage)
        .await
        .map_err(|error| error.replace("The map agent", "Thought splitting"))
}

/// A recording cut into its thoughts: every byte kept, in order.
pub async fn sift(
    client: &OpenRouterClient,
    config: &CloudConfig,
    text: &str,
    context: &Session,
    usage: &mut Usage,
) -> Result<Vec<String>, String> {
    let count = tokens(text).len();
    if count == 0 || count > 12000 || text.len() > 96000 {
        return Err("Recording too long to split automatically; full transcript kept".into());
    }
    if count < BRIEF {
        return Ok(vec![text.to_owned()]);
    }
    // An unreadable answer is asked for once more; a provider that fails is not asked again.
    match parse(&hear(client, config, text, context, usage).await?, text) {
        Err(_) => parse(&hear(client, config, text, context, usage).await?, text),
        cut => cut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn raw(ends: &[(usize, &str)]) -> String {
        let unit = |(end, closes): &(usize, &str)| serde_json::json!({"about":"a thought","closes":closes,"end":end});
        serde_json::json!({"thoughts":ends.iter().map(unit).collect::<Vec<_>>()}).to_string()
    }
    #[test]
    fn boundaries_preserve_every_byte_and_unicode_in_order() {
        let text = "  Café meetings help.\n\nBut budgets don’t.  Back to meetings: fewer! ";
        let chunks = parse(&raw(&[(3, ""), (6, ""), (10, "")]), text).unwrap();
        assert_eq!(chunks.concat(), text);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[1], "But budgets don’t.  ");
    }
    #[test]
    fn rejects_missing_repeated_reversed_and_out_of_range_boundaries() {
        let text = "one two three four five six seven eight";
        for ends in [&[][..], &[2], &[2, 2, 8], &[5, 3, 8], &[9, 8], &[0, 8]] {
            let ends: Vec<_> = ends.iter().map(|end| (*end, "")).collect();
            assert!(parse(&raw(&ends), text).is_err(), "{ends:?}");
        }
        assert_eq!(parse(&raw(&[(8, "")]), text).unwrap(), vec![text]);
        assert!(parse("no boundaries here", text).is_err());
    }
    #[test]
    fn reads_a_wrapped_answer_and_takes_the_last_thought_to_the_final_word() {
        let text =
            "Meetings take all day. Yesterday we had six. Separately, hosting costs doubled.";
        let whole = [
            "Meetings take all day. Yesterday we had six. ",
            "Separately, hosting costs doubled.",
        ];
        let fenced = format!("```json\n{}\n```", raw(&[(8, "we had six."), (12, "")]));
        assert_eq!(parse(&fenced, text).unwrap(), whole);
        // A final count a few words out was a miscount: nothing said is left behind.
        assert_eq!(parse(&raw(&[(8, ""), (11, "")]), text).unwrap(), whole);
        assert_eq!(parse(&raw(&[(8, ""), (14, "")]), text).unwrap(), whole);
    }
}
