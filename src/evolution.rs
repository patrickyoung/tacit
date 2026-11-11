//! The source ledger: the words behind every thought, and the thoughts retired from the map.
//! Whole-map revisions are no longer written, since the map agent edits by operation; `Plan`
//! stays so that journals holding one still replay.
use crate::model::{
    Category, Interpretation, Relationship, Role, Session, Thought, validate_concept,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type Sources = BTreeMap<String, String>;
pub fn source_ids(thought: &Thought) -> Vec<String> {
    if thought.evidence.is_empty() {
        vec![format!("thought-{}", thought.id)]
    } else {
        thought.evidence.clone()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concept {
    pub id: u64,
    pub concept: String,
    pub sources: Vec<String>,
    pub category: String,
    pub role: Role,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retirement {
    pub id: u64,
    pub reason: String,
    pub replaced_by: Vec<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetiredThought {
    pub thought: Thought,
    pub reason: String,
    pub replaced_by: Vec<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub explanation: String,
    pub title: String,
    pub narrative: String,
    pub categories: Vec<Category>,
    pub concepts: Vec<Concept>,
    pub retirements: Vec<Retirement>,
    pub relationships: Vec<Relationship>,
}
impl Plan {
    pub fn apply(&self, current: &Session, sources: &Sources) -> Result<Session, String> {
        if self.explanation.trim().is_empty()
            || self.explanation.len() > 4000
            || self.concepts.len() > 200
        {
            return Err("Invalid editorial explanation or map size".into());
        }
        crate::model::validate_categories(&self.categories)?;
        let old_ids = current
            .thoughts
            .iter()
            .map(|t| t.id)
            .collect::<BTreeSet<_>>();
        let mut ids = BTreeSet::new();
        let mut next = current.clone();
        next.sources = sources.clone();
        next.thoughts.clear();
        next.categories = self.categories.clone();
        for c in &self.concepts {
            validate_concept(&c.concept)?;
            if c.id == 0
                || !ids.insert(c.id)
                || (!old_ids.contains(&c.id) && c.id < current.next_id)
            {
                return Err("Revision reused a historical or duplicate concept ID".into());
            }
            if c.sources.is_empty()
                || c.sources.len() > 200
                || c.sources.iter().collect::<BTreeSet<_>>().len() != c.sources.len()
            {
                return Err("Every concept needs distinct source references".into());
            }
            if !self.categories.iter().any(|cat| cat.id == c.category) {
                return Err("Concept has no valid classification".into());
            }
            let text = c
                .sources
                .iter()
                .map(|id| {
                    sources
                        .get(id)
                        .map(String::as_str)
                        .ok_or("Unknown source reference")
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("\n\n");
            if text.len() > 1024 * 1024 {
                return Err("Concept evidence is too large".into());
            }
            let old = current.thoughts.iter().find(|t| t.id == c.id);
            if let Some(old) = old.filter(|t| t.pinned) {
                if old.role != c.role
                    || old.category.as_deref().is_some_and(|id| id != c.category)
                    || source_ids(old) != c.sources
                {
                    return Err("Revision changed an explicitly pinned concept".into());
                }
                for parent in current.category_path(old.category.as_deref()) {
                    if !self.categories.iter().any(|c| {
                        c.id == parent.id && c.name == parent.name && c.parent == parent.parent
                    }) {
                        return Err("Revision moved a pinned classification path".into());
                    }
                }
            }
            next.thoughts.push(Thought {
                id: c.id,
                text,
                concept: Some(c.concept.clone()),
                evidence: c.sources.clone(),
                role: c.role,
                category: Some(c.category.clone()),
                pinned: old.is_some_and(|t| t.pinned),
                kept: old.is_some_and(|t| t.kept),
            });
        }
        // Model IDs are deterministic and cannot reserve or reuse arbitrary history IDs.
        for id in ids.iter().filter(|id| **id >= current.next_id) {
            if *id != next.next_id {
                return Err("New concept IDs must be consecutive".into());
            }
            next.next_id = next.next_id.checked_add(1).ok_or("Concept IDs exhausted")?;
        }
        let removed = old_ids.difference(&ids).copied().collect::<BTreeSet<_>>();
        let mut retired = BTreeSet::new();
        for r in &self.retirements {
            if !removed.contains(&r.id)
                || !retired.insert(r.id)
                || r.reason.trim().is_empty()
                || r.reason.len() > 1000
                || r.replaced_by.iter().any(|id| !ids.contains(id))
                || r.replaced_by.iter().collect::<BTreeSet<_>>().len() != r.replaced_by.len()
            {
                return Err(
                    "Every retired concept needs a reason and valid replacement links".into(),
                );
            }
            let mut thought = current
                .thoughts
                .iter()
                .find(|t| t.id == r.id)
                .unwrap()
                .clone();
            if thought.pinned {
                return Err("Unpin a concept before retiring it".into());
            }
            thought.evidence = source_ids(&thought);
            next.retired.push(RetiredThought {
                thought,
                reason: r.reason.clone(),
                replaced_by: r.replaced_by.clone(),
            });
        }
        if removed != retired {
            return Err("Revision omitted concepts without explaining their retirement".into());
        }
        let covered = next
            .thoughts
            .iter()
            .chain(next.retired.iter().map(|r| &r.thought))
            .flat_map(source_ids)
            .collect::<BTreeSet<_>>();
        for id in current.thoughts.iter().flat_map(source_ids) {
            if !covered.contains(&id) {
                return Err("Revision orphaned existing evidence".into());
            }
        }
        next.title = self.title.clone();
        next.interpretation = Some(Interpretation {
            title: self.title.clone(),
            narrative: self.narrative.clone(),
            categories: self.categories.clone(),
            assignments: next
                .thoughts
                .iter()
                .map(|t| crate::model::Assignment {
                    id: t.id,
                    concept: t.concept.clone(),
                    role: t.role,
                    category: t.category.clone(),
                })
                .collect(),
            relationships: self.relationships.clone(),
        });
        if let Some(charter) = &mut next.charter {
            charter.narrative.clone_from(&self.narrative);
            charter.links.clone_from(&self.relationships);
        }
        next.validate()?;
        Ok(next)
    }
}
