use tacit::{
    model::{Role, Session},
    session_event::SessionEvent,
    timeline::{Event, Journal, Projection, VERSION},
};

#[test]
fn live_content_equals_replay_without_any_presentation_track() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let mut journal = Journal::open(&path, Session::default()).unwrap();
    journal
        .append(
            10,
            Event::SessionChanged {
                change: SessionEvent::capture(
                    &journal.projection.session,
                    "First thought. Another idea.",
                ),
            },
        )
        .unwrap();
    let first = journal.projection.clone();
    journal
        .append(
            20,
            Event::SessionChanged {
                change: SessionEvent::Split {
                    source_id: 1,
                    original: "First thought. Another idea.".into(),
                    chunks: vec!["First thought. ".into(), "Another idea.".into()],
                },
            },
        )
        .unwrap();
    assert_eq!(journal.projection.splits[0].thought_ids, vec![1, 2]);
    journal
        .append(
            30,
            Event::SessionChanged {
                change: SessionEvent::PlacementCorrected {
                    id: 2,
                    role: Role::Question,
                    category: None,
                },
            },
        )
        .unwrap();
    let corrected = journal.projection.clone();
    journal
        .append(
            40,
            Event::SessionChanged {
                change: SessionEvent::Removed { id: 1 },
            },
        )
        .unwrap();
    journal
        .append(
            50,
            Event::SessionChanged {
                change: SessionEvent::Restored {
                    session: corrected.session.clone(),
                    reason: "undo".into(),
                },
            },
        )
        .unwrap();
    // A renderer's opaque state cannot change the shared content, even when unknown to Rune.
    let content = serde_json::to_value(journal.projection.content()).unwrap();
    journal
        .append(
            60,
            Event::Presentation {
                renderer: "future-3d".into(),
                version: 99,
                state: serde_json::json!({"camera": [1,2,3]}),
            },
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(journal.projection.content()).unwrap(),
        content
    );
    assert!(content.get("presentations").is_none());
    assert!(
        !journal
            .projection
            .presentations
            .contains_key(tacit::timeline::RENDERER)
    );
    assert_eq!(journal.at(15).unwrap(), first);
    assert_eq!(journal.at(30).unwrap(), corrected);
    let expected = journal.projection.clone();
    drop(journal);
    let loaded = Journal::read(&path).unwrap();
    assert_eq!(loaded.projection, expected);
    assert_eq!(loaded.at(60).unwrap(), expected);
    assert!(
        !loaded
            .entries
            .iter()
            .any(|e| matches!(e.event, Event::Scene { .. }))
    );
}

#[test]
fn invalid_event_is_atomic_and_pins_survive_organization() {
    let mut p = Projection {
        session: Session::demo(),
        ..Default::default()
    };
    p.apply(&Event::SessionChanged {
        change: SessionEvent::PlacementCorrected {
            id: 1,
            role: Role::Tension,
            category: None,
        },
    })
    .unwrap();
    let before = p.clone();
    assert!(
        p.apply(&Event::SessionChanged {
            change: SessionEvent::Split {
                source_id: 1,
                original: p.session.thoughts[0].text.clone(),
                chunks: vec!["rewritten".into()]
            }
        })
        .is_err()
    );
    assert_eq!(p, before);
    let interpretation = tacit::model::Interpretation {
        categories: vec![],
        title: "A new reading".into(),
        narrative: "Original words, revised organization".into(),
        assignments: p
            .session
            .thoughts
            .iter()
            .map(|t| tacit::model::Assignment {
                concept: t.concept.clone(),
                id: t.id,
                role: Role::Observation,
                category: None,
            })
            .collect(),
        relationships: vec![],
    };
    p.apply(&Event::SessionChanged {
        change: SessionEvent::Organized {
            interpretation,
            pass: "review".into(),
        },
    })
    .unwrap();
    assert_eq!(p.session.thoughts[0].role, Role::Tension);
    assert!(p.session.thoughts[0].pinned);
}

