//! Run a saved talk through the map agent the way the app does, on a virtual clock, and write
//! the map as it stood after each step. The talk is the transcript windows of a saved session
//! (Hex's `transcript.vtt`): each window becomes a thought when its speech window ends, the
//! listener cuts it, the placer files it and the steward steps back when `Pace` says. Calls
//! take the wall time they really took, so the map changes when it would have in the room.
//! A recording taken in Direct the map is carried out as an instruction when it ends.
//!
//! It spends OpenRouter credits and sends the transcript to the configured models:
//!   cargo run --release --example story_video -- vtt=FILE out=DIR [direct=2,3]
//! `direct=` numbers the recordings (in order) that were instructions. `out` gets one
//! `MMMMMMMM-label.json` map per step (milliseconds of session time) and `timeline.json`.
use openrouter_rs::api::chat::Message;
use std::collections::BTreeSet;
use std::time::Instant;
use tacit::agent::pace::{Due, Pace};
use tacit::agent::turn::Stands;
use tacit::agent::{self, Tier, Usage, brief, turn, wire};
use tacit::model::Session;
use tacit::organizer::CloudConfig;
use tacit::recording;

struct Arrival {
    at: f64,
    text: String,
    direct: bool,
}

fn seconds(stamp: &str) -> f64 {
    let mut parts = stamp.split(':').map(|p| p.parse::<f64>().unwrap_or(0.0));
    let (h, m, s) = (
        parts.next().unwrap_or(0.0),
        parts.next().unwrap_or(0.0),
        parts.next().unwrap_or(0.0),
    );
    h * 3600.0 + m * 60.0 + s
}

/// Windows (or whole recordings, for instructions) in the order they finish.
fn arrivals(vtt: &str, direct: &[usize]) -> Vec<Arrival> {
    let mut recordings: Vec<(String, Vec<(f64, String)>)> = vec![];
    for block in vtt.split("\n\n") {
        let mut lines = block.lines();
        let (Some(id), Some(times)) = (lines.next(), lines.next()) else {
            continue;
        };
        let Some((_, end)) = times.split_once(" --> ") else {
            continue;
        };
        let text: Vec<&str> = lines.collect();
        let recording = id.rsplit_once('-').map_or(id, |(r, _)| r).to_owned();
        let entry = (seconds(end), text.join(" "));
        match recordings.last_mut() {
            Some((name, windows)) if *name == recording => windows.push(entry),
            _ => recordings.push((recording, vec![entry])),
        }
    }
    let mut out = vec![];
    for (index, (_, windows)) in recordings.iter().enumerate() {
        if direct.contains(&(index + 1)) {
            let text: Vec<&str> = windows.iter().map(|w| w.1.as_str()).collect();
            let at = windows.last().map_or(0.0, |w| w.0);
            out.push(Arrival {
                at,
                text: text.join(" "),
                direct: true,
            });
        } else {
            out.extend(windows.iter().map(|(at, text)| Arrival {
                at: *at,
                text: text.clone(),
                direct: false,
            }));
        }
    }
    out.sort_by(|a, b| a.at.total_cmp(&b.at));
    out
}

type Cloud<'a> = (&'a openrouter_rs::OpenRouterClient, &'a CloudConfig);

/// One turn of a tier, its edits tried on the map and kept as the journal would replay them.
/// Returns whether the turn asked for the steward and what it said.
async fn step(
    (client, config): Cloud<'_>,
    tier: Tier,
    session: &mut Session,
    stands: &mut Stands,
    instruction: &str,
) -> Result<(Option<String>, String), String> {
    let none = BTreeSet::new();
    let (model, effort) = match tier {
        Tier::Place => (&config.place_model, &config.place_effort),
        Tier::Steward => (&config.steward_model, &config.steward_effort),
        Tier::Direct => (&config.direct_model, &config.direct_effort),
    };
    let mut usage = Usage::default();
    let ask = async |messages: &[Message], budget: u32| -> Result<String, String> {
        let request = turn::request(tier, model, effort, messages.to_vec(), budget)?;
        let raw = turn::call(client, &request, tier.patience(), &mut usage).await?;
        let _ = wire::read(&raw, session, tier);
        Ok(raw)
    };
    let turn = match tier {
        Tier::Direct => turn::follow(session, &none, instruction, stands, ask).await?,
        _ => turn::drive(tier, session, &none, stands, ask).await?,
    };
    let outcome = agent::apply(session, &turn.ops, tier, &none);
    let journal = match tier {
        Tier::Direct if outcome.ops.is_empty() && !turn.remember => Ok(session.clone()),
        Tier::Direct => agent::guided(session, instruction, &outcome.ops),
        _ => agent::replay(session, &outcome.ops),
    };
    let mut next = outcome.session;
    if let Ok(journaled) = &journal {
        next.directions.clone_from(&journaled.directions);
    }
    *session = next;
    Ok((turn.escalate, turn.note))
}

