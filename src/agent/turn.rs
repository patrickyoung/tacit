//! One turn of the agent: brief the model, take its edits, check them against the map, and
//! let it correct what the map refused.
use super::{AT_LIMIT, Op, Rejection, Tier, apply, at_limit, brief, health, prompt, wire};
use crate::model::{Category, Session, Thought};
use crate::organizer::CloudConfig;
use openrouter_rs::{
    api::chat::{ChatCompletionRequest, Message},
    types::{ResponseFormat, Role},
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    pub calls: u32,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cost: f64,
    pub elapsed: Duration,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub tier: Tier,
    /// Edits that held against the session the turn was briefed on. Rebase before journaling.
    pub ops: Vec<Op>,
    /// The steward's account of its turn, or a directed turn's word to the user.
    pub note: String,
    /// The placer met something only the steward can fix.
    pub escalate: Option<String>,
    /// The user said how the map is to be kept from now on, whatever it changed today.
    pub remember: bool,
    /// What the map refused along the way, corrected or not.
    pub refused: Vec<Rejection>,
    /// Edits of the last answer the map refused: its note may claim more than was done.
    pub unmet: usize,
    /// The first answer could not be read and was asked for again.
    pub garbled: bool,
    pub usage: Usage,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Self::Place => "place",
            Self::Steward => "steward",
            Self::Direct => "direct",
        }
    }
    /// Model calls one turn may spend, the first included.
    fn calls(self) -> u32 {
        match self {
            Self::Place => 2,
            Self::Steward | Self::Direct => 3,
        }
    }
    pub fn patience(self) -> Duration {
        Duration::from_secs(match self {
            Self::Place => 12,
            Self::Steward | Self::Direct => 90,
        })
    }
}

pub fn request(
    tier: Tier,
    model: &str,
    effort: &str,
    messages: Vec<Message>,
    budget: u32,
) -> Result<ChatCompletionRequest, String> {
    // The placer answers in a few dozen tokens, so first-token latency is what counts.
    // The steward writes at length after thinking, so throughput is, as for a directed turn.
    let (sort, name, schema) = match tier {
        Tier::Place => ("latency", "placement", prompt::placer_schema()),
        Tier::Steward => ("throughput", "stewardship", prompt::steward_schema()),
        Tier::Direct => ("throughput", "direction", prompt::director_schema()),
    };
    structured(model, effort, sort, name, schema, messages, budget)
}

/// A request for one schema-shaped answer. `sort` is what the provider is chosen for.
pub fn structured(
    model: &str,
    effort: &str,
    sort: &str,
    name: &str,
    schema: serde_json::Value,
    messages: Vec<Message>,
    budget: u32,
) -> Result<ChatCompletionRequest, String> {
    let provider = serde_json::from_value(json!({"sort":sort,"require_parameters":true}))
        .map_err(|_| "Invalid provider configuration")?;
    let mut builder = ChatCompletionRequest::builder();
    builder
        .model(model)
        .messages(messages)
        .max_tokens(budget)
        .provider(provider)
        .response_format(ResponseFormat::json_schema(name, true, schema));
    // "off" turns reasoning off; an empty effort leaves the model's own default alone.
    if !effort.is_empty() {
        let reasoning = if effort == "off" {
            json!({"enabled":false})
        } else {
            json!({"effort":effort})
        };
        builder.reasoning(
            serde_json::from_value(reasoning).map_err(|_| "Invalid reasoning configuration")?,
        );
    }
    builder
        .build()
        .map_err(|_| "Could not build OpenRouter request".into())
}

pub async fn call(
    client: &openrouter_rs::OpenRouterClient,
    request: &ChatCompletionRequest,
    patience: Duration,
    usage: &mut Usage,
) -> Result<String, String> {
    let response = tokio::time::timeout(patience, client.chat().create(request))
        .await
        .map_err(|_| "The map agent timed out; your map is safe")?
        // Provider error bodies can echo private prompts; do not put them in UI/logs.
        .map_err(|_| "The map agent is unavailable; check key, credits and model availability")?;
    usage.calls += 1;
    if let Some(spent) = &response.usage {
        usage.prompt_tokens += u64::from(spent.prompt_tokens);
        usage.completion_tokens += u64::from(spent.completion_tokens);
        usage.cost += spent.cost.unwrap_or(0.0);
    }
    let choice = response
        .choices
        .first()
        .ok_or("The map agent returned no result")?;
    if choice.finish_reason().is_some_and(|r| r.as_str() != "stop") {
        return Err("The map agent returned an incomplete result".into());
    }
    let raw = choice.content().ok_or("The map agent returned no text")?;
    if raw.len() > 256 * 1024 {
        return Err("The map agent's result is too large".into());
    }
    Ok(raw.to_owned())
}

