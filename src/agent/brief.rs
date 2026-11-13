//! What a turn is shown: the map as a mapper reads it from the wall, as an outline rather than
//! the session's JSON. Small enough to send every few seconds.
use super::turn::Stands;
use super::{MAX_ROOTS, Tier, health};
use crate::knowledge::{Kind, Library, Note};
use crate::model::{ParkKind, Session, Thought};
use std::collections::BTreeSet;
use std::fmt::Write;

/// New thoughts one placer turn takes on.
pub const BATCH: usize = 12;
/// Characters of original words the steward reads in one turn, newest first.
const WORDS: usize = 24_000;
/// Notes of the knowledge base a turn is shown a line of: the placer, the others, and for
/// those the notes one link on.
const NEAR_PLACE: usize = 5;
const NEAR: usize = 10;
const LINKED: usize = 4;
/// How many of the topics the nearest notes are in have their overview shown.
const OVERVIEWS: usize = 2;
/// How many more notes are asked for than shown, since only ideas are.
const SPARE: usize = 6;

/// One line of someone's words, however long they spoke.
fn clip(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max).collect();
    cut.push('…');
    cut
}
/// Thoughts on the map with no branch, oldest first, leaving alone captures still being split.
pub fn waiting(session: &Session, holds: &BTreeSet<u64>) -> Vec<u64> {
    let mut ids: Vec<u64> = session
        .thoughts
        .iter()
        .filter(|t| t.category.is_none() && !holds.contains(&t.id))
        .map(|t| t.id)
        .collect();
    ids.sort_unstable();
    ids
}
/// The narrative a model wrote for this map, if any: never the offline placeholder.
pub fn narrative(session: &Session) -> &str {
    let charter = session.charter.as_ref().map(|c| c.narrative.trim());
    let reading = session.interpretation.as_ref().map(|i| i.narrative.trim());
    charter
        .filter(|text| !text.is_empty())
        .or(reading)
        .unwrap_or_default()
}
fn kind(kind: ParkKind) -> &'static str {
    match kind {
        ParkKind::Later => "later",
        ParkKind::OffTopic => "off_topic",
        ParkKind::Aside => "aside",
    }
}
fn subject(session: &Session, out: &mut String, narrative: &str) {
    let charter = session.charter.clone().unwrap_or_default();
    let or = |text: &str| {
        if text.trim().is_empty() {
            "not settled yet".to_owned()
        } else {
            text.trim().to_owned()
        }
    };
    let title = if session.title == Session::default().title {
        "(placeholder, no title yet)"
    } else {
        &session.title
    };
    let _ = writeln!(
        out,
        "SUBJECT\nTitle: {title}\nFocus: {}\nOrdering idea: {}",
        or(&charter.focus),
        or(&charter.principle)
    );
    if !narrative.is_empty() {
        let _ = writeln!(out, "Narrative so far: {}", clip(narrative, 900));
    }
    if !session.directions.is_empty() {
        out.push_str("\nGUIDANCE\n");
        for direction in &session.directions {
            let _ = writeln!(out, "- {}", clip(direction, 400));
        }
    }
}
/// The map as an indented outline. `line` writes whatever follows a thought's number, and
/// `full` is how many main branches the reader may leave it with.
fn outline(
    session: &Session,
    out: &mut String,
    full: usize,
    line: &dyn Fn(&Thought, &str, &mut String),
) {
    fn branch(
        session: &Session,
        parent: Option<&str>,
        depth: usize,
        out: &mut String,
        line: &dyn Fn(&Thought, &str, &mut String),
    ) {
        let pad = "  ".repeat(depth);
        for c in session
            .categories
            .iter()
            .filter(|c| c.parent.as_deref() == parent)
        {
            let _ = writeln!(out, "{pad}[{}] {}", c.id, c.name);
            for t in session
                .thoughts
                .iter()
                .filter(|t| t.category.as_deref() == Some(&c.id))
            {
                let _ = write!(out, "{pad}  #{} ", t.id);
                line(t, &pad, out);
            }
            branch(session, Some(&c.id), depth + 1, out, line);
        }
    }
    let roots = session.categories.iter().filter(|c| c.parent.is_none());
    let roots = roots.count();
    out.push_str("\nMAP");
    if roots >= full {
        let _ = write!(out, " ({roots} main branches: full, it takes no more)");
    }
    out.push('\n');
    if session.categories.is_empty() {
        out.push_str(
            "(empty: its first main branches name parts of the subject, never the subject)\n",
        );
    }
    branch(session, None, 0, out, line);
}
/// What is said of a strain on a branch the user arranged, in place of how to mend it.
const THEIRS: &str = "This stands on the user's word: leave it as they arranged it.";
/// The checklist's findings. The placer hears only of strain: the rest is the steward's work,
/// which is told first, and in so many words, what it will be held to. A branch that strains
/// as the user arranged it is no fault: the steward is told to leave it, the placer nothing.
fn strains(session: &Session, out: &mut String, stands: &Stands, all: bool) {
    let mut findings = health::review(session);
    let theirs = |f: &health::Finding| f.strain && stands.theirs(session, f);
    findings.retain(|f| all || (f.strain && !theirs(f)));
    findings.sort_by_key(|f| (!f.strain, theirs(f)));
    if !findings.is_empty() {
        out.push_str("\nHEALTH\n");
        for f in findings.iter().take(10) {
            let _ = match (theirs(f), all && f.strain) {
                (true, _) => writeln!(out, "- {} {THEIRS}", f.fact),
                (false, true) => writeln!(out, "- STRAIN: {}", f.note()),
                (false, false) => writeln!(out, "- {}", f.note()),
            };
        }
    }
}

