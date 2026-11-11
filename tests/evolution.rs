use tacit::{
    evolution::{Concept, Plan, Retirement, Sources},
    model::{Category, Role, Session},
    timeline::{Event, Journal, Projection, content_hash},
};

fn fixture() -> (Session, Sources, Plan) {
    let mut s = Session::default();
    s.add("We could share a research budget.");
    s.add("Pooling funds might help research teams.");
    let sources = s
        .thoughts
        .iter()
        .map(|t| (format!("thought-{}", t.id), t.text.clone()))
        .collect();
    let p = Plan {
        explanation: "Combine overlapping proposals.".into(),
        title: "Shared research".into(),
        narrative: "A proposal for pooled resources.".into(),
        categories: vec![Category {
            id: "funding".into(),
            name: "Funding".into(),
            parent: None,
            motif: None,
            illustration: None,
        }],
        concepts: vec![Concept {
            id: 1,
            concept: "Proposed pooled research budget".into(),
            sources: vec!["thought-1".into(), "thought-2".into()],
            category: "funding".into(),
            role: Role::Possibility,
        }],
        retirements: vec![Retirement {
            id: 2,
            reason: "Merged overlapping proposal.".into(),
            replaced_by: vec![1],
        }],
        relationships: vec![],
    };
    (s, sources, p)
}
#[test]
fn merge_preserves_exact_words_and_retirement_links() {
    let (s, sources, p) = fixture();
    let next = p.apply(&s, &sources).unwrap();
    assert_eq!(next.thoughts.len(), 1);
    assert_eq!(
        next.thoughts[0].text,
        format!("{}\n\n{}", s.thoughts[0].text, s.thoughts[1].text)
    );
    assert_eq!(next.retired[0].thought.text, s.thoughts[1].text);
    assert_eq!(next.retired[0].replaced_by, vec![1]);
    assert_eq!(next.sources, sources);
    let restored: Session = serde_json::from_str(&serde_json::to_string(&next).unwrap()).unwrap();
    assert_eq!(restored, next);
}
#[test]
fn splits_and_additions_can_share_evidence_and_use_full_transcript() {
    let (s, mut sources, mut p) = fixture();
    sources.insert(
        "transcript-recording".into(),
        "Perhaps build a shared lab too.".into(),
    );
    p.concepts.push(Concept {
        id: 3,
        concept: "Possible shared lab".into(),
        sources: vec!["transcript-recording".into()],
        category: "funding".into(),
        role: Role::Possibility,
    });
    p.concepts.push(Concept {
        id: 4,
        concept: "Research collaboration".into(),
        sources: vec!["thought-1".into()],
        category: "funding".into(),
        role: Role::Possibility,
    });
    let next = p.apply(&s, &sources).unwrap();
    assert_eq!(next.next_id, 5);
    assert_eq!(next.thoughts[1].text, sources["transcript-recording"]);
}
#[test]
fn unsafe_editorial_plans_are_rejected_atomically() {
    let (s, sources, p) = fixture();
    let mut invalid = vec![];
    let mut bad = p.clone();
    bad.retirements.clear();
    invalid.push(bad);
    let mut bad = p.clone();
    bad.concepts[0].sources = vec!["invented".into()];
    invalid.push(bad);
    let mut bad = p.clone();
    bad.concepts[0].sources = vec!["thought-2".into()];
    invalid.push(bad);
    let mut bad = p.clone();
    bad.retirements[0].replaced_by = vec![99];
    invalid.push(bad);
    let mut bad = p.clone();
    bad.concepts[0].id = 4;
    invalid.push(bad);
    let mut bad = p.clone();
    bad.concepts[0].category = "missing".into();
    invalid.push(bad);
    for bad in invalid {
        assert!(bad.apply(&s, &sources).is_err());
    }
    let mut pinned = s.clone();
    pinned.thoughts[1].pinned = true;
    assert!(p.apply(&pinned, &sources).is_err());
    assert_eq!(s.thoughts.len(), 2);
}
#[test]
fn obsolete_concepts_remain_accessible_without_successors() {
    let (s, sources, mut p) = fixture();
    p.concepts[0].sources = vec!["thought-1".into()];
    p.retirements[0].replaced_by.clear();
    assert_eq!(
        p.apply(&s, &sources).unwrap().retired[0].thought.text,
        s.thoughts[1].text
    );
}
#[test]
fn revision_replays_exactly_and_rejects_stale_results() {
    let (s, _, p) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let mut journal = Journal::open(&path, s.clone()).unwrap();
    let event = Event::MapRevised {
        fingerprint: content_hash(&s).unwrap(),
        plan: p,
    };
    journal.append(10, event.clone()).unwrap();
    let expected = journal.projection.session.clone();
    assert_eq!(journal.at(10).unwrap().session, expected);
    let mut projection = journal.projection.clone();
    assert!(projection.apply(&event).is_err());
    assert_eq!(projection.session, expected);
    assert_eq!(projection.evidence()["thought-1"], s.thoughts[0].text);
    let portable = Projection {
        session: expected.clone(),
        ..Projection::default()
    };
    assert_eq!(portable.evidence(), projection.evidence());
}
#[test]
fn legacy_sessions_omit_new_empty_fields() {
    let json = serde_json::to_value(Session::default()).unwrap();
    assert!(json.get("sources").is_none());
    assert!(json.get("retired").is_none());
}

#[test]
fn source_ledger_survives_split_remove_and_restore() {
    use tacit::session_event::SessionEvent;
    let mut p = Projection::default();
    let capture = SessionEvent::capture(&p.session, "First idea. Second idea.");
    p.apply(&Event::SessionChanged { change: capture }).unwrap();
    let original = p.session.clone();
    p.apply(&Event::SessionChanged {
        change: SessionEvent::Split {
            source_id: 1,
            original: "First idea. Second idea.".into(),
            chunks: vec!["First idea. ".into(), "Second idea.".into()],
        },
    })
    .unwrap();
    assert_eq!(p.evidence()["thought-1"], "First idea. Second idea.");
    assert_eq!(p.evidence()["thought-2"], "Second idea.");
    p.apply(&Event::SessionChanged {
        change: SessionEvent::Removed { id: 2 },
    })
    .unwrap();
    p.apply(&Event::SessionChanged {
        change: SessionEvent::Restored {
            session: Box::new(original),
            reason: "undo".into(),
        },
    })
    .unwrap();
    assert_eq!(p.evidence()["thought-2"], "Second idea.");
}

#[test]
fn unpin_releases_editorial_protection() {
    use tacit::session_event::SessionEvent;
    let (mut s, sources, p) = fixture();
    s.thoughts[1].pinned = true;
    let mut projection = Projection {
        session: s,
        ..Projection::default()
    };
    assert!(p.apply(&projection.session, &sources).is_err());
    projection
        .apply(&Event::SessionChanged {
            change: SessionEvent::Unpinned { id: 2 },
        })
        .unwrap();
    assert!(p.apply(&projection.session, &sources).is_ok());
}