/// What still needs doing after a call's edits were applied, and whether it must be done
/// (`hard`) or is only worth one more try.
fn critique(
    tier: Tier,
    work: &Session,
    holds: &BTreeSet<u64>,
    targets: &[u64],
    applied: usize,
    rejected: &[Rejection],
    strains: &[String],
) -> Option<(bool, String)> {
    let waiting: Vec<u64> = brief::waiting(work, holds)
        .into_iter()
        .filter(|id| tier == Tier::Steward || targets.contains(id))
        .collect();
    let unread = tier == Tier::Steward
        && work.thoughts.iter().any(|t| t.category.is_some())
        && brief::narrative(work).is_empty();
    let hard = !waiting.is_empty() || unread;
    // A placer edit the map refused matters only if it left a new thought without a home.
    let soft = (tier != Tier::Place && !rejected.is_empty()) || !strains.is_empty();
    if !hard && !soft {
        return None;
    }
    let mut out = format!("Checked against the map: {applied} edit(s) applied.\n");
    if !rejected.is_empty() {
        out.push_str("Refused:\n");
        for r in rejected.iter().take(24) {
            let op = serde_json::to_string(&r.op).unwrap_or_default();
            let _ = writeln!(out, "- {op}: {}", r.reason);
        }
    }
    if !waiting.is_empty() {
        let ids: Vec<String> = waiting.iter().map(|id| format!("#{id}")).collect();
        let _ = writeln!(
            out,
            "Still without a branch: {}. Place or park each one.",
            ids.join(", ")
        );
    }
    if unread {
        out.push_str("The map has no narrative yet. Give one.\n");
    }
    if !strains.is_empty() {
        out.push_str(if tier.user() {
            "The map as you divided it still strains. Mend it within what the user asked for:\n"
        } else {
            "The map still strains. Fix what a seasoned mapper would fix, and leave what is true to the thinking:\n"
        });
        for note in strains {
            let _ = writeln!(out, "- {note}");
        }
    }
    out.push_str(if tier.user() {
        prompt::REDIRECT
    } else {
        prompt::REPAIR
    });
    Some((hard, out))
}

/// The strains the steward is not sent back to, kept from one turn to the next: those it is
/// taken at its word on, and those that are the user's doing. What an instruction did lasts
/// as long as the instruction: undone, or never carried out, it did nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stands {
    /// Strains it was sent back to once and left as they were, which it judged true to the
    /// thinking. One that is mended is forgotten: should it come back, it is new.
    pub left: BTreeSet<String>,
    /// Strains an instruction of the user's made, each with the instruction.
    pub made: BTreeMap<String, String>,
    /// Branches the user's instructions arranged, each with the instruction and the thoughts
    /// the branch held then. A strain on one is theirs until the branch outgrows what they
    /// left.
    pub asked: BTreeMap<String, (String, usize)>,
}
fn stands(session: &Session, branch: &str, (wish, held): &(String, usize)) -> bool {
    session.directions.contains(wish) && health::weight(session, branch) <= *held
}
impl Stands {
    /// The strain is on a branch the user arranged, which is still as they left it: no fault
    /// of the map's, and nothing for a turn to mend.
    pub fn theirs(&self, session: &Session, strain: &health::Finding) -> bool {
        let kept = |id: &String| {
            let asked = self.asked.get(id);
            asked.is_some_and(|asked| stands(session, id, asked))
        };
        strain.branch.as_ref().is_some_and(kept)
    }
    fn covers(&self, session: &Session, strain: &health::Finding) -> bool {
        let key = &strain.key;
        self.left.contains(key) || self.made.contains_key(key) || self.theirs(session, strain)
    }
    /// Take what another lane has learned, leaving it nothing.
    pub fn absorb(&mut self, other: &mut Self) {
        self.left.append(&mut other.left);
        self.made.append(&mut other.made);
        self.asked.append(&mut other.asked);
    }
    /// Forget what is mended, and what an instruction did that was since taken back. Forget
    /// too the branches that are gone or have outgrown what the user left: from here on they
    /// are the map's to answer for.
    fn settle(&mut self, session: &Session) {
        let standing: BTreeSet<String> = strains(session).map(|f| f.key).collect();
        self.left.retain(|key| standing.contains(key));
        let said = |wish: &String| session.directions.contains(wish);
        self.made
            .retain(|key, wish| standing.contains(key) && said(wish));
        let there = |id: &str| session.categories.iter().any(|c| c.id == id);
        self.asked
            .retain(|id, asked| there(id) && stands(session, id, asked));
    }
}

