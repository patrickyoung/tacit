//! The mind map agent: the one editor of the live map.
//!
//! It maps the way a seasoned facilitator does. One central subject; three to seven main
//! branches that divide it by a single ordering idea; keyword labels; about seven items to a
//! branch at most; and a parking lot for whatever is not part of this map, or not yet.
//!
//! Three tiers share one vocabulary of edits ([`Op`]). The placer files each thought as it
//! arrives, or parks it. The steward steps back when the talk pauses or the map strains, and
//! restructures, combines repeats, revisits the parking lot and rewrites the reading. A
//! directed turn carries out what the user asks of the map, with the user's own authority.
//!
//! A turn returns edits, never a whole map. [`apply`] takes them one at a time against the
//! session as it is now, so a turn that began before the speaker said more is rebased instead
//! of thrown away, and one bad edit costs only itself. What survives is journaled as
//! `SessionEvent::Mapped`, or `SessionEvent::Guided` when the user asked for it, and replays
//! through [`replay`] and [`guided`] with no model in the loop.
use crate::model::{ParkKind, Relationship, Role, Session};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub mod brief;
pub mod health;
pub mod pace;
pub mod prompt;
mod step;
pub mod turn;
pub mod wire;
pub mod worker;
pub use step::{MAX_ROOTS, label};
pub use turn::{Turn, Usage};
pub use worker::Worker;

/// One edit to the map. Thoughts are never deleted: they are placed, parked or combined.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Grow a branch. Repeating an existing branch with the same name and parent is a no-op.
    Branch {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
    },
    Rename {
        branch: String,
        name: String,
    },
    Move {
        branch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
    },
    /// Fold one branch into another, with its thoughts and sub-branches.
    Merge {
        from: String,
        into: String,
    },
    /// A branch can go once no thought hangs from it; its empty sub-branches go with it.
    Remove {
        branch: String,
    },
    /// Put branches in reading order: those named take, between them, the order given, and
    /// the rest stay where they are.
    Order {
        branches: Vec<String>,
    },
    /// File a thought. Placing a parked thought brings it back onto the map.
    Place {
        thought: u64,
        branch: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        concept: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<Role>,
    },
    /// The user said this thought belongs in this branch. It stays there until they say
    /// otherwise.
    Pin {
        thought: u64,
        branch: String,
    },
    /// Hold a thought beside the map, or change why a parked one is held.
    Park {
        thought: u64,
        kind: ParkKind,
        topic: String,
        reason: String,
        /// A label for the parking lot, when the thought has none yet.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        concept: Option<String>,
    },
    /// The speaker said one thing more than once: keep `into`, with everyone's words.
    Combine {
        into: u64,
        from: Vec<u64>,
        concept: String,
    },
    /// Replace the map's cross-links.
    Links {
        links: Vec<Relationship>,
    },
    Title {
        title: String,
    },
    Narrative {
        narrative: String,
    },
    /// What the map is about and the one idea its main branches are divided by.
    Focus {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focus: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        principle: Option<String>,
    },
}
pub const MAX_OPS: usize = 400;
/// Thoughts, parked ones included, past which the agent leaves the map in the user's hands.
pub const MAX_THOUGHTS: usize = 400;
pub const AT_LIMIT: &str = "Map limit reached (400 thoughts); editing by hand remains available";
pub fn at_limit(session: &Session) -> bool {
    session.thoughts.len() + session.parked.len() > MAX_THOUGHTS
}

/// Who is editing. The placer works under a deliberately narrow licence, and a directed turn
/// under the user's: what they pinned, kept or parked is theirs to change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Place,
    Steward,
    Direct,
}
impl Tier {
    /// What a turn is told when it would leave the map with too many main branches.
    fn full(self) -> String {
        let instead = match self {
            Self::Place => "file the thought in the closest branch and tell the steward",
            Self::Steward | Self::Direct => "merge two or nest this one",
        };
        let most = MAX_ROOTS + usize::from(self.user());
        format!("A map keeps at most {most} main branches; {instead}")
    }
    /// The edits are the user's own decisions, carried out for them.
    pub fn user(self) -> bool {
        self == Self::Direct
    }
}
/// Instructions a map remembers as guidance, and the longest one it takes.
pub const MAX_GUIDANCE: usize = 20;
pub const MAX_INSTRUCTION: usize = 8000;
/// What the user asked of the map, when it is something a turn can carry out.
pub fn instruction(text: &str) -> Result<&str, String> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_INSTRUCTION {
        return Err("Guidance must be present and under 8000 characters".into());
    }
    Ok(text)
}
#[derive(Clone, Debug, PartialEq)]
pub struct Rejection {
    pub op: Op,
    pub reason: String,
}
pub struct Outcome {
    pub session: Session,
    /// The edits that took effect, in order. Journal exactly these.
    pub ops: Vec<Op>,
    pub rejected: Vec<Rejection>,
}

