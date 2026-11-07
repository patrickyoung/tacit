use tacit::{
    direction::{self, Classification, Edit, Placement},
    model::{Assignment, Category, Interpretation, Relationship, Session},
    session_event::SessionEvent,
    timeline::{Event, Journal},
};

fn category(id: &str, parent: Option<&str>) -> Category {
    Category {
        id: id.into(),
        name: id.into(),
        parent: parent.map(str::to_owned),
        motif: None,
        illustration: None,
    }
}

fn broad_map(count: usize) -> Session {
    let mut session = Session {
        categories: vec![category("operations", None)],
        ..Session::default()
    };
    for i in 0..count {
        session.add(&format!("Original contribution {i}"));
        session.thoughts[i].category = Some("operations".into());
        session.thoughts[i].concept = Some(format!("Contribution {i}"));
    }
    session
}

#[test]
fn broad_refinement_preserves_every_contribution_without_pinning_it() {
    let mut session = broad_map(60);
    for thought in &mut session.thoughts {
        let source = format!("thought-{}", thought.id);
        session.sources.insert(source.clone(), thought.text.clone());
        thought.evidence = vec![source];
    }
    session.interpretation = Some(Interpretation {
        title: session.title.clone(),
        narrative: "The discussion covers several business issues [1].".into(),
        categories: session.categories.clone(),
        assignments: session
            .thoughts
            .iter()
            .map(|thought| Assignment {
                id: thought.id,
                concept: thought.concept.clone(),
                role: thought.role,
                category: thought.category.clone(),
            })
            .collect(),
        relationships: vec![Relationship {
            from: 1,
            to: 2,
            label: "related".into(),
        }],
    });
    let names = ["hiring", "cloud-costs", "release-ownership"];
    let classification = Classification {
        categories: names.iter().map(|name| category(name, None)).collect(),
        placements: session
            .thoughts
            .iter()
            .enumerate()
            .map(|(i, thought)| Placement {
                id: thought.id,
                category: names[i % names.len()].into(),
            })
            .collect(),
    };
    let instruction = "Can you split Operations into more specific categories?";
    let next = direction::apply(
        &session,
        instruction,
        &[Edit::RefineCategories {
            classification: classification.clone(),
        }],
    )
    .unwrap();
    let mut expected = session.thoughts.clone();
    for (thought, placement) in expected.iter_mut().zip(&classification.placements) {
        thought.category = Some(placement.category.clone());
    }
    assert_eq!(next.thoughts, expected);
    assert_eq!(next.directions, [instruction]);
    assert_eq!(next.next_id, session.next_id);
    assert_eq!(next.sources, session.sources);
    assert_eq!(next.retired, session.retired);
    let interpretation = next.interpretation.as_ref().unwrap();
    assert_eq!(interpretation.categories, next.categories);
    assert_eq!(interpretation.narrative, session.narrative());
    assert_eq!(
        interpretation.relationships,
        session.interpretation.as_ref().unwrap().relationships
    );
    for (assignment, thought) in interpretation.assignments.iter().zip(&next.thoughts) {
        assert_eq!(assignment.category, thought.category);
    }

    // A later refinement can still move these placements and improve their paths.
    let mut deeper = classification;
    deeper.categories[1].parent = Some("release-ownership".into());
    deeper.placements[0].category = "cloud-costs".into();
    direction::apply(
        &next,
        "Refine the categories again",
        &[Edit::RefineCategories {
            classification: deeper,
        }],
    )
    .unwrap();
}

#[test]
fn refinement_rejects_incomplete_assignments_invalid_trees_and_changed_pins() {
    let mut session = broad_map(3);
    session.thoughts[0].pinned = true;
    let valid = Classification {
        categories: vec![category("operations", None), category("hiring", None)],
        placements: vec![
            Placement {
                id: 1,
                category: "operations".into(),
            },
            Placement {
                id: 2,
                category: "hiring".into(),
            },
            Placement {
                id: 3,
                category: "hiring".into(),
            },
        ],
    };
    let next = direction::apply(
        &session,
        "Refine",
        &[Edit::RefineCategories {
            classification: valid.clone(),
        }],
    )
    .unwrap();
    assert_eq!(next.thoughts[0], session.thoughts[0]);
    assert!(!next.thoughts[1].pinned);
    let mut invalid = Vec::new();
    let mut bad = valid.clone();
    bad.placements.pop();
    invalid.push(bad);
    let mut bad = valid.clone();
    bad.placements[2].id = 2;
    invalid.push(bad);
    let mut bad = valid.clone();
    bad.placements[2].id = 99;
    invalid.push(bad);
    let mut bad = valid.clone();
    bad.placements[2].category = "missing".into();
    invalid.push(bad);
    let mut bad = valid.clone();
    bad.placements[0].category = "hiring".into();
    invalid.push(bad);
    let mut bad = valid.clone();
    bad.categories[0].name = "Changed pinned ancestor".into();
    invalid.push(bad);
    let mut bad = valid.clone();
    bad.categories[0].parent = Some("hiring".into());
    invalid.push(bad);
    let mut bad = valid;
    bad.categories[1].parent = Some("operations".into());
    bad.categories[0].parent = Some("hiring".into());
    invalid.push(bad);
    for classification in invalid {
        assert!(
            direction::apply(
                &session,
                "Refine",
                &[Edit::RefineCategories { classification }]
            )
            .is_err()
        );
    }
}

#[test]
fn category_refinement_and_future_guidance_replay_and_restore_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let mut journal = Journal::open(&path, Session::default()).unwrap();
    journal
        .append(
            10,
            Event::SessionChanged {
                change: SessionEvent::Directed {
                    instruction: "Keep hiring and cloud costs in separate categories".into(),
                    edits: vec![Edit::RememberGuidance],
                },
            },
        )
        .unwrap();
    let preference = journal.projection.session.clone();
    assert_eq!(preference.directions.len(), 1);
    assert!(preference.thoughts.is_empty());
    journal
        .append(
            20,
            Event::SessionChanged {
                change: SessionEvent::capture(&preference, "The cloud bill doubled."),
            },
        )
        .unwrap();
    let before = journal.projection.session.clone();
    journal
        .append(
            30,
            Event::SessionChanged {
                change: SessionEvent::Directed {
                    instruction: "Put this into a specific category".into(),
                    edits: vec![Edit::RefineCategories {
                        classification: Classification {
                            categories: vec![category("cloud-costs", None)],
                            placements: vec![Placement {
                                id: 1,
                                category: "cloud-costs".into(),
                            }],
                        },
                    }],
                },
            },
        )
        .unwrap();
    let refined = journal.projection.session.clone();
    assert_eq!(refined.directions.len(), 2);
    journal
        .append(
            40,
            Event::SessionChanged {
                change: SessionEvent::Restored {
                    session: before.clone(),
                    reason: "undo".into(),
                },
            },
        )
        .unwrap();
    drop(journal);
    let reopened = Journal::read(&path).unwrap();
    assert_eq!(reopened.at(10).unwrap().session, preference);
    assert_eq!(reopened.at(30).unwrap().session, refined);
    assert_eq!(reopened.at(40).unwrap().session, before);
}
