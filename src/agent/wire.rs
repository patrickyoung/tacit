//! What a model writes, and how it becomes edits. The answer arrives grouped the way the
//! schema in `prompt` asks for it; here it is tidied and put in the order a mapper would make
//! the edits. Tidying is generous, because a refused edit costs the speaker another round trip.
use super::{Op, Tier};
use crate::model::{ParkKind, Relationship, Role, Session};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use std::collections::BTreeMap;

/// Models write null where a list or a group is empty.
fn lenient<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}
/// A role by name. "Keep", null or a word that is no role leaves the thought's role alone.
fn role<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Role>, D::Error> {
    let name = Option::<String>::deserialize(d)?.unwrap_or_default();
    let roles = [
        Role::Observation,
        Role::Question,
        Role::Tension,
        Role::Possibility,
    ];
    Ok(roles
        .into_iter()
        .find(|role| format!("{role:?}").eq_ignore_ascii_case(name.trim())))
}
#[derive(Deserialize)]
struct Grow {
    id: String,
    name: String,
    #[serde(default)]
    parent: Option<String>,
}
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    Place,
    Later,
    OffTopic,
    Aside,
}
/// The placer's entry for one thought.
#[derive(Deserialize)]
struct Entry {
    id: u64,
    #[serde(default)]
    idea: Option<String>,
    outcome: Verdict,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default, deserialize_with = "role")]
    role: Option<Role>,
    #[serde(default)]
    topic: Option<String>,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct Placement {
    #[serde(deserialize_with = "lenient")]
    branches: Vec<Grow>,
    #[serde(deserialize_with = "lenient")]
    file: Vec<Entry>,
    steward: Option<String>,
    about: Option<String>,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct Aim {
    focus: Option<String>,
    principle: Option<String>,
    title: Option<String>,
}
#[derive(Deserialize)]
struct Fold {
    from: String,
    into: String,
}
#[derive(Deserialize)]
struct Reparent {
    branch: String,
    #[serde(default)]
    parent: Option<String>,
}
#[derive(Deserialize)]
struct Rename {
    branch: String,
    name: String,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct Branches {
    #[serde(deserialize_with = "lenient")]
    grow: Vec<Grow>,
    #[serde(deserialize_with = "lenient")]
    merge: Vec<Fold>,
    #[serde(rename = "move", deserialize_with = "lenient")]
    moves: Vec<Reparent>,
    #[serde(deserialize_with = "lenient")]
    order: Vec<String>,
    #[serde(deserialize_with = "lenient")]
    remove: Vec<String>,
    #[serde(deserialize_with = "lenient")]
    rename: Vec<Rename>,
}
#[derive(Deserialize)]
struct Put {
    id: u64,
    #[serde(default)]
    idea: Option<String>,
    parent: String,
    /// Only a directed turn is believed: the user said where this one thought belongs.
    #[serde(default, deserialize_with = "lenient")]
    pin: bool,
    #[serde(default, deserialize_with = "role")]
    role: Option<Role>,
}
#[derive(Deserialize)]
struct Join {
    from: Vec<u64>,
    idea: String,
    into: u64,
}
#[derive(Deserialize)]
struct Set {
    id: u64,
    kind: ParkKind,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    topic: String,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct Ideas {
    #[serde(deserialize_with = "lenient")]
    file: Vec<Put>,
    #[serde(deserialize_with = "lenient")]
    merge: Vec<Join>,
    #[serde(deserialize_with = "lenient")]
    park: Vec<Set>,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct Stewardship {
    #[serde(deserialize_with = "lenient")]
    aim: Aim,
    audit: Option<String>,
    #[serde(deserialize_with = "lenient")]
    branches: Branches,
    #[serde(deserialize_with = "lenient")]
    ideas: Ideas,
    links: Option<Vec<Relationship>>,
    narrative: Option<String>,
    /// A directed turn's two last words: whether the instruction is a standing one, and what
    /// to tell the user.
    #[serde(deserialize_with = "lenient")]
    remember: bool,
    reply: Option<String>,
}
pub struct Reply {
    pub ops: Vec<Op>,
    /// The steward's own account of what it changed, or a directed turn's word to the user.
    pub note: String,
    /// The placer's word to the steward.
    pub escalate: Option<String>,
    /// The user's instruction is a standing one: keep it as guidance even if nothing changed.
    pub remember: bool,
}

fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn some(text: Option<String>) -> Option<String> {
    text.map(|t| clean(&t)).filter(|t| !t.is_empty())
}
fn clip(text: &str, max: usize) -> String {
    let cut: String = clean(text).chars().take(max).collect();
    cut.trim_end().to_owned()
}
/// A label is keywords: keep the whole words that fit.
fn fit(text: &str, words: usize, chars: usize) -> String {
    let mut out = String::new();
    for word in text.split_whitespace().take(words) {
        if out.chars().count() + word.chars().count() + usize::from(!out.is_empty()) > chars {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}
fn idea(text: Option<String>) -> Option<String> {
    text.map(|t| fit(&t, 8, 58)).filter(|t| !t.is_empty())
}
fn topic(text: Option<String>, kind: ParkKind) -> String {
    let topic = fit(&text.unwrap_or_default(), 6, 36);
    if topic.is_empty() {
        kind.label().into()
    } else {
        topic
    }
}
/// Why a thought is parked: the model's sentence, or the kind's own.
fn reason(text: &str, kind: ParkKind) -> String {
    let reason = clip(text, 300);
    if !reason.is_empty() {
        return reason;
    }
    match kind {
        ParkKind::Later => "Waiting for its place on the map",
        ParkKind::OffTopic => "A different subject from this map",
        ParkKind::Aside => "Nothing to map",
    }
    .into()
}
/// A branch ID from whatever the model called it.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(60);
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "branch".into()
    } else {
        out.into()
    }
}
/// The ID inside `[team-trust] Team trust`, `[team-trust]` or `team-trust`.
fn bare(id: &str) -> String {
    let id = id.trim();
    let inner = id.strip_prefix('[').and_then(|rest| rest.split_once(']'));
    inner
        .map_or(id, |(inner, _)| inner)
        .trim_matches(['[', ']', '#', ' '])
        .to_owned()
}

/// New branches get slug IDs whatever the model wrote, and every reference follows.
struct Ids<'a> {
    session: &'a Session,
    new: BTreeMap<String, String>,
}
impl<'a> Ids<'a> {
    fn new(grow: &[Grow], session: &'a Session) -> Self {
        let mut new = BTreeMap::new();
        for g in grow {
            let raw = bare(&g.id);
            if !session.categories.iter().any(|c| c.id == raw) {
                let id = slug(if raw.is_empty() { &g.name } else { &raw });
                new.insert(raw, id);
            }
        }
        Self { session, new }
    }
    fn at(&self, id: &str) -> String {
        let raw = bare(id);
        let known = |id: &str| self.session.categories.iter().any(|c| c.id == id);
        if raw.is_empty() || known(&raw) {
            return raw;
        }
        if let Some(id) = self.new.get(&raw) {
            return id.clone();
        }
        let guess = slug(&raw);
        if known(&guess) || self.new.values().any(|id| *id == guess) {
            return guess;
        }
        // A model sometimes answers with the branch's name instead of its ID.
        let mut named = self
            .session
            .categories
            .iter()
            .filter(|c| c.name.eq_ignore_ascii_case(&raw));
        match (named.next(), named.next()) {
            (Some(c), None) => c.id.clone(),
            _ => raw,
        }
    }
    /// A branch the map has, or one this answer grows.
    fn real(&self, id: &str) -> bool {
        let known = self.session.categories.iter().any(|c| c.id == id);
        known || self.new.values().any(|new| new == id)
    }
    fn parent(&self, id: &Option<String>) -> Option<String> {
        id.as_deref()
            .map(|id| self.at(id))
            .filter(|id| !id.is_empty())
    }
    fn grow(&self, grow: &[Grow], ops: &mut Vec<Op>) {
        for g in grow {
            let raw = bare(&g.id);
            ops.push(Op::Branch {
                id: self.new.get(&raw).cloned().unwrap_or(raw),
                name: fit(&g.name, 5, 36),
                parent: self.parent(&g.parent),
            });
        }
    }
}

/// The first object in an answer. A model sometimes fences it, introduces it, or says it twice.
pub(crate) fn first<T: DeserializeOwned>(raw: &str) -> Option<T> {
    let object = &raw[raw.find('{')?..];
    let mut values = serde_json::Deserializer::from_str(object).into_iter();
    values.next()?.ok()
}
pub fn read(raw: &str, session: &Session, tier: Tier) -> Result<Reply, String> {
    match tier {
        Tier::Place => first(raw).map(|wire| placement(wire, session)),
        Tier::Steward | Tier::Direct => {
            first(raw).map(|wire| stewardship(wire, session, tier.user()))
        }
    }
    .ok_or_else(|| "The map agent returned an unreadable answer".into())
}

fn placement(wire: Placement, session: &Session) -> Reply {
    let ids = Ids::new(&wire.branches, session);
    let mut ops = vec![];
    ids.grow(&wire.branches, &mut ops);
    // Until a map has a subject, nothing can be off it: such a thought waits as "not yet",
    // where the steward looks again.
    let filed = session.thoughts.iter().filter(|t| t.category.is_some());
    let settled = filed.count() >= 3
        || session
            .charter
            .as_ref()
            .is_some_and(|c| !c.focus.trim().is_empty());
    for e in wire.file {
        let kind = match e.outcome {
            Verdict::Place => {
                // A placement without a branch is left for the turn's check to report.
                if let Some(branch) = some(e.parent) {
                    ops.push(Op::Place {
                        thought: e.id,
                        branch: ids.at(&branch),
                        concept: idea(e.idea),
                        role: e.role,
                    });
                }
                continue;
            }
            Verdict::OffTopic if settled => ParkKind::OffTopic,
            Verdict::Later | Verdict::OffTopic => ParkKind::Later,
            Verdict::Aside => ParkKind::Aside,
        };
        ops.push(Op::Park {
            thought: e.id,
            kind,
            topic: topic(e.topic, kind),
            reason: reason(&e.reason.unwrap_or_default(), kind),
            concept: idea(e.idea),
        });
    }
    // The placer names a map only while it has no name; a later change of subject is the
    // steward's call.
    let placeholder = session.title == Session::default().title;
    if let Some(title) = some(wire.about).filter(|_| placeholder) {
        ops.push(Op::Title {
            title: clip(&title, 100),
        });
    }
    Reply {
        ops,
        note: String::new(),
        escalate: some(wire.steward).map(|e| clip(&e, 300)),
        remember: false,
    }
}

/// The steward's edits, or with `user` the same edits made on the user's word.
fn stewardship(wire: Stewardship, session: &Session, user: bool) -> Reply {
    let (branches, ideas) = (wire.branches, wire.ideas);
    let ids = Ids::new(&branches.grow, session);
    let mut ops = vec![];
    ids.grow(&branches.grow, &mut ops);
    for m in &branches.moves {
        ops.push(Op::Move {
            branch: ids.at(&m.branch),
            parent: ids.parent(&m.parent),
        });
    }
    for m in &branches.merge {
        ops.push(Op::Merge {
            from: ids.at(&m.from),
            into: ids.at(&m.into),
        });
    }
    for r in &branches.rename {
        ops.push(Op::Rename {
            branch: ids.at(&r.branch),
            name: fit(&r.name, 5, 36),
        });
    }
    for p in ideas.file {
        let branch = ids.at(&p.parent);
        ops.push(Op::Place {
            thought: p.id,
            branch: branch.clone(),
            concept: idea(p.idea),
            role: p.role,
        });
        if user && p.pin {
            ops.push(Op::Pin {
                thought: p.id,
                branch,
            });
        }
    }
    for p in ideas.park {
        ops.push(Op::Park {
            thought: p.id,
            kind: p.kind,
            topic: topic(Some(p.topic), p.kind),
            reason: reason(&p.reason, p.kind),
            concept: None,
        });
    }
    for c in ideas.merge {
        ops.push(Op::Combine {
            into: c.into,
            from: c.from,
            concept: fit(&c.idea, 8, 58),
        });
    }
    for branch in &branches.remove {
        ops.push(Op::Remove {
            branch: ids.at(branch),
        });
    }
    // Reading order is settled last, among the branches the other edits leave standing.
    let gone = |id: &String| {
        let merged = branches.merge.iter().any(|m| ids.at(&m.from) == *id);
        merged || branches.remove.iter().any(|branch| ids.at(branch) == *id)
    };
    let mut order: Vec<String> = vec![];
    for id in branches.order.iter().map(|id| ids.at(id)) {
        if ids.real(&id) && !gone(&id) && !order.contains(&id) {
            order.push(id);
        }
    }
    if order.len() > 1 {
        ops.push(Op::Order { branches: order });
    }
    if let Some(mut links) = wire.links {
        for link in &mut links {
            link.label = clip(&link.label, 60);
        }
        links.retain(|l| !l.label.is_empty());
        ops.push(Op::Links { links });
    }
    let aim = wire.aim;
    let (focus, principle) = (some(aim.focus), some(aim.principle));
    if focus.is_some() || principle.is_some() {
        ops.push(Op::Focus {
            focus: focus.map(|t| clip(&t, 300)),
            principle: principle.map(|t| clip(&t, 300)),
        });
    }
    if let Some(title) = some(aim.title) {
        ops.push(Op::Title {
            title: clip(&title, 100),
        });
    }
    if let Some(narrative) = wire.narrative.filter(|n| !n.trim().is_empty()) {
        ops.push(Op::Narrative {
            narrative: narrative.trim().chars().take(3000).collect(),
        });
    }
    let note = if user { wire.reply } else { wire.audit };
    Reply {
        ops,
        note: clip(&note.unwrap_or_default(), 300),
        escalate: None,
        remember: user && wire.remember,
    }
}