/// The placer's brief and the thoughts it must file, or nothing when none are waiting.
/// `stands` is what the user's instructions arranged.
pub fn place(
    session: &Session,
    holds: &BTreeSet<u64>,
    stands: &Stands,
) -> Option<(String, Vec<u64>)> {
    let mut targets = waiting(session, holds);
    targets.truncate(BATCH);
    if targets.is_empty() {
        return None;
    }
    let mut out = String::new();
    subject(session, &mut out, "");
    outline(session, &mut out, MAX_ROOTS, &|t, _, out| {
        let _ = writeln!(out, "{}", clip(t.map_label(), 80));
    });
    if !session.parked.is_empty() {
        out.push_str("\nPARKED\n");
        // Topics only, for what is not the placer's to bring back: enough to park more of
        // the same under one name.
        let (mut topics, mut held): (Vec<&str>, Vec<&str>) = (vec![], vec![]);
        for p in &session.parked {
            let known = match p.kind {
                ParkKind::Later if !p.held => {
                    // Its words, not its label: a thought that comes back is named afresh.
                    let _ = writeln!(
                        out,
                        "#{} later · topic {:?} · said: {:?}",
                        p.thought.id,
                        p.topic,
                        clip(&p.thought.text, 240)
                    );
                    continue;
                }
                ParkKind::Later => &mut held,
                ParkKind::OffTopic => &mut topics,
                ParkKind::Aside => continue,
            };
            if !known.contains(&p.topic.as_str()) {
                known.push(&p.topic);
            }
        }
        if !topics.is_empty() {
            let _ = writeln!(out, "off_topic topics so far: {}", topics.join(", "));
        }
        if !held.is_empty() {
            let _ = writeln!(out, "later topics held by the user: {}", held.join(", "));
        }
    }
    let mut recent: Vec<&Thought> = session
        .thoughts
        .iter()
        .filter(|t| t.category.is_some())
        .collect();
    recent.sort_unstable_by_key(|t| t.id);
    if !recent.is_empty() {
        out.push_str("\nRECENT\n");
        for t in &recent[recent.len().saturating_sub(4)..] {
            let _ = writeln!(
                out,
                "#{} in {}: {:?}",
                t.id,
                t.category.as_deref().unwrap_or_default(),
                clip(&t.text, 320)
            );
        }
    }
    out.push_str("\nNEW\n");
    for t in session.thoughts.iter().filter(|t| targets.contains(&t.id)) {
        let wanted = if t.kept || t.pinned {
            " (the user wants this on the map: place it)"
        } else {
            ""
        };
        let _ = writeln!(out, "#{}{wanted}: {:?}", t.id, clip(&t.text, 1500));
    }
    strains(session, &mut out, stands, false);
    Some((out, targets))
}

