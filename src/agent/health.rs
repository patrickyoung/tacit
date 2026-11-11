//! The map's condition, judged by rule: a seasoned mapper's checklist. The model is told
//! what strains before it edits and again after, and the scheduler learns when the steward
//! is needed without waiting for the talk to pause.
use super::step::MAX_ROOTS;
use crate::model::{Category, ParkKind, Session};
use std::collections::BTreeMap;

/// Items a branch carries directly before it stops reading at a glance.
pub const SPAN: usize = 7;
/// Thoughts on the map before balance between main branches is worth judging.
pub const SETTLED: usize = 8;
/// Filed thoughts from which a single main branch is a list rather than a map.
pub const LONE: usize = 5;

pub struct Finding {
    /// Structural strain: worth a steward turn on its own.
    pub strain: bool,
    /// The rule and what it faults: the same from one look to the next, whatever the counts
    /// in the note have come to.
    pub key: String,
    /// The branch it faults, when it faults one and not the map as a whole.
    pub branch: Option<String>,
    /// What is so.
    pub fact: String,
    /// What a mapper would do about it.
    pub advice: &'static str,
}
impl Finding {
    pub fn note(&self) -> String {
        format!("{} {}", self.fact, self.advice).trim_end().into()
    }
}
/// Numbers for evaluation and status text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shape {
    pub filed: usize,
    pub unfiled: usize,
    pub parked: usize,
    pub roots: usize,
    pub branches: usize,
    /// Thoughts under the heaviest main branch.
    pub heaviest: usize,
    /// Branches carrying more than [`SPAN`] items directly.
    pub crowded: usize,
    /// Sub-branches that hold a single thought and nothing else.
    pub thin: usize,
}

/// Thoughts filed under a branch or anywhere beneath it.
pub fn weight(session: &Session, branch: &str) -> usize {
    session
        .thoughts
        .iter()
        .filter(|t| session.within_category(t, branch))
        .count()
}
/// Thoughts and sub-branches directly under a branch.
pub fn items(session: &Session, branch: &str) -> (usize, usize) {
    (
        session
            .thoughts
            .iter()
            .filter(|t| t.category.as_deref() == Some(branch))
            .count(),
        session
            .categories
            .iter()
            .filter(|c| c.parent.as_deref() == Some(branch))
            .count(),
    )
}
const GROUP: &str = "Group them into two or three sub-branches named for what they share.";
/// What is said of a branch carrying more than [`SPAN`] items directly.
fn packed(session: &Session, c: &Category) -> Option<String> {
    let (thoughts, branches) = items(session, &c.id);
    let held = thoughts + branches;
    (held > SPAN).then(|| {
        format!(
            "Branch {} ({}) has {held} items directly under it.",
            c.id, c.name
        )
    })
}
/// The branches too crowded to read at a glance, as the model is told of them.
pub fn crowded(session: &Session) -> Vec<String> {
    let branches = session.categories.iter();
    let packed = branches.filter_map(|c| packed(session, c));
    packed.map(|fact| format!("{fact} {GROUP}")).collect()
}
fn thin(session: &Session, branch: &str) -> bool {
    items(session, branch) == (1, 0)
}
pub fn shape(session: &Session) -> Shape {
    let roots = session.categories.iter().filter(|c| c.parent.is_none());
    let filed = session
        .thoughts
        .iter()
        .filter(|t| t.category.is_some())
        .count();
    Shape {
        filed,
        unfiled: session.thoughts.len() - filed,
        parked: session.parked.len(),
        roots: roots.clone().count(),
        branches: session.categories.len(),
        heaviest: roots.map(|c| weight(session, &c.id)).max().unwrap_or(0),
        crowded: session
            .categories
            .iter()
            .filter(|c| {
                let (thoughts, branches) = items(session, &c.id);
                thoughts + branches > SPAN
            })
            .count(),
        thin: session
            .categories
            .iter()
            .filter(|c| c.parent.is_some() && thin(session, &c.id))
            .count(),
    }
}