#[test]
fn timeline_version_is_independent_of_renderer_version() {
    let mut p = Projection::default();
    p.apply(&Event::Opened {
        version: VERSION,
        renderer: "a-renderer-we-have-never-seen".into(),
        origin_unix_ms: 0,
    })
    .unwrap();
    assert!(
        p.apply(&Event::Opened {
            version: VERSION + 1,
            renderer: "anything".into(),
            origin_unix_ms: 0
        })
        .is_err()
    );
    let legacy = tacit::presentation::Scene {
        session: Session::demo(),
        draft: "Unfinished".into(),
        ..Default::default()
    };
    p.apply(&Event::Opened {
        version: 1,
        renderer: tacit::timeline::RENDERER.into(),
        origin_unix_ms: 0,
    })
    .unwrap();
    p.apply(&Event::Scene {
        reason: "legacy".into(),
        scene: Box::new(legacy.clone()),
    })
    .unwrap();
    assert_eq!(p.session, legacy.session);
    assert_eq!(p.scene(), legacy);
}

#[test]
fn checkpoint_seeks_match_full_reduction_including_same_timestamp_events() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let mut journal = Journal::open(&path, Session::default()).unwrap();
    for n in 0..150 {
        journal
            .append(
                n / 3 + 1,
                Event::SessionChanged {
                    change: SessionEvent::capture(
                        &journal.projection.session,
                        &format!("Thought {n}"),
                    ),
                },
            )
            .unwrap();
    }
    drop(journal);
    let journal = Journal::read(&path).unwrap();
    for at in [0, 1, 20, 21, 22, 42, 43, 50, 999] {
        let mut expected = Projection::default();
        for entry in journal.entries.iter().filter(|e| e.at_ms <= at) {
            expected.apply(&entry.event).unwrap();
        }
        assert_eq!(journal.at(at).unwrap(), expected);
    }
}

#[test]
fn undo_does_not_reuse_historical_thought_ids() {
    let mut p = Projection::default();
    let initial = p.session.clone();
    p.apply(&Event::SessionChanged {
        change: SessionEvent::capture(&p.session, "Original"),
    })
    .unwrap();
    p.apply(&Event::SessionChanged {
        change: SessionEvent::Restored {
            session: initial,
            reason: "undo".into(),
        },
    })
    .unwrap();
    p.apply(&Event::SessionChanged {
        change: SessionEvent::capture(&p.session, "Different thought"),
    })
    .unwrap();
    assert_eq!(p.session.thoughts[0].id, 2);
}

#[test]
fn rune_can_adapt_content_authored_without_a_rune_presentation() {
    let mut session = Session::default();
    session.categories.push(tacit::model::Category {
        parent: None,
        id: "learning".into(),
        name: "Learning".into(),
        motif: None,
        illustration: None,
    });
    let mut p = Projection {
        session,
        ..Default::default()
    };
    p.apply(&Event::ArtworkAvailable {
        artwork: tacit::timeline::SessionArtwork {
            scope: tacit::timeline::ArtworkScope::Theme,
            theme: "learning".into(),
            fingerprint: "first".into(),
            asset: "first.png".into(),
        },
    })
    .unwrap();
    p.apply(&Event::ArtworkAvailable {
        artwork: tacit::timeline::SessionArtwork {
            scope: tacit::timeline::ArtworkScope::Theme,
            theme: "learning".into(),
            fingerprint: "second".into(),
            asset: "second.png".into(),
        },
    })
    .unwrap();
    let scene = p.scene();
    assert_eq!(scene.session, p.session);
    assert_eq!(scene.artwork.len(), 1);
    assert_eq!(scene.artwork[0].asset, "second.png");
    assert_eq!(scene.artwork[0].key, "theme-learning");
    assert_eq!(p.artwork.len(), 2); // History remains available to other renderers.
}

#[test]
fn transactions_are_atomic_and_presentation_pins_the_displayed_revision_and_clock() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&dir.path().join("session.json"), Session::default()).unwrap();
    let mut scene = j.projection.scene();
    scene.render_time = Some(0.25);
    j.present_scene(250, scene.clone()).unwrap();
    let before = j.projection.clone();
    let count = j.entries.len();
    let capture = Event::SessionChanged {
        change: SessionEvent::capture(&j.projection.session, "New thought"),
    };
    assert!(
        j.append(
            300,
            Event::Transaction {
                events: vec![
                    capture.clone(),
                    Event::Transcript {
                        recording: "missing".into(),
                        text: "no audio".into(),
                        cues: vec![]
                    }
                ]
            }
        )
        .is_err()
    );
    assert_eq!(j.projection, before);
    assert_eq!(j.entries.len(), count);
    j.append(300, capture).unwrap();
    assert_eq!(j.at(350).unwrap().session.thoughts.len(), 1);
    let original = j.replay_scene_at(350).unwrap();
    assert!(original.session.thoughts.is_empty()); // Not displayed until the next presentation.
    assert_eq!(original.render_time, Some(0.25));
    scene.session = j.projection.session.clone();
    scene.render_time = Some(0.4);
    j.present_scene(400, scene).unwrap();
    assert_eq!(j.replay_scene_at(450).unwrap().session.thoughts.len(), 1);
    assert_eq!(j.replay_scene_at(450).unwrap().render_time, Some(0.4));
    j.validate_replay().unwrap();
}