/// The whole map, the words behind it, and everything beside it. `user`: read on the user's
/// behalf, who may overrule what they held and take the map to its eighth main branch.
fn whole(session: &Session, holds: &BTreeSet<u64>, user: bool) -> String {
    // Words are what placement is judged by. Newest first when there are too many to send.
    let mut order: Vec<&Thought> = session
        .thoughts
        .iter()
        .chain(session.parked.iter().map(|p| &p.thought))
        .collect();
    order.sort_unstable_by_key(|t| std::cmp::Reverse(t.id));
    let (mut budget, mut worded) = (WORDS, BTreeSet::new());
    for t in order {
        let cost = t.text.chars().count().min(420);
        if cost > budget {
            break;
        }
        budget -= cost;
        worded.insert(t.id);
    }
    let said = |t: &Thought, pad: &str, out: &mut String| {
        if worded.contains(&t.id) {
            let _ = writeln!(out, "{pad}     said: {:?}", clip(&t.text, 420));
        }
    };
    let mut out = String::new();
    subject(session, &mut out, narrative(session));
    outline(
        session,
        &mut out,
        MAX_ROOTS + usize::from(user),
        &|t, pad, out| {
            let mut flags = String::new();
            for (on, flag) in [(t.pinned, " · pinned"), (t.kept, " · kept")] {
                if on {
                    flags.push_str(flag);
                }
            }
            let _ = writeln!(out, "{} · {:?}{flags}", clip(t.map_label(), 80), t.role);
            said(t, pad, out);
        },
    );
    let unfiled: Vec<&Thought> = session
        .thoughts
        .iter()
        .filter(|t| t.category.is_none() && !holds.contains(&t.id))
        .collect();
    if !unfiled.is_empty() {
        out.push_str("\nUNFILED\n");
        for t in unfiled {
            let wanted = if t.kept || t.pinned { " · kept" } else { "" };
            let _ = writeln!(out, "#{}{wanted}: {:?}", t.id, clip(&t.text, 1200));
        }
    }
    if !session.parked.is_empty() {
        out.push_str("\nPARKED\n");
        for p in &session.parked {
            let held = match (p.held, user) {
                (false, _) => "",
                (true, false) => " · held by the user: leave it",
                (true, true) => " · held by the user",
            };
            let _ = writeln!(
                out,
                "#{} {} · topic {:?} · {}{held}",
                p.thought.id,
                kind(p.kind),
                p.topic,
                clip(p.thought.map_label(), 80)
            );
            if user || !p.held {
                if !p.reason.trim().is_empty() {
                    let _ = writeln!(out, "     why: {}", clip(&p.reason, 200));
                }
                said(&p.thought, "", &mut out);
            }
        }
    }
    let links = session.charter.as_ref().map_or(&[][..], |c| &c.links);
    if !links.is_empty() {
        out.push_str("\nLINKS\n");
        for link in links {
            let _ = writeln!(out, "#{} → #{}: {}", link.from, link.to, link.label);
        }
    }
    out
}
/// The steward's brief: the whole map and where it strains, by its own fault or, in `stands`,
/// by the user's word.
pub fn steward(session: &Session, holds: &BTreeSet<u64>, stands: &Stands) -> String {
    let mut out = whole(session, holds, false);
    strains(session, &mut out, stands, true);
    out
}
/// A directed turn's brief: the whole map, then what the user asked of it. No checklist: the
/// instruction decides what changes, and the steward tends the rest.
pub fn direct(session: &Session, holds: &BTreeSet<u64>, instruction: &str) -> String {
    directed(session, holds, instruction, "")
}
/// The same with what the knowledge base holds on the map, from [`knowledge`]. The
/// instruction stays last.
pub fn directed(
    session: &Session,
    holds: &BTreeSet<u64>,
    instruction: &str,
    knowledge: &str,
) -> String {
    let mut out = whole(session, holds, true);
    out.push_str(knowledge);
    let _ = writeln!(out, "\nINSTRUCTION\n{}", instruction.trim());
    out
}