fn main() -> Result<(), String> {
    let (mut vtt, mut out, mut direct) = (None, None, vec![]);
    for arg in std::env::args().skip(1) {
        match arg.split_once('=') {
            Some(("vtt", v)) => vtt = Some(v.to_owned()),
            Some(("out", v)) => out = Some(v.to_owned()),
            Some(("direct", v)) => direct = v.split(',').filter_map(|n| n.parse().ok()).collect(),
            _ => return Err(format!("Unknown argument {arg}")),
        }
    }
    let (vtt, out) = (vtt.ok_or("vtt=FILE")?, out.ok_or("out=DIR")?);
    let vtt = std::fs::read_to_string(&vtt).map_err(|e| format!("{vtt}: {e}"))?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut config = CloudConfig::from_env()?.ok_or("OpenRouter is not configured")?;
    let _ = &mut config;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Could not start a runtime")?;
    let client = config.client()?;
    let cloud = (&client, &config);
    let arrivals = arrivals(&vtt, &direct);
    println!("{} arrivals", arrivals.len());
    let (mut session, mut stands, mut pace, mut usage) = (
        Session::default(),
        Stands::default(),
        Pace::default(),
        Usage::default(),
    );
    let (mut now, mut next) = (0.0_f64, 0);
    let mut timeline: Vec<serde_json::Value> = vec![];
    let none = BTreeSet::new();
    let mut keep = |now: f64, label: String, note: &str, session: &Session| -> Result<(), String> {
        let file = format!("{:08}-{}.json", (now * 1000.0) as u64, timeline.len());
        let saved = serde_json::to_string(session).map_err(|e| e.to_string())?;
        std::fs::write(format!("{out}/{file}"), saved).map_err(|e| e.to_string())?;
        println!("{now:7.1}s  {label}  {note}");
        timeline.push(serde_json::json!({"at": now, "file": file, "label": label, "note": note}));
        Ok(())
    };
    keep(0.0, "start".into(), "", &session)?;
    let mut idle = 0;
    loop {
        // What has been said by now.
        if let Some(arrival) = arrivals.get(next).filter(|a| a.at <= now) {
            {
                next += 1;
                if arrival.direct {
                    pace.take_over(&session);
                    pace.sent(Tier::Direct, &session, now);
                    let started = Instant::now();
                    let (_, note) = runtime.block_on(step(
                        cloud,
                        Tier::Direct,
                        &mut session,
                        &mut stands,
                        &arrival.text,
                    ))?;
                    now += started.elapsed().as_secs_f64();
                    pace.landed(Tier::Direct, &session, true, now);
                    keep(
                        now,
                        format!("You asked: {}", arrival.text),
                        &format!("Directed turn. {note}"),
                        &session,
                    )?;
                } else {
                    pace.speaking(now);
                    let started = Instant::now();
                    let chunks = runtime.block_on(recording::sift(
                        &client,
                        &config,
                        &arrival.text,
                        &session,
                        &mut usage,
                    ))?;
                    now += started.elapsed().as_secs_f64();
                    for chunk in &chunks {
                        session.add(chunk.trim());
                    }
                    keep(
                        now,
                        format!("Heard: {}", arrival.text),
                        &format!("Listener cut it into {} thought(s)", chunks.len()),
                        &session,
                    )?;
                }
                idle = 0;
                continue;
            }
        }
        match pace.due(&session, &none, now) {
            Due::Ask(tier) => {
                let waiting = brief::waiting(&session, &none).len();
                pace.sent(tier, &session, now);
                let started = Instant::now();
                let (escalate, note) =
                    runtime.block_on(step(cloud, tier, &mut session, &mut stands, ""))?;
                now += started.elapsed().as_secs_f64();
                let filed = brief::waiting(&session, &none).len() < waiting;
                pace.landed(tier, &session, filed, now);
                if escalate.is_some() {
                    pace.call();
                }
                keep(now, format!("{} turn", tier.name()), &note, &session)?;
                idle = 0;
            }
            Due::At(at) => {
                let until = arrivals.get(next).map_or(at, |a| a.at.min(at));
                now = until.max(now + 0.05);
            }
            Due::Rest => match arrivals.get(next) {
                Some(a) => now = now.max(a.at),
                None => {
                    idle += 1;
                    if idle > 3 {
                        break;
                    }
                    now += 5.0;
                }
            },
        }
        if now > 3600.0 {
            break;
        }
    }
    keep(now, "end".into(), "", &session)?;
    std::fs::write(
        format!("{out}/timeline.json"),
        serde_json::to_string_pretty(&timeline).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("listener ${:.4}", usage.cost);
    Ok(())
}