fn strict(current: &Session, ops: &[Op], user: bool) -> Result<Session, String> {
    if ops.len() > MAX_OPS {
        return Err("Too many map edits in one turn".into());
    }
    let mut next = current.clone();
    next.interpretation = None;
    for op in ops {
        step::step(&mut next, op, user)?;
    }
    if step::wide(current, &next, user) {
        let tier = if user { Tier::Direct } else { Tier::Steward };
        return Err(tier.full());
    }
    next.refresh_reading();
    next.validate()?;
    Ok(next)
}
/// Strict application for the journal: every edit applies or the whole turn is invalid.
pub fn replay(current: &Session, ops: &[Op]) -> Result<Session, String> {
    strict(current, ops, false)
}
/// Strict application of a turn the user directed. The instruction stays with the map as
/// guidance for every later turn, whether or not it changed anything now.
pub fn guided(current: &Session, instruction: &str, ops: &[Op]) -> Result<Session, String> {
    let instruction = self::instruction(instruction)?;
    let mut next = if ops.is_empty() {
        current.clone()
    } else {
        strict(current, ops, true)?
    };
    next.directions.push(instruction.into());
    if next.directions.len() > MAX_GUIDANCE {
        next.directions.remove(0);
    }
    Ok(next)
}

/// Lenient application for live turns. Each edit is tried against the session as it stands;
/// one that no longer fits is set aside with a reason the model can act on.
/// `holds` are captures still being split into thoughts, which nothing may touch.
pub fn apply(current: &Session, ops: &[Op], tier: Tier, holds: &BTreeSet<u64>) -> Outcome {
    // Edits the turn goes again without: scaffolding, and main branches past the limit.
    let (mut idle, mut wide) = (BTreeSet::new(), vec![]);
    if tier == Tier::Place {
        // The placer's removals are housekeeping, worked out afresh against this session.
        idle.extend((0..ops.len()).filter(|index| matches!(ops[*index], Op::Remove { .. })));
    }
    loop {
        let mut work = current.clone();
        work.interpretation = None;
        let (mut applied, mut rejected) = (vec![], vec![]);
        let mut grown = BTreeSet::new();
        let mut queue: Vec<usize> = (0..ops.len().min(MAX_OPS))
            .filter(|index| !idle.contains(index))
            .collect();
        // An edit can depend on a later one (empty a branch, then remove it), so whatever
        // was refused is tried again for as long as the pass before it changed the map.
        loop {
            let before = applied.len();
            rejected.clear();
            for &index in &queue {
                let op = &ops[index];
                let mut trial = work.clone();
                // Whole but for its width, which is judged where the turn ends.
                let result = permit(&work, op, tier, holds, &grown)
                    .and_then(|()| step::step(&mut trial, op, tier.user()))
                    .and_then(|()| trial.validate_midway());
                let on = |s: &Session, id: &str| s.categories.iter().any(|c| c.id == id);
                let spent = matches!(op, Op::Remove { branch } if on(current, branch) && !on(&work, branch));
                match result {
                    Ok(()) if trial == work => {}
                    Ok(()) => {
                        if let Op::Branch { id, .. } = op {
                            grown.insert((index, id.clone()));
                        }
                        work = trial;
                        applied.push(index);
                    }
                    // A branch an earlier edit of this turn folded away needs no removing.
                    Err(_) if spent => {}
                    Err(reason) => rejected.push((index, reason)),
                }
            }
            if rejected.is_empty() || applied.len() == before {
                break;
            }
            queue = rejected.iter().map(|(index, _)| *index).collect();
        }
        // A branch grown this turn and left bare is scaffolding, unless the user asked for
        // it. Go again without it. One the turn took down again itself has left nothing.
        let stands = |id: &String| work.categories.iter().any(|c| c.id == *id);
        let bare: Vec<_> = grown
            .iter()
            .filter(|(_, id)| !tier.user() && stands(id) && empty(&work, id))
            .map(|(index, _)| *index)
            .collect();
        if !bare.is_empty() {
            idle.extend(bare);
            continue;
        }
        // The placer cannot remove a branch, so one its own re-filing emptied goes with the turn.
        let mut tidy = vec![];
        while tier == Tier::Place && applied.len() + tidy.len() < MAX_OPS {
            let emptied = |id: &String| {
                !empty(current, id)
                    && empty(&work, id)
                    && work.categories.iter().any(|c| c.id == *id)
            };
            let Some(branch) = current
                .categories
                .iter()
                .map(|c| &c.id)
                .find(|id| emptied(id))
            else {
                break;
            };
            let op = Op::Remove {
                branch: branch.clone(),
            };
            let mut trial = work.clone();
            if step::step(&mut trial, &op, false)
                .and_then(|()| trial.validate_midway())
                .is_err()
            {
                break;
            }
            work = trial;
            tidy.push(op);
        }
        // Regrouping passes through a wider map, but no turn may leave one wider than it is
        // allowed. Its newest main branch goes, and whatever hung from it is refused for want
        // of it.
        let main = |id: &str| {
            let mut roots = work.categories.iter().filter(|c| c.parent.is_none());
            roots.any(|c| c.id == id)
        };
        let newest = applied.iter().rev().find(|index| match &ops[**index] {
            Op::Branch {
                id, parent: None, ..
            } => main(id),
            Op::Move {
                branch,
                parent: None,
            } => main(branch),
            _ => false,
        });
        if let Some(index) = newest.filter(|_| step::wide(current, &work, tier.user())) {
            idle.insert(*index);
            wide.push(*index);
            continue;
        }
        work.refresh_reading();
        let wide = wide.into_iter().map(|index| (index, tier.full()));
        return Outcome {
            session: work,
            ops: applied
                .into_iter()
                .map(|index| ops[index].clone())
                .chain(tidy)
                .collect(),
            rejected: wide
                .chain(rejected)
                .map(|(index, reason)| Rejection {
                    op: ops[index].clone(),
                    reason,
                })
                .collect(),
        };
    }
}
pub fn empty(session: &Session, branch: &str) -> bool {
    !session
        .thoughts
        .iter()
        .any(|t| t.category.as_deref() == Some(branch))
        && !session
            .categories
            .iter()
            .any(|c| c.parent.as_deref() == Some(branch))
}
fn permit(
    session: &Session,
    op: &Op,
    tier: Tier,
    holds: &BTreeSet<u64>,
    grown: &BTreeSet<(usize, String)>,
) -> Result<(), String> {
    let touched: Vec<u64> = match op {
        Op::Place { thought, .. } | Op::Park { thought, .. } => vec![*thought],
        Op::Combine { into, from, .. } => from.iter().chain([into]).copied().collect(),
        _ => vec![],
    };
    if touched.iter().any(|id| holds.contains(id)) {
        return Err("That capture is still being split into thoughts; leave it alone".into());
    }
    if tier != Tier::Place {
        return Ok(());
    }
    let waiting = |id: &u64| {
        session
            .thoughts
            .iter()
            .any(|t| t.id == *id && t.category.is_none())
    };
    let main = |branch: Option<&str>| session.category_path(branch).first().map(|c| &c.id);
    // A filed thought follows a branch grown this turn: a new main branch, or a finer part
    // of the main branch it already hangs from. Regrouping across the map is the steward's.
    let follows = |thought: &u64, branch: &String| {
        let home = session.thoughts.iter().find(|t| t.id == *thought);
        let (home, part) = (
            home.and_then(|t| main(t.category.as_deref())),
            main(Some(branch)),
        );
        grown.iter().any(|(_, id)| id == branch) && (part == Some(branch) || part == home)
    };
    match op {
        Op::Branch { .. } => Ok(()),
        Op::Title { .. } if session.title == Session::default().title => Ok(()),
        // A new thought can be the reason a "not yet" thought finally has somewhere to go.
        Op::Place {
            thought, branch, ..
        } if waiting(thought)
            || follows(thought, branch)
            || session
                .parked
                .iter()
                .any(|p| p.thought.id == *thought && p.kind == ParkKind::Later) =>
        {
            Ok(())
        }
        Op::Place { .. } => Err(
            "The placer re-files a thought only into a main branch it has just grown, or a new sub-branch of the thought's own main branch".into(),
        ),
        Op::Park { thought, .. } if waiting(thought) => Ok(()),
        Op::Park { .. } => Err("The placer parks only thoughts that are not filed yet".into()),
        _ => Err("Only the steward restructures the map".into()),
    }
}
