//! What each edit does to a session. Pure and deterministic: this is what replays.
//! Errors are written for the model that proposed the edit, so it can correct itself.
//!
//! `user` marks an edit the user asked for. The agent's own edits leave alone whatever the
//! user pinned, kept on the map or parked; the user's edits are how those decisions change.
use super::Op;
use crate::evolution::{RetiredThought, source_ids};
use crate::model::{
    Category, MAX_LINKS, Relationship, Session, validate_concept, validate_hierarchy,
    validate_topic,
};
use std::collections::BTreeSet;

/// Main branches an agent leaves a map with. A person can still add an eighth by direction.
pub const MAX_ROOTS: usize = 7;

fn roots(session: &Session) -> usize {
    let roots = session.categories.iter().filter(|c| c.parent.is_none());
    roots.count()
}
/// A turn may pass through a wider map while it regroups: new main branches arrive before
/// the old ones fold into them. It may not leave one wider than it found, or than seven
/// main branches, eight on the user's word.
pub(super) fn wide(before: &Session, after: &Session, user: bool) -> bool {
    roots(after) > (MAX_ROOTS + usize::from(user)).max(roots(before))
}

/// A branch carries a keyword, not a sentence.
pub fn label(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name != name.trim()
        || name.chars().count() > 36
        || name.split_whitespace().count() > 5
        || name.contains(['\n', '\r'])
    {
        return Err(format!(
            "Branch name {name:?} must be a keyword label: one to five words, at most 36 characters"
        ));
    }
    Ok(())
}
fn slug(id: &str) -> Result<(), String> {
    let plain = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
    if id.is_empty() || id.len() > 60 || !id.bytes().all(plain) {
        return Err(format!(
            "Branch ID {id:?} must be a lowercase slug such as team-trust"
        ));
    }
    Ok(())
}
fn known(session: &Session, id: &str) -> Result<usize, String> {
    session
        .categories
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| format!("There is no branch {id}"))
}
/// The user pinned a thought on this branch's path, which fixes the path for the agent.
fn fixed(session: &Session, id: &str, user: bool) -> Result<(), String> {
    if !user
        && session
            .thoughts
            .iter()
            .any(|t| t.pinned && session.within_category(t, id))
    {
        return Err(format!(
            "Branch {id} holds a thought the user pinned; leave its name and position alone"
        ));
    }
    Ok(())
}
fn clash(session: &Session, id: &str, parent: Option<&str>, name: &str) -> Result<(), String> {
    let name = name.to_lowercase();
    if session
        .categories
        .iter()
        .any(|c| c.id != id && c.parent.as_deref() == parent && c.name.to_lowercase() == name)
    {
        return Err(format!("A sibling branch is already named {name:?}"));
    }
    Ok(())
}

