//! The facilitator commands of journals written before the map agent. What the user asks of
//! the map is now carried out by the agent's directed turn (`agent::guided`); these edits are
//! no longer written, and stay so that journals holding them still replay.
use crate::model::{Category, Session};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    RefineCategories {
        classification: Classification,
    },
    RememberGuidance,
    RenameTheme {
        theme: String,
        name: String,
    },
    CreateTheme {
        theme: String,
        name: String,
    },
    MergeThemes {
        source: String,
        target: String,
    },
    ReparentTheme {
        theme: String,
        parent: Option<String>,
    },
    MoveThought {
        thought: u64,
        theme: String,
    },
    OrderThemes {
        themes: Vec<String>,
    },
    OrderThoughts {
        thoughts: Vec<u64>,
    },
    SetTitle {
        title: String,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub id: u64,
    pub category: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Classification {
    pub categories: Vec<Category>,
    pub placements: Vec<Placement>,
}
impl Classification {
    fn apply(&self, current: &Session) -> Result<Session, String> {
        crate::model::validate_categories(&self.categories)?;
        if self.placements.len() != current.thoughts.len() || self.placements.len() > 200 {
            return Err("Category refinement must place every thought exactly once".into());
        }
        let mut next = current.clone();
        let mut assigned = std::collections::BTreeSet::new();
        for placement in &self.placements {
            if !assigned.insert(placement.id)
                || !self.categories.iter().any(|c| c.id == placement.category)
            {
                return Err("Invalid category refinement placement".into());
            }
            let thought = next
                .thoughts
                .iter_mut()
                .find(|t| t.id == placement.id)
                .ok_or("Category refinement referenced an unknown thought")?;
            if thought.pinned {
                if thought
                    .category
                    .as_ref()
                    .is_some_and(|id| id != &placement.category)
                {
                    return Err("Category refinement cannot move a pinned thought".into());
                }
                for parent in current.category_path(thought.category.as_deref()) {
                    if !self.categories.iter().any(|c| {
                        c.id == parent.id && c.name == parent.name && c.parent == parent.parent
                    }) {
                        return Err("Category refinement cannot change a pinned path".into());
                    }
                }
            }
            thought.category = Some(placement.category.clone());
        }
        next.categories = self
            .categories
            .iter()
            .cloned()
            .map(|mut category| {
                if let Some(old) = current.categories.iter().find(|c| c.id == category.id) {
                    category.motif = old.motif;
                    category.illustration.clone_from(&old.illustration);
                }
                category
            })
            .collect();
        Ok(next)
    }
}
pub fn apply(session: &Session, instruction: &str, edits: &[Edit]) -> Result<Session, String> {
    if instruction.trim().is_empty() || instruction.len() > 8000 || edits.len() > 40 {
        return Err("Direction is empty or too large".into());
    }
    let mut next = session.clone();
    for edit in edits {
        match edit {
            Edit::RefineCategories { classification } => {
                next = classification.apply(&next)?;
            }
            Edit::RememberGuidance => {}
            Edit::RenameTheme { theme, name } => {
                next.categories
                    .iter_mut()
                    .find(|c| &c.id == theme)
                    .ok_or("Unknown branch")?
                    .name = name.clone();
            }
            Edit::CreateTheme { theme, name } => {
                if next.categories.iter().any(|c| &c.id == theme) {
                    return Err("Branch already exists".into());
                }
                next.categories.push(Category {
                    parent: None,
                    id: theme.clone(),
                    name: name.clone(),
                    motif: None,
                    illustration: None,
                });
            }
            Edit::MergeThemes { source, target } => {
                if source == target
                    || !next.categories.iter().any(|c| &c.id == source)
                    || !next.categories.iter().any(|c| &c.id == target)
                {
                    return Err("Invalid branches to merge".into());
                }
                let affected: Vec<_> = next
                    .thoughts
                    .iter()
                    .filter(|t| next.within_category(t, source))
                    .map(|t| t.id)
                    .collect();
                for t in &mut next.thoughts {
                    if affected.contains(&t.id) {
                        t.pinned = true;
                    }
                    if t.category.as_ref() == Some(source) {
                        t.category = Some(target.clone());
                        t.pinned = true;
                    }
                }
                for c in &mut next.categories {
                    if c.parent.as_ref() == Some(source) {
                        c.parent = Some(target.clone());
                    }
                }
                next.categories.retain(|c| &c.id != source);
            }
            Edit::ReparentTheme { theme, parent } => {
                let affected: Vec<_> = next
                    .thoughts
                    .iter()
                    .filter(|t| next.within_category(t, theme))
                    .map(|t| t.id)
                    .collect();
                next.categories
                    .iter_mut()
                    .find(|c| &c.id == theme)
                    .ok_or("Unknown branch")?
                    .parent = parent.clone();
                for t in &mut next.thoughts {
                    if affected.contains(&t.id) {
                        t.pinned = true;
                    }
                }
            }
            Edit::MoveThought { thought, theme } => {
                if !next.categories.iter().any(|c| &c.id == theme) {
                    return Err("Unknown destination branch".into());
                }
                let t = next
                    .thoughts
                    .iter_mut()
                    .find(|t| t.id == *thought)
                    .ok_or("Unknown thought")?;
                t.category = Some(theme.clone());
                t.pinned = true;
            }
            Edit::OrderThemes { themes } => {
                let ids: std::collections::HashSet<_> = themes.iter().collect();
                if ids.len() != themes.len()
                    || themes.len() != next.categories.len()
                    || next.categories.iter().any(|c| !ids.contains(&c.id))
                {
                    return Err("Branch order must contain every branch exactly once".into());
                }
                next.categories
                    .sort_by_key(|c| themes.iter().position(|id| id == &c.id).unwrap());
            }
            Edit::OrderThoughts { thoughts } => {
                let ids: std::collections::HashSet<_> = thoughts.iter().collect();
                if ids.len() != thoughts.len()
                    || thoughts.len() != next.thoughts.len()
                    || next.thoughts.iter().any(|t| !ids.contains(&t.id))
                {
                    return Err("Thought order must contain every thought exactly once".into());
                }
                next.thoughts
                    .sort_by_key(|t| thoughts.iter().position(|id| *id == t.id).unwrap());
            }
            Edit::SetTitle { title } => {
                if title.trim().is_empty() || title.chars().count() > 100 {
                    return Err("Invalid map title".into());
                }
                next.title = title.clone();
            }
        }
    }
    if !edits.is_empty() {
        // Layout edits do not discard the existing narrative or its evidence links.
        if let Some(interpretation) = &mut next.interpretation {
            interpretation.title = next.title.clone();
            interpretation.categories = next.categories.clone();
            interpretation.assignments = next
                .thoughts
                .iter()
                .map(|t| crate::model::Assignment {
                    concept: t.concept.clone(),
                    id: t.id,
                    role: t.role,
                    category: t.category.clone(),
                })
                .collect();
        }
        next.directions.push(instruction.trim().into());
        if next.directions.len() > 20 {
            next.directions.remove(0);
        }
    }
    next.validate()?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_are_atomic_preserve_words_and_pin_explicit_moves() {
        let mut s = Session::demo();
        s.categories = vec![
            Category {
                parent: None,
                id: "a".into(),
                name: "First".into(),
                motif: None,
                illustration: None,
            },
            Category {
                parent: None,
                id: "b".into(),
                name: "Second".into(),
                motif: None,
                illustration: None,
            },
        ];
        let renamed = [
            Edit::RenameTheme {
                theme: "a".into(),
                name: "Renamed".into(),
            },
            Edit::OrderThemes {
                themes: vec!["b".into(), "a".into()],
            },
        ];
        let edited = apply(&s, "Rename First and put Second first", &renamed).unwrap();
        assert_eq!(edited.thoughts, s.thoughts);
        assert_eq!(edited.categories[1].name, "Renamed");
        let short = [Edit::OrderThemes {
            themes: vec!["b".into()],
        }];
        assert!(apply(&s, "order", &short).is_err());
        let original = s.clone();
        let edits = vec![
            Edit::MoveThought {
                thought: 1,
                theme: "b".into(),
            },
            Edit::OrderThemes {
                themes: vec!["b".into(), "a".into()],
            },
        ];
        let next = apply(&s, "Move calendars into Second and put it first", &edits).unwrap();
        assert_eq!(next.thoughts[0].text, s.thoughts[0].text);
        assert!(next.thoughts[0].pinned);
        assert_eq!(next.categories[0].id, "b");
        assert_eq!(next.thoughts.len(), s.thoughts.len());
        assert_eq!(next.directions.len(), 1);
        assert!(
            apply(
                &s,
                "bad",
                &[
                    edits[0].clone(),
                    Edit::MoveThought {
                        thought: 999,
                        theme: "a".into()
                    }
                ]
            )
            .is_err()
        );
        assert_eq!(s, original);
        assert!(
            apply(
                &s,
                "bad",
                &[Edit::OrderThemes {
                    themes: vec!["a".into(), "a".into()]
                }]
            )
            .is_err()
        );
    }
}