pub fn review(session: &Session) -> Vec<Finding> {
    let mut out = vec![];
    type Faulted<'a> = (&'a str, Option<&'a str>);
    let mut add = |strain: bool, (rule, branch): Faulted, fact: String, advice| {
        let key = branch.map_or(rule.into(), |id| format!("{rule} {id}"));
        let branch = branch.map(Into::into);
        out.push(Finding {
            strain,
            key,
            branch,
            fact,
            advice,
        })
    };
    let shape = shape(session);
    let filed = shape.filed;
    // Only the user takes a map past seven, so the eighth is theirs and no fault to repair.
    if shape.roots > MAX_ROOTS {
        add(
            false,
            ("wide", None),
            format!(
                "{} main branches, where a map reads best with three to seven.",
                shape.roots
            ),
            "The eighth stands on the user's word: merge two only if they cover the same part of the subject.",
        );
    }
    // One branch holding everything is the subject again, not a part of it.
    if (filed >= LONE && shape.roots == 1) || (filed >= SETTLED && shape.roots < 3) {
        add(
            true,
            ("few", None),
            format!(
                "{filed} thoughts hang from {} main branch(es).",
                shape.roots
            ),
            "Find the one ordering idea that divides the subject into three to seven.",
        );
    }
    for c in &session.categories {
        let (thoughts, branches) = items(session, &c.id);
        let held = weight(session, &c.id);
        let name = format!("{} ({})", c.id, c.name);
        let at = |rule| (rule, Some(c.id.as_str()));
        // Over 55% of the map under one main branch reads as a list, not a map.
        if c.parent.is_none() && shape.roots >= 3 && filed >= SETTLED && held * 20 > filed * 11 {
            add(
                true,
                at("heavy"),
                format!("Main branch {name} holds {held} of {filed} thoughts."),
                "Split it along a real distinction or promote its sub-branches.",
            );
        }
        if let Some(fact) = packed(session, c) {
            add(true, at("packed"), fact, GROUP);
        } else if thoughts + branches == 0 {
            let fact = format!("Branch {name} is empty.");
            add(
                false,
                at("empty"),
                fact,
                "Remove it unless the user asked for it.",
            );
        } else if (thoughts, branches) == (0, 1) {
            let fact = format!("Branch {name} only wraps one sub-branch.");
            add(false, at("wrapper"), fact, "Merge the two.");
        }
        let lone = session
            .categories
            .iter()
            .filter(|kid| kid.parent.as_deref() == Some(&c.id) && thin(session, &kid.id))
            .count();
        if lone >= 2 {
            add(
                lone >= 3,
                at("thin"),
                format!("Branch {name} has {lone} sub-branches holding one thought each."),
                "Put those thoughts directly under it, or group them by what they share.",
            );
        }
        if c.name.split_whitespace().count() > 5 {
            add(
                false,
                at("phrase"),
                format!("Branch {name} is named with a phrase."),
                "Use a keyword of one to three words.",
            );
        }
    }
    // The same words twice are one idea heard twice, or two ideas not yet told apart.
    let mut labels: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for t in session.thoughts.iter().filter(|t| t.category.is_some()) {
        let twins = labels.entry(t.map_label().to_lowercase()).or_default();
        twins.push(t.id);
    }
    for mut twins in labels.into_values().filter(|twins| twins.len() > 1) {
        twins.sort_unstable();
        let label = session.thoughts.iter().find(|t| t.id == twins[0]);
        let ids: Vec<String> = twins.iter().map(|id| format!("#{id}")).collect();
        add(
            false,
            (&format!("twins {}", ids.join(" ")), None),
            format!(
                "{} carry the same label, {:?}.",
                ids.join(" and "),
                label.map_or("", |t| t.map_label())
            ),
            "Merge them if they state the same idea; otherwise label each by what sets it apart.",
        );
    }
    let later = session
        .parked
        .iter()
        .filter(|p| p.kind == ParkKind::Later && !p.held)
        .count();
    if later > 0 {
        add(
            false,
            ("later", None),
            format!("{later} parked thought(s) are marked later."),
            "Bring back any the map now has a home for.",
        );
    }
    let charter = session.charter.clone().unwrap_or_default();
    if filed >= 3 && (charter.focus.trim().is_empty() || charter.principle.trim().is_empty()) {
        let fact = "The map has no stated focus or ordering principle yet.";
        add(false, ("aimless", None), fact.into(), "");
    }
    out
}
pub fn notes(session: &Session) -> Vec<String> {
    review(session).iter().map(Finding::note).collect()
}
pub fn strained(session: &Session) -> bool {
    review(session).iter().any(|f| f.strain)
}