pub(super) fn step(s: &mut Session, op: &Op, user: bool) -> Result<(), String> {
    match op {
        Op::Branch { id, name, parent } => {
            if let Some(old) = s.categories.iter().find(|c| c.id == *id) {
                return if old.name == *name && old.parent == *parent {
                    Ok(())
                } else {
                    Err(format!("Branch {id} already exists; rename or move it"))
                };
            }
            slug(id)?;
            label(name)?;
            if let Some(parent) = parent {
                known(s, parent)?;
            }
            clash(s, id, parent.as_deref(), name)?;
            s.categories.push(Category {
                parent: parent.clone(),
                illustration: None,
                motif: None,
                id: id.clone(),
                name: name.clone(),
            });
            validate_hierarchy(&s.categories)
        }
        Op::Rename { branch, name } => {
            let index = known(s, branch)?;
            if s.categories[index].name == *name {
                return Ok(());
            }
            label(name)?;
            fixed(s, branch, user)?;
            clash(s, branch, s.categories[index].parent.as_deref(), name)?;
            s.categories[index].name.clone_from(name);
            Ok(())
        }
        Op::Move { branch, parent } => {
            let index = known(s, branch)?;
            if s.categories[index].parent == *parent {
                return Ok(());
            }
            if let Some(parent) = parent {
                known(s, parent)?;
            }
            fixed(s, branch, user)?;
            clash(s, branch, parent.as_deref(), &s.categories[index].name)?;
            s.categories[index].parent.clone_from(parent);
            validate_hierarchy(&s.categories)
        }
        Op::Merge { from, into } => {
            let index = known(s, from)?;
            known(s, into)?;
            if from == into {
                return Err(format!("Branch {from} cannot merge into itself"));
            }
            fixed(s, from, user)?;
            if s.category_path(Some(into)).iter().any(|c| c.id == *from) {
                return Err(format!("Branch {into} is inside {from}; move it out first"));
            }
            for t in &mut s.thoughts {
                if t.category.as_deref() == Some(from) {
                    t.category = Some(into.clone());
                }
            }
            for c in &mut s.categories {
                if c.parent.as_deref() == Some(from) {
                    c.parent = Some(into.clone());
                }
            }
            s.categories.remove(index);
            validate_hierarchy(&s.categories)
        }
        Op::Remove { branch } => {
            known(s, branch)?;
            let held = s.thoughts.iter().filter(|t| s.within_category(t, branch));
            let held = held.count();
            if held > 0 {
                return Err(format!(
                    "Branch {branch} still holds {held} thought(s), its sub-branches counted; merge it or move them first"
                ));
            }
            // Sub-branches that hold nothing are scaffolding, and go with it.
            let under = |c: &Category| s.category_path(Some(&c.id)).iter().any(|p| p.id == *branch);
            let gone: BTreeSet<String> = s
                .categories
                .iter()
                .filter(|c| under(c))
                .map(|c| c.id.clone())
                .collect();
            s.categories.retain(|c| !gone.contains(&c.id));
            Ok(())
        }
        Op::Order { branches } => {
            // A branch that has gone since is passed over, as a link to a thought that has
            // gone is: the order of the rest still stands.
            let seat = |id: &String| s.categories.iter().position(|c| c.id == *id);
            let mut seats: Vec<usize> = vec![];
            for at in branches.iter().filter_map(seat) {
                if !seats.contains(&at) {
                    seats.push(at);
                }
            }
            // The seats the named branches hold between them, refilled in the order given:
            // every other branch stays where it is.
            let named: Vec<Category> = seats.iter().map(|at| s.categories[*at].clone()).collect();
            seats.sort_unstable();
            for (seat, branch) in seats.into_iter().zip(named) {
                s.categories[seat] = branch;
            }
            Ok(())
        }
        Op::Place {
            thought,
            branch,
            concept,
            role,
        } => {
            known(s, branch)?;
            if let Some(concept) = concept {
                validate_concept(concept)?;
            }
            let back = match s.parked.iter().find(|p| p.thought.id == *thought) {
                Some(p) if p.held && !user => {
                    return Err(format!("The user keeps thought {thought} parked"));
                }
                Some(_) => {
                    s.revive(*thought)?;
                    true
                }
                None => false,
            };
            let t = s
                .thoughts
                .iter_mut()
                .find(|t| t.id == *thought)
                .ok_or_else(|| format!("There is no thought {thought}"))?;
            if !user && t.pinned && t.category.as_deref().is_some_and(|c| c != branch) {
                return Err(format!(
                    "The user pinned thought {thought} where it is; do not move it"
                ));
            }
            if concept.is_none() && t.concept.is_none() {
                return Err(format!("Thought {thought} needs a concept label"));
            }
            if let Some(concept) = concept {
                t.concept = Some(concept.clone());
            }
            if let Some(role) = role.filter(|_| user || !t.pinned) {
                t.role = role;
            }
            t.category = Some(branch.clone());
            // What the user brings back, the agent does not park again.
            t.kept |= user && back;
            Ok(())
        }
        Op::Pin { thought, branch } => {
            if !user {
                return Err("Only the user pins a thought to a branch".into());
            }
            let home = Some(branch.as_str());
            let mut filed = s.thoughts.iter_mut();
            filed
                .find(|t| t.id == *thought && t.category.as_deref() == home)
                .ok_or_else(|| format!("Thought {thought} is not in branch {branch}"))?
                .pinned = true;
            Ok(())
        }
        Op::Park {
            thought,
            kind,
            topic,
            reason,
            concept,
        } => {
            validate_topic(topic)?;
            if reason.chars().count() > 300 {
                return Err("Give the reason for parking in one short sentence".into());
            }
            if let Some(concept) = concept {
                validate_concept(concept)?;
            }
            if let Some(p) = s.parked.iter_mut().find(|p| p.thought.id == *thought) {
                if p.held && !user {
                    return Err(format!("The user decided thought {thought} stays parked"));
                }
                (p.kind, p.topic, p.reason) = (*kind, topic.clone(), reason.clone());
                p.held |= user;
                if concept.is_some() {
                    p.thought.concept.clone_from(concept);
                }
                return Ok(());
            }
            let t = s
                .thoughts
                .iter_mut()
                .find(|t| t.id == *thought)
                .ok_or_else(|| format!("There is no thought {thought}"))?;
            if !user && (t.pinned || t.kept) {
                return Err(format!(
                    "The user wants thought {thought} on the map; place it instead"
                ));
            }
            if concept.is_some() {
                t.concept.clone_from(concept);
            }
            // What the user parks stays parked until they bring it back.
            s.park(*thought, *kind, topic, reason, user)
        }
        Op::Combine {
            into,
            from,
            concept,
        } => combine(s, *into, from, concept, user),
        Op::Links { links } => {
            let live: BTreeSet<u64> = s.thoughts.iter().map(|t| t.id).collect();
            let mut kept: Vec<Relationship> = vec![];
            for link in links {
                if link.label.trim().is_empty() || link.label.chars().count() > 60 {
                    return Err("A link needs a label of at most 60 characters".into());
                }
                // Links are undirected for our purposes; one per pair, between thoughts on the map.
                let pair = |r: &Relationship| (r.from.min(r.to), r.from.max(r.to));
                if link.from != link.to
                    && live.contains(&link.from)
                    && live.contains(&link.to)
                    && !kept.iter().any(|k| pair(k) == pair(link))
                {
                    kept.push(link.clone());
                }
            }
            kept.truncate(MAX_LINKS);
            s.charter.get_or_insert_with(Default::default).links = kept;
            Ok(())
        }
        Op::Title { title } => {
            if title.trim().is_empty()
                || title != title.trim()
                || title.chars().count() > 100
                || title.contains(['\n', '\r'])
            {
                return Err("A title is one short line".into());
            }
            s.title.clone_from(title);
            Ok(())
        }
        Op::Narrative { narrative } => {
            if narrative.trim().is_empty() || narrative.chars().count() > 3000 {
                return Err("The narrative must be present and under 3000 characters".into());
            }
            s.charter
                .get_or_insert_with(Default::default)
                .narrative
                .clone_from(narrative);
            Ok(())
        }
        Op::Focus { focus, principle } => {
            let charter = s.charter.get_or_insert_with(Default::default);
            if let Some(focus) = focus {
                charter.focus.clone_from(focus);
            }
            if let Some(principle) = principle {
                charter.principle.clone_from(principle);
            }
            charter.validate()
        }
    }
}