#[test]
fn strict_replay_rejects_missing_or_incompatible_presentation_instead_of_inventing_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&dir.path().join("session.json"), Session::demo()).unwrap();
    assert!(j.validate_replay().is_err());
    assert!(j.replay_scene_at(0).is_err());
    j.append(
        10,
        Event::Presentation {
            renderer: tacit::timeline::RENDERER.into(),
            version: 999,
            state: serde_json::json!({}),
        },
    )
    .unwrap();
    assert!(j.validate_replay().is_err());
}

#[test]
fn concepts_replay_with_original_words_and_split_invalidates_old_label() {
    use tacit::model::{Assignment, Interpretation};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let mut j = Journal::open(&path, Session::default()).unwrap();
    let original = "Meetings take all day. Hosting costs doubled.";
    j.append(
        1,
        Event::SessionChanged {
            change: SessionEvent::capture(&j.projection.session, original),
        },
    )
    .unwrap();
    j.append(
        2,
        Event::SessionChanged {
            change: SessionEvent::Organized {
                interpretation: Interpretation {
                    title: "Operating pressures".into(),
                    narrative: "Time and cost pressures [1].".into(),
                    categories: vec![],
                    relationships: vec![],
                    assignments: vec![Assignment {
                        id: 1,
                        role: Role::Observation,
                        category: None,
                        concept: Some("Time and cost pressures".into()),
                    }],
                },
                pass: "review".into(),
            },
        },
    )
    .unwrap();
    j.append(
        3,
        Event::SessionChanged {
            change: SessionEvent::Split {
                source_id: 1,
                original: original.into(),
                chunks: vec![
                    "Meetings take all day. ".into(),
                    "Hosting costs doubled.".into(),
                ],
            },
        },
    )
    .unwrap();
    drop(j);
    let j = Journal::read(&path).unwrap();
    let before = j.at(2).unwrap();
    assert_eq!(
        before.session.thoughts[0].map_label(),
        "Time and cost pressures"
    );
    assert_eq!(before.session.thoughts[0].text, original);
    let after = j.at(3).unwrap();
    assert_eq!(after.session.thoughts.len(), 2);
    assert!(after.session.thoughts.iter().all(|t| t.concept.is_none()));
}

#[test]
fn whole_map_artwork_has_portable_scope_and_cannot_replace_a_same_named_theme() {
    use tacit::timeline::{ArtworkScope, SessionArtwork};
    let mut p = Projection::default();
    p.session.categories.push(tacit::model::Category {
        id: "map-synthesis".into(),
        name: "Map synthesis".into(),
        parent: None,
        motif: None,
        illustration: None,
    });
    for (scope, asset) in [
        (ArtworkScope::Theme, "theme.png"),
        (ArtworkScope::Map, "central.png"),
    ] {
        p.apply(&Event::ArtworkAvailable {
            artwork: SessionArtwork {
                scope,
                theme: "map-synthesis".into(),
                fingerprint: asset.into(),
                asset: asset.into(),
            },
        })
        .unwrap();
    }
    let scene = p.scene();
    assert_eq!(
        scene
            .artwork
            .iter()
            .find(|a| a.key == "theme-map-synthesis")
            .unwrap()
            .asset,
        "theme.png"
    );
    assert_eq!(
        scene
            .artwork
            .iter()
            .find(|a| a.key == "map-synthesis")
            .unwrap()
            .asset,
        "central.png"
    );
    let legacy: SessionArtwork =
        serde_json::from_str(r#"{"theme":"time","fingerprint":"old","asset":"old.png"}"#).unwrap();
    assert_eq!(legacy.scope, ArtworkScope::Theme);
    assert!(serde_json::to_value(legacy).unwrap().get("scope").is_none());
}
