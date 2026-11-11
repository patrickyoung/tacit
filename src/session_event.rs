//! Portable content events. The same pure reducer is used during live dispatch and replay.
//! No clocks, model requests, UI handles, or layout decisions belong here.
use crate::model::{Interpretation, ParkKind, Role, Session, Thought, classify};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEvent {
    Unpinned {
        id: u64,
    },
    Directed {
        instruction: String,
        edits: Vec<crate::direction::Edit>,
    },
    Captured {
        thought: Thought,
    },
    PlacementCorrected {
        id: u64,
        role: Role,
        category: Option<String>,
    },
    Removed {
        id: u64,
    },
    Split {
        source_id: u64,
        original: String,
        chunks: Vec<String>,
    },
    Organized {
        interpretation: Interpretation,
        pass: String,
    },
    Restored {
        session: Box<Session>,
        reason: String,
    },
    /// One turn of the mind map agent: an ordered list of edits that all applied.
    Mapped {
        ops: Vec<crate::agent::Op>,
        pass: String,
        note: String,
    },
    /// A turn of the mind map agent that carried out the user's instruction, with their
    /// authority over what they had pinned, kept or parked.
    Guided {
        instruction: String,
        ops: Vec<crate::agent::Op>,
        note: String,
    },
    /// The user set a thought aside. The agent may not bring it back.
    Parked {
        id: u64,
    },
    /// The user brought a parked thought back. The agent places it and may not park it again.
    Revived {
        id: u64,
    },
    /// The user decided a parked thought stays off the map.
    Held {
        id: u64,
    },
}
impl SessionEvent {
    /// Return a new validated state; an invalid event never partially changes its input.
    pub fn reduce(&self, current: &Session) -> Result<Session, String> {
        let mut next = current.clone();
        match self {
            Self::Unpinned { id } => {
                next.thoughts
                    .iter_mut()
                    .find(|t| t.id == *id)
                    .ok_or("Unknown thought")?
                    .pinned = false;
                next.interpretation = None;
            }
            Self::Directed { instruction, edits } => {
                next = crate::direction::apply(current, instruction, edits)?
            }
            Self::Captured { thought } => {
                if thought.id != next.next_id {
                    return Err("Capture has an unexpected thought ID".into());
                }
                next.next_id = next.next_id.checked_add(1).ok_or("Thought IDs exhausted")?;
                next.thoughts.push(thought.clone());
                next.interpretation = None;
            }
            Self::PlacementCorrected { id, role, category } => {
                let t = next
                    .thoughts
                    .iter_mut()
                    .find(|t| t.id == *id)
                    .ok_or("Unknown thought")?;
                t.role = *role;
                t.category = category.clone();
                t.pinned = true;
                next.interpretation = None;
            }
            Self::Removed { id } => {
                let before = next.thoughts.len() + next.parked.len();
                next.thoughts.retain(|t| t.id != *id);
                next.parked.retain(|p| p.thought.id != *id);
                if before == next.thoughts.len() + next.parked.len() {
                    return Err("Unknown thought".into());
                }
                next.interpretation = None;
            }
            Self::Split {
                source_id,
                original,
                chunks,
            } => {
                let index = next
                    .thoughts
                    .iter()
                    .position(|t| {
                        t.id == *source_id
                            && t.text == *original
                            && !t.pinned
                            && t.evidence.is_empty()
                    })
                    .ok_or("Split source has changed")?;
                if chunks.is_empty()
                    || chunks.iter().any(|c| c.trim().is_empty())
                    || chunks.concat() != *original
                {
                    return Err("Split must preserve all original words".into());
                }
                next.thoughts[index].concept = None;
                next.thoughts[index].text = chunks[0].trim().into();
                next.thoughts[index].role = classify(&chunks[0]);
                next.thoughts[index].category = None;
                for (offset, chunk) in chunks.iter().enumerate().skip(1) {
                    next.add(chunk);
                    let thought = next.thoughts.pop().unwrap();
                    next.thoughts.insert(index + offset, thought);
                }
                next.interpretation = None;
            }
            Self::Organized { interpretation, .. } => interpretation.clone().apply(&mut next)?,
            Self::Restored { session, .. } => {
                next = Session::clone(session);
                // Undo must never reuse an ID that historical transcript/artwork references may retain.
                next.next_id = next.next_id.max(current.next_id);
            }
            Self::Mapped { ops, .. } => next = crate::agent::replay(current, ops)?,
            Self::Guided {
                instruction, ops, ..
            } => next = crate::agent::guided(current, instruction, ops)?,
            Self::Parked { id } => {
                next.park(*id, ParkKind::OffTopic, "Set aside", "Parked by you", true)?;
                next.interpretation = None;
            }
            Self::Revived { id } => {
                next.revive(*id)?.kept = true;
                next.interpretation = None;
            }
            Self::Held { id } => {
                let p = next
                    .parked
                    .iter_mut()
                    .find(|p| p.thought.id == *id)
                    .ok_or("Unknown parked thought")?;
                if p.kind == ParkKind::Later {
                    p.kind = ParkKind::OffTopic;
                }
                p.held = true;
            }
        }
        // A map the agent tends keeps its reading in step with every other kind of change.
        if next.charter.is_some()
            && next.interpretation.is_none()
            && !matches!(self, Self::Restored { .. })
        {
            next.refresh_reading();
        }
        next.validate()?;
        Ok(next)
    }
    pub fn capture(session: &Session, text: &str) -> Self {
        Self::Captured {
            thought: Thought {
                evidence: vec![],
                concept: None,
                kept: false,
                id: session.next_id,
                text: text.trim().into(),
                role: classify(text.trim()),
                pinned: false,
                category: None,
            },
        }
    }
    pub fn correct_placement(session: &Session, id: u64) -> Result<Self, String> {
        let mut proposed = session.clone();
        proposed.move_thought(id);
        let t = proposed
            .thoughts
            .iter()
            .find(|t| t.id == id)
            .ok_or("Unknown thought")?;
        Ok(Self::PlacementCorrected {
            id,
            role: t.role,
            category: t.category.clone(),
        })
    }
}