/// Fold repeats of one idea into `into`. Every participant's words stay as cited sources and
/// the absorbed thoughts move to the retired history, so the combination can be explained.
fn combine(
    s: &mut Session,
    into: u64,
    from: &[u64],
    concept: &str,
    user: bool,
) -> Result<(), String> {
    validate_concept(concept)?;
    let mut distinct = BTreeSet::from([into]);
    if from.is_empty() || from.iter().any(|id| !distinct.insert(*id)) {
        return Err("Combine needs two or more different thoughts".into());
    }
    let (mut evidence, mut words, mut kept) = (Vec::<String>::new(), Vec::<String>::new(), false);
    for id in [into].iter().chain(from) {
        let t = s
            .thoughts
            .iter()
            .find(|t| t.id == *id)
            .ok_or_else(|| format!("There is no thought {id} on the map"))?;
        if t.pinned && !user {
            return Err(format!("The user pinned thought {id}; do not combine it"));
        }
        kept |= t.kept;
        for key in source_ids(t) {
            let text = match s.sources.get(&key) {
                Some(text) => text.clone(),
                None if t.evidence.is_empty() => t.text.clone(),
                None => return Err("Concept references missing source words".into()),
            };
            if !evidence.contains(&key) {
                evidence.push(key.clone());
                words.push(text.clone());
            }
            s.sources.insert(key, text);
        }
    }
    if evidence.len() > 200 || words.iter().map(String::len).sum::<usize>() > 1024 * 1024 {
        return Err("Too much evidence for one thought; keep them separate".into());
    }
    for id in from {
        let index = s.thoughts.iter().position(|t| t.id == *id).unwrap();
        let mut thought = s.thoughts.remove(index);
        thought.evidence = source_ids(&thought);
        s.retired.push(RetiredThought {
            thought,
            reason: "Said again elsewhere; combined into one idea".into(),
            replaced_by: vec![into],
        });
    }
    let t = s.thoughts.iter_mut().find(|t| t.id == into).unwrap();
    t.text = words.join("\n\n");
    t.evidence = evidence;
    t.concept = Some(concept.into());
    t.kept = kept;
    if let Some(charter) = &mut s.charter {
        for link in &mut charter.links {
            for end in [&mut link.from, &mut link.to] {
                if from.contains(end) {
                    *end = into;
                }
            }
        }
        charter.links.retain(|l| l.from != l.to);
    }
    Ok(())
}