/// What the user's knowledge base holds that bears on this map, as a section of a brief, or
/// nothing when it has nothing to show. Context is loaded a level at a time. The placer is
/// shown a line of each note near the thoughts it is filing, and no section when none is.
/// The steward and a directed turn are shown the topics, the overview of the topics the map
/// falls in, a line of each note near the map and of the notes those link to, and open whole
/// notes by asking.
pub fn knowledge(
    library: &Library,
    session: &Session,
    holds: &BTreeSet<u64>,
    tier: Tier,
    instruction: &str,
) -> String {
    let mut out = String::new();
    if library.is_empty() {
        return out;
    }
    // What the map is about, in parts: the more a part says of the subject, or of what the
    // turn is there to do, the more its words count.
    let mut about: Vec<(f32, &str)> = vec![];
    if session.title != Session::default().title {
        about.push((3.0, &session.title));
    }
    if let Some(charter) = &session.charter {
        about.push((3.0, &charter.focus));
    }
    about.extend(session.categories.iter().map(|c| (2.0, c.name.as_str())));
    let mut filed: Vec<&Thought> = session
        .thoughts
        .iter()
        .filter(|t| t.category.is_some())
        .collect();
    filed.sort_unstable_by_key(|t| t.id);
    let unfiled = waiting(session, holds);
    let unfiled = |limit: usize| {
        let new = move |t: &&Thought| unfiled.iter().take(limit).any(|id| *id == t.id);
        session.thoughts.iter().filter(new)
    };
    let place = tier == Tier::Place;
    if place {
        about.extend(unfiled(BATCH).map(|t| (3.0, t.text.as_str())));
        let recent = &filed[filed.len().saturating_sub(4)..];
        about.extend(recent.iter().map(|t| (1.0, t.text.as_str())));
    } else {
        about.push((3.0, instruction));
        about.extend(unfiled(usize::MAX).map(|t| (2.0, t.text.as_str())));
        about.extend(filed.iter().map(|t| (1.0, t.map_label())));
        let recent = &filed[filed.len().saturating_sub(12)..];
        about.extend(recent.iter().map(|t| (1.0, t.text.as_str())));
    }
    // Ideas only: a note on a source or an overview of a topic is reached through them.
    let limit = if place { NEAR_PLACE } else { NEAR };
    let idea = |note: &&Note| note.kind == Kind::Idea;
    let near = library.nearest(&about, limit + SPARE);
    let near: Vec<&Note> = near.into_iter().filter(idea).take(limit).collect();
    if place && near.is_empty() {
        return out;
    }
    out.push_str("\nKNOWLEDGE\n");
    let cards = |out: &mut String, notes: &[&Note]| {
        for note in notes {
            let _ = writeln!(out, "{}", library.card(note));
        }
    };
    if place {
        cards(&mut out, &near);
        return out;
    }
    out.push_str(&library.index());
    if near.is_empty() {
        out.push_str("No note is near this map so far.\n");
        return out;
    }
    // How the topics most of the nearest notes are in hang together, where that is written.
    let mut topics: Vec<(&str, usize)> = vec![];
    for note in &near {
        match topics.iter_mut().find(|(topic, _)| *topic == note.topic) {
            Some((_, notes)) => *notes += 1,
            None => topics.push((&note.topic, 1)),
        }
    }
    topics.sort_by_key(|(_, notes)| std::cmp::Reverse(*notes));
    let outlines = topics
        .iter()
        .filter_map(|(topic, _)| library.outline(topic));
    for outline in outlines.take(OVERVIEWS) {
        out.push_str(&outline);
    }
    out.push_str("Nearest this map:\n");
    cards(&mut out, &near);
    // One link on from the nearest: what a note builds on is often what explains it.
    let mut linked: Vec<&Note> = vec![];
    for note in near.iter().take(3) {
        let to = note.links.iter().filter_map(|id| library.get(id));
        for other in to.chain(library.linked_from(&note.id)) {
            let shown = |n: &&Note| n.id == other.id;
            let new = !near.iter().any(shown) && !linked.iter().any(shown);
            if linked.len() < LINKED && new && other.kind == Kind::Idea {
                linked.push(other);
            }
        }
    }
    if !linked.is_empty() {
        out.push_str("Linked to those:\n");
        cards(&mut out, &linked);
    }
    out
}

/// What a listener needs to follow the next recording: the subject, the branch names and the
/// last things said. A few hundred tokens, however long the session has run.
pub fn listen(session: &Session) -> String {
    let mut out = String::new();
    subject(session, &mut out, "");
    let main = |id: &str| {
        session
            .category_path(Some(id))
            .first()
            .map(|c| c.id.as_str())
    };
    let roots = session.categories.iter().filter(|c| c.parent.is_none());
    for (index, root) in roots.enumerate() {
        if index == 0 {
            out.push_str("\nMAP\n");
        }
        let parts: Vec<&str> = session
            .categories
            .iter()
            .filter(|c| c.parent.is_some() && main(&c.id) == Some(&root.id))
            .map(|c| c.name.as_str())
            .collect();
        let _ = match parts.is_empty() {
            true => writeln!(out, "- {}", root.name),
            false => writeln!(out, "- {}: {}", root.name, parts.join(", ")),
        };
    }
    let mut said: Vec<&Thought> = session
        .thoughts
        .iter()
        .chain(session.parked.iter().map(|p| &p.thought))
        .collect();
    said.sort_unstable_by_key(|t| t.id);
    if !said.is_empty() {
        out.push_str("\nJUST SAID\n");
        for t in &said[said.len().saturating_sub(4)..] {
            let _ = writeln!(out, "- {:?}", clip(&t.text, 240));
        }
    }
    out
}
