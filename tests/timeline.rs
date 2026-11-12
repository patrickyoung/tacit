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
                    session: Box::new(corrected.session.clone()),
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
            session: Box::new(initial),
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
            mural: None,
            scope: tacit::timeline::ArtworkScope::Theme,
            theme: "learning".into(),
            fingerprint: "first".into(),
            asset: "first.png".into(),
        },
    })
    .unwrap();
    p.apply(&Event::ArtworkAvailable {
        artwork: tacit::timeline::SessionArtwork {
            mural: None,
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
fn pixel_transactions_preserve_presentation_format_and_roll_back_invalid_frames() {
    use tacit::timeline::{PixelFrame, RENDERER, content_hash};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let mut journal = Journal::open(&path, Session::demo()).unwrap();
    let mut scene = journal.projection.scene();
    scene.draft = "Not yet captured".into();
    scene.render_time = Some(1.25);
    scene.session_title = "A saved map".into();
    let expected = scene.presentation_value().unwrap();
    let png = tacit::video_archive::png_bytes(1, 1, &[12, 34, 56, 255]).unwrap();
    let frame = PixelFrame {
        asset: journal.import_bytes(&png, "png").unwrap(),
        width: 1,
        height: 1,
        scale: 2.0,
        content_hash: content_hash(&scene.session).unwrap(),
    };
    let before = journal.projection.clone();
    let count = journal.entries.len();
    let log = journal.root.join("events.jsonl");
    let bytes = std::fs::read(&log).unwrap();
    let mut invalid = frame.clone();
    invalid.content_hash = "stale content".into();
    assert!(
        journal
            .present_pixels(1250, scene.clone(), invalid)
            .is_err()
    );
    assert_eq!(journal.projection, before);
    assert_eq!(journal.entries.len(), count);
    assert_eq!(std::fs::read(&log).unwrap(), bytes);

    journal
        .present_pixels(1250, scene.clone(), frame.clone())
        .unwrap();
    assert_eq!(journal.entries.len(), count + 1);
    assert_eq!(journal.projection.presentations[RENDERER].state, expected);
    assert_eq!(journal.projection.pixel_frame, Some(frame));
    assert_eq!(journal.projection.replay_scene().unwrap(), scene);
    let loaded = Journal::read(&path).unwrap();
    assert_eq!(loaded.projection, journal.projection);
    assert_eq!(loaded.replay_scene_at(1250).unwrap(), scene);
    loaded.validate_replay().unwrap();
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
                mural: None,
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

#[test]
fn a_dropped_file_replays_from_the_journal_without_being_read_again() {
    use tacit::{ingest::Kind, timeline::Attachment};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let mut journal = Journal::open(&path, Session::default()).unwrap();
    let before = serde_json::to_value(journal.projection.content()).unwrap();
    assert!(
        before.get("attachments").is_none() && before.get("attachment_links").is_none(),
        "a session with no files reads as it did before files could be dropped"
    );
    let source = dir.path().join("Plan.PDF");
    std::fs::write(&source, b"%PDF-1.4 not really").unwrap();
    let asset = tacit::timeline::store(&journal.root.join("media"), &source, "pdf").unwrap();
    assert_eq!(
        asset,
        format!("{}.pdf", blake3::hash(b"%PDF-1.4 not really").to_hex())
    );
    // The same contents are kept once, whatever the file was called.
    let again = dir.path().join("copy.pdf");
    std::fs::copy(&source, &again).unwrap();
    assert_eq!(
        tacit::timeline::store(&journal.root.join("media"), &again, "pdf").unwrap(),
        asset
    );
    assert_eq!(
        std::fs::read_dir(journal.root.join("media"))
            .unwrap()
            .count(),
        1
    );
    let attachment = Attachment {
        id: "attachment-1".into(),
        name: "Plan.PDF".into(),
        kind: Kind::Pdf,
        asset,
    };
    journal
        .append(
            10,
            Event::AttachmentSaved {
                attachment: attachment.clone(),
            },
        )
        .unwrap();
    // Read, captured and linked together, or not at all.
    assert!(
        journal
            .append(
                15,
                Event::Transaction {
                    events: vec![
                        Event::AttachmentRead {
                            attachment: "attachment-1".into(),
                            text: "We ship in March.".into(),
                        },
                        Event::AttachmentLinked {
                            attachment: "attachment-9".into(),
                            thought_id: 1,
                        },
                    ],
                },
            )
            .is_err()
    );
    assert!(journal.projection.readings.is_empty());
    journal
        .append(
            20,
            Event::Transaction {
                events: vec![
                    Event::AttachmentRead {
                        attachment: "attachment-1".into(),
                        text: "We ship in March.".into(),
                    },
                    Event::SessionChanged {
                        change: SessionEvent::capture(
                            &journal.projection.session,
                            "We ship in March.",
                        ),
                    },
                    Event::AttachmentLinked {
                        attachment: "attachment-1".into(),
                        thought_id: 1,
                    },
                ],
            },
        )
        .unwrap();
    for refused in [
        Event::AttachmentSaved {
            attachment: attachment.clone(),
        },
        Event::AttachmentSaved {
            attachment: Attachment {
                id: "attachment-2".into(),
                asset: "../outside.pdf".into(),
                ..attachment.clone()
            },
        },
        Event::AttachmentRead {
            attachment: "attachment-2".into(),
            text: "Words from nowhere.".into(),
        },
    ] {
        assert!(journal.append(30, refused).is_err());
    }
    let original = journal.original(&attachment).unwrap();
    // An original may be removed to save space: the journal holds what it said.
    std::fs::remove_file(original).unwrap();
    let replayed = Journal::read(&path).unwrap();
    assert_eq!(replayed.projection, journal.projection);
    assert_eq!(replayed.projection.attachments, vec![attachment.clone()]);
    assert_eq!(
        &*replayed.projection.readings["attachment-1"],
        "We ship in March."
    );
    assert_eq!(replayed.projection.attachment_links["attachment-1"], 1);
    assert_eq!(
        replayed.projection.session.thoughts[0].text,
        "We ship in March."
    );
    assert_eq!(replayed.original(&attachment), None);
    let content = serde_json::to_value(replayed.projection.content()).unwrap();
    assert_eq!(content["attachments"][0]["kind"], "pdf");
    assert_eq!(content["attachment_links"]["attachment-1"], 1);
}
