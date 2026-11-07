//! Opt-in live checks for spoken guidance and editorial grouping. Synthetic content only.
use tacit::{
    direction, evolution,
    model::{Category, Session},
    organizer::CloudConfig,
    timeline::Projection,
};

fn broad_map() -> Session {
    let mut session = Session {
        categories: vec![Category {
            id: "operations".into(),
            name: "Operations".into(),
            parent: None,
            motif: None,
            illustration: None,
        }],
        ..Session::default()
    };
    for text in [
        "Idle database replicas doubled our cloud bill.",
        "New hires wait two weeks to get a mentor.",
        "Engineering and product disagree about who approves a release.",
        "Sales hands new customer accounts to support without implementation notes.",
        "We could automatically shut down idle database replicas overnight.",
        "We could assign each new hire a mentor before their first day.",
        "We could name one accountable release approver for each product.",
        "We could require a customer handoff checklist with implementation notes.",
        "Database failover capacity must remain available during overnight shutdowns.",
        "Mentors can support no more than two new hires at once.",
        "Security must retain a separate veto over risky releases.",
        "Support should confirm receipt before sales closes a customer handoff.",
    ] {
        session.add(text);
        session.thoughts.last_mut().unwrap().category = Some("operations".into());
    }
    session
}

fn assert_specific_threads(session: &Session) -> Result<(), String> {
    for (name, members) in [
        ("database capacity costs", [1, 5, 9]),
        ("new-hire mentoring", [2, 6, 10]),
        ("release approval", [3, 7, 11]),
        ("customer handoff", [4, 8, 12]),
    ] {
        // Permit sensible nesting and naming variation, but require each thread
        // to have its own category instead of sharing a catch-all business bucket.
        if !session.categories.iter().any(|category| {
            session.thoughts.iter().all(|thought| {
                session.within_category(thought, &category.id) == members.contains(&thought.id)
            })
        }) {
            return Err(format!("No distinct, coherent category for {name}"));
        }
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let config = CloudConfig::from_env()?.ok_or("OpenRouter is not configured")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let session = broad_map();
        let instruction = "Could you organize this more clearly? Everything is getting lumped into Operations. Please refine the categories so the different issues are easy to understand, while keeping the individual ideas.";
        let plan = direction::plan(&config, &session, instruction).await?;
        if !plan.edits.iter().any(|edit| matches!(edit, direction::Edit::RefineCategories { .. })) {
            return Err("Polite broad guidance did not produce category refinement".into());
        }
        let refined = direction::apply(&session, instruction, &plan.edits)?;
        assert_specific_threads(&refined)?;
        let mut original_placements = refined.thoughts.clone();
        for thought in &mut original_placements {
            thought.category = Some("operations".into());
        }
        if original_placements != session.thoughts {
            return Err("Guided refinement changed content, ordering, or pins".into());
        }
        println!("PASS voice guidance: polite instruction separates four business issues and preserves all 12 contributions without pinning");

        let sources = Projection { session: session.clone(), ..Projection::default() }.evidence();
        let plan = evolution::review(&config, &session, &sources).await?;
        let reviewed = plan.apply(&session, &sources)?;
        if !reviewed.retired.is_empty() || reviewed.thoughts.len() != session.thoughts.len() {
            return Err("Editorial review collapsed or retired distinct contributions".into());
        }
        for original in &session.thoughts {
            let thought = reviewed.thoughts.iter().find(|thought| thought.id == original.id)
                .ok_or("Editorial review replaced a continuing concept ID")?;
            if thought.text != original.text || thought.evidence != [format!("thought-{}", original.id)] {
                return Err("Editorial review combined distinct contributions instead of classifying them".into());
            }
        }
        assert_specific_threads(&reviewed)?;
        println!("PASS automatic review: refines the broad category while retaining separate problems, proposals, and constraints");

        let instruction = "For future ideas, keep customer onboarding separate from employee onboarding.";
        let empty = Session::default();
        let plan = direction::plan(&config, &empty, instruction).await?;
        let remembered = direction::apply(&empty, instruction, &plan.edits)?;
        if remembered.directions != [instruction] || !remembered.thoughts.is_empty() {
            return Err("Future organizational guidance was not retained on an empty map".into());
        }
        println!("PASS future guidance: retained before there are thoughts to organize");
        Ok(())
    })
}
