//! Opt-in live check: a talk arriving chunk by chunk, organized only by the quick pass, as
//! during uninterrupted speech. Synthetic content only. Pass a run count (default 1).
use tacit::{model::Session, organizer::CloudConfig};

/// A spoken walkthrough of one framework, in the chunks a recording delivers.
const TALK: &[&[&str]] = &[
    &[
        "Every story ever told follows one of seven basic plot structures.",
        "Christopher Booker described these seven basic plots.",
    ],
    &[
        "Number one, overcoming the monster: the hero defeats a threat to their home.",
        "Number two, rags to riches: someone poor gains wealth, loses it, and grows to deserve it.",
    ],
    &[
        "Number three, the quest: the hero sets out to find a treasure or place against obstacles.",
        "Number four, voyage and return: the hero visits a strange land and comes back changed.",
    ],
    &[
        "Number five, comedy: confusion and misunderstanding resolve into a happy ending.",
        "Number six, tragedy: a flaw or mistake leads the hero to downfall.",
    ],
    &["Number seven, rebirth: a dark force traps the hero until an act of redemption frees them."],
    &[
        "So pretty much every story fits one of these patterns.",
        "Knowing them lets you master storytelling and plan your own arc.",
    ],
    &["What if we used these structures to shape our product launch story?"],
];

fn largest_share(session: &Session) -> (String, usize) {
    session
        .categories
        .iter()
        .filter(|c| c.parent.is_none())
        .map(|root| {
            let n = session
                .thoughts
                .iter()
                .filter(|t| session.within_category(t, &root.id))
                .count();
            (root.name.clone(), n)
        })
        .max_by_key(|(_, n)| *n)
        .unwrap_or_default()
}

fn outline(session: &Session) -> String {
    let mut lines = Vec::new();
    for t in &session.thoughts {
        lines.push(format!(
            "  {:>2} {} :: {}",
            t.id,
            session.placement_path(t),
            t.concept.as_deref().unwrap_or("-")
        ));
    }
    lines.join("\n")
}

fn main() -> Result<(), String> {
    let config = CloudConfig::from_env()?.ok_or("OpenRouter is not configured")?;
    let runs = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(1usize);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let mut failures = 0;
    for run in 1..=runs {
        let session = runtime.block_on(async {
            let mut session = Session::default();
            let mut regroups = 0;
            for chunk in TALK {
                for text in *chunk {
                    session.add(text);
                }
                regroups += usize::from(tacit::organizer::crowded(&session));
                let interpretation = config.organize(&session, false).await?;
                interpretation.apply(&mut session)?;
            }
            Ok::<_, String>((session, regroups))
        })?;
        let (session, regroups) = session;
        let total = session.thoughts.len();
        let (name, largest) = largest_share(&session);
        let roots = session
            .categories
            .iter()
            .filter(|c| c.parent.is_none())
            .count();
        let used = session
            .categories
            .iter()
            .filter(|c| {
                session
                    .thoughts
                    .iter()
                    .any(|t| t.category.as_deref() == Some(&c.id))
            })
            .count();
        // The map draws each root theme as one side's branch, so one root carrying most of the
        // talk reads as a single branch however finely it is subdivided.
        // Regrouping renames branches, so it should settle rather than recur with every chunk.
        let pass = roots >= 2 && largest * 5 <= total * 3 && regroups <= 2;
        failures += usize::from(!pass);
        println!(
            "run {run}: {} · {roots} roots, {used} categories in use, largest root {name:?} holds {largest}/{total}, regrouped {regroups}×\n{}",
            if pass { "PASS" } else { "FAIL" },
            outline(&session)
        );
    }
    if failures > 0 {
        return Err(format!("{failures} of {runs} runs crowded one branch"));
    }
    Ok(())
}