/// One turn against a snapshot of the session, with the model behind `ask`: it is given the
/// conversation so far and a token budget, and answers in the tier's format. Nothing here
/// touches the journal. Both tiers read in `stands` what is the user's doing, and the steward
/// adds what it leaves standing.
pub async fn drive(
    tier: Tier,
    session: &Session,
    holds: &BTreeSet<u64>,
    stands: &mut Stands,
    ask: impl AsyncFnMut(&[Message], u32) -> Result<String, String>,
) -> Result<Turn, String> {
    work(tier, session, holds, "", stands, ask).await
}
/// The same for a turn that carries out what the user asked of the map. The branches it
/// rearranges, and whatever strains that did not before, go to `stands`: the user asked.
pub async fn follow(
    session: &Session,
    holds: &BTreeSet<u64>,
    instruction: &str,
    stands: &mut Stands,
    ask: impl AsyncFnMut(&[Message], u32) -> Result<String, String>,
) -> Result<Turn, String> {
    work(Tier::Direct, session, holds, instruction, stands, ask).await
}
fn strains(session: &Session) -> impl Iterator<Item = health::Finding> {
    health::review(session).into_iter().filter(|f| f.strain)
}
/// Where a branch hangs, and the thoughts and sub-branches directly under it.
type Seat = (Option<String>, BTreeSet<u64>, BTreeSet<String>);
fn seat(session: &Session, id: &str) -> Option<Seat> {
    let branch = session.categories.iter().find(|c| c.id == id)?;
    let under = |c: &&Category| c.parent.as_deref() == Some(id);
    let on = |t: &&Thought| t.category.as_deref() == Some(id);
    let thoughts = session.thoughts.iter().filter(on).map(|t| t.id);
    let branches = session.categories.iter().filter(under);
    Some((
        branch.parent.clone(),
        thoughts.collect(),
        branches.map(|c| c.id.clone()).collect(),
    ))
}
/// The branches an instruction arranged, with the thoughts each holds now: one that is new,
/// that moved, or that took in thoughts or sub-branches, and those above it. A branch it only
/// took from is as crowded as it was by no word of the user's.
fn arranged(before: &Session, after: &Session) -> BTreeMap<String, usize> {
    let took = |c: &&Category| match (seat(before, &c.id), seat(after, &c.id)) {
        (Some(was), Some(is)) => {
            was.0 != is.0 || !is.1.is_subset(&was.1) || !is.2.is_subset(&was.2)
        }
        _ => true,
    };
    let mut out = BTreeMap::new();
    for c in after.categories.iter().filter(took) {
        let mut above = Some(c);
        while let Some(branch) = above {
            out.insert(branch.id.clone(), health::weight(after, &branch.id));
            let parent = |c: &&Category| Some(&c.id) == branch.parent.as_ref();
            above = after.categories.iter().find(parent);
        }
    }
    out
}
async fn work(
    tier: Tier,
    session: &Session,
    holds: &BTreeSet<u64>,
    instruction: &str,
    stands: &mut Stands,
    mut ask: impl AsyncFnMut(&[Message], u32) -> Result<String, String>,
) -> Result<Turn, String> {
    if at_limit(session) {
        return Err(AT_LIMIT.into());
    }
    if !tier.user() {
        stands.settle(session);
    }
    let (system, brief, targets) = match tier {
        Tier::Place => {
            let brief = brief::place(session, holds, stands);
            let (brief, targets) = brief.ok_or("No thoughts are waiting for a branch")?;
            (prompt::PLACER, brief, targets)
        }
        Tier::Steward => {
            let brief = brief::steward(session, holds, stands);
            (prompt::STEWARD, brief, vec![])
        }
        Tier::Direct => {
            let brief = brief::direct(session, holds, super::instruction(instruction)?);
            (prompt::DIRECTOR, brief, vec![])
        }
    };
    // Reasoning is billed as completion, so the budget leaves it room.
    let budget = match tier {
        Tier::Place => 2000 + 160 * targets.len() as u32,
        Tier::Steward | Tier::Direct => (12_000 + 120 * session.thoughts.len() as u32).min(40_000),
    };
    let mut messages = vec![
        Message::new(Role::System, system),
        Message::new(Role::User, brief),
    ];
    let mut turn = Turn {
        tier,
        ops: vec![],
        note: String::new(),
        escalate: None,
        remember: false,
        refused: vec![],
        unmet: 0,
        garbled: false,
        usage: Usage::default(),
    };
    let (mut work, mut heard, mut whole) = (session.clone(), false, false);
    // Strains the model was sent back to: those still waiting for its answer, and those it
    // has answered for.
    let (mut named, mut judged) = (vec![], vec![]);
    for _ in 0..tier.calls() {
        let raw = match ask(&messages, budget).await {
            Ok(raw) => raw,
            // A failed correction costs nothing: what already held is still good.
            Err(_) if heard => break,
            Err(error) => return Err(error),
        };
        let reply = match wire::read(&raw, &work, tier) {
            Ok(reply) => reply,
            Err(_) if heard => break,
            // A garbled first answer is asked for once more.
            Err(_) if !std::mem::replace(&mut turn.garbled, true) => continue,
            Err(error) => return Err(error),
        };
        let first = !std::mem::replace(&mut heard, true);
        judged.append(&mut named);
        // Naming the idea the map is divided by is dividing it anew.
        let divides = |op: &Op| matches!(op, Op::Focus { principle, .. } if principle.is_some());
        whole |= reply.ops.iter().any(divides);
        let outcome = apply(&work, &reply.ops, tier, holds);
        let applied = outcome.ops.len();
        work = outcome.session;
        turn.ops.extend(outcome.ops);
        // A directed turn's last word is its account of everything it did.
        if !reply.note.is_empty() && (first || applied > 0 || tier.user()) {
            turn.note = reply.note;
        }
        turn.escalate = turn.escalate.or(reply.escalate);
        turn.remember |= reply.remember;
        turn.unmet = outcome.rejected.len();
        let rejected = &outcome.rejected;
        // A directed turn answers for what it was asked and no more: a refusal, never the
        // rest. One that divided the map anew was asked for the whole method, so a branch it
        // leaves too crowded to read is its own to divide.
        let (notes, keys): (Vec<String>, Vec<String>) = match tier {
            Tier::Steward => strains(&work)
                .filter(|f| !stands.covers(&work, f))
                .map(|f| (f.note(), f.key))
                .unzip(),
            Tier::Direct if whole => (health::crowded(&work), vec![]),
            _ => (vec![], vec![]),
        };
        let critique = critique(tier, &work, holds, &targets, applied, rejected, &notes);
        turn.refused.extend(outcome.rejected);
        match critique {
            Some((hard, feedback)) if hard || first => {
                named = keys;
                messages.push(Message::new(Role::Assistant, raw));
                messages.push(Message::new(Role::User, feedback));
            }
            _ => break,
        }
    }
    match tier {
        Tier::Place => stands.settle(&work),
        // Sent back once, a strain that is still there is one the steward chose to leave.
        Tier::Steward => {
            stands.left.extend(judged);
            stands.settle(&work);
        }
        // What the map strains under now and did not before, the user asked for, like the
        // branches they had rearranged.
        Tier::Direct => {
            let keys = |session| strains(session).map(|f| f.key).collect::<BTreeSet<_>>();
            let (before, after) = (keys(session), keys(&work));
            let wish = || instruction.trim().to_owned();
            let made = after.difference(&before);
            stands.made.extend(made.map(|key| (key.clone(), wish())));
            let theirs = |(id, held)| (id, (wish(), held));
            let arranged = arranged(session, &work).into_iter();
            stands.asked.extend(arranged.map(theirs));
        }
    }
    Ok(turn)
}

/// Run one turn with the configured model for its tier. `instruction` is what the user asked
/// of the map, which only a directed turn reads, and `stands` what the steward is not sent
/// back to: a directed turn adds to it, the others read it and the steward's keeps it.
pub async fn run(
    client: &openrouter_rs::OpenRouterClient,
    config: &CloudConfig,
    tier: Tier,
    session: &Session,
    holds: &BTreeSet<u64>,
    instruction: &str,
    stands: &mut Stands,
) -> Result<Turn, String> {
    let started = Instant::now();
    let (model, effort) = match tier {
        Tier::Place => (&config.place_model, &config.place_effort),
        Tier::Steward => (&config.steward_model, &config.steward_effort),
        Tier::Direct => (&config.direct_model, &config.direct_effort),
    };
    let mut usage = Usage::default();
    let mut turn = work(
        tier,
        session,
        holds,
        instruction,
        stands,
        async |messages, budget| {
            let request = request(tier, model, effort, messages.to_vec(), budget)?;
            call(client, &request, tier.patience(), &mut usage).await
        },
    )
    .await?;
    usage.elapsed = started.elapsed();
    turn.usage = usage;
    Ok(turn)
}
