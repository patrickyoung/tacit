use tacit::{
    direction::{self, Edit},
    model::{Assignment, Category, Interpretation, Role, Session},
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
fn session() -> Session {
    let mut s = Session::default();
    s.add("We could pool funding for local research.");
    s.categories = vec![
        category("Research", None),
        category("Funding", Some("Research")),
        category("Shared budgets", Some("Funding")),
    ];
    s.thoughts[0].category = Some("Shared budgets".into());
    s
}
fn interpretation(s: &Session) -> Interpretation {
    Interpretation {
        categories: s.categories.clone(),
        title: s.title.clone(),
        narrative: "Research funding [1].".into(),
        assignments: vec![Assignment {
            id: 1,
            category: s.thoughts[0].category.clone(),
            role: Role::Possibility,
            concept: Some("Proposed pooled funding".into()),
        }],
        relationships: vec![],
    }
}
#[test]
fn three_levels_preserve_sources_and_ancestor_membership() {
    let mut s = session();
    s.validate().unwrap();
    let original = s.thoughts[0].text.clone();
    interpretation(&s).apply(&mut s).unwrap();
    assert_eq!(
        s.placement_path(&s.thoughts[0]),
        "Research › Funding › Shared budgets"
    );
    assert!(s.within_category(&s.thoughts[0], "Research"));
    assert_eq!(s.thoughts[0].text, original);
    assert_eq!(tacit::art::concepts(&s).len(), 1);
    assert_eq!(
        tacit::generated_art::briefs(&s, tacit::art::Style::Pastel, "fixture").len(),
        1
    );
}
#[test]
fn rejects_cycles_orphans_and_excess_depth_atomically() {
    let original = session();
    for categories in [
        vec![
            category("Research", Some("Shared budgets")),
            category("Funding", Some("Research")),
            category("Shared budgets", Some("Funding")),
        ],
        vec![
            category("Research", None),
            category("Funding", Some("missing")),
            category("Shared budgets", Some("Funding")),
        ],
        vec![
            category("Research", None),
            category("Funding", Some("Research")),
            category("Shared budgets", Some("Funding")),
            category("Too deep", Some("Shared budgets")),
        ],
    ] {
        let mut next = original.clone();
        let mut result = interpretation(&next);
        result.categories = categories;
        assert!(result.apply(&mut next).is_err());
        assert_eq!(next, original);
    }
}
#[test]
fn pinned_paths_protect_ancestors_but_explicit_directions_can_move_subtrees() {
    let mut s = session();
    s.thoughts[0].pinned = true;
    s.categories.push(category("Operations", None));
    let mut result = interpretation(&s);
    result.categories[1].parent = Some("Operations".into());
    assert!(result.apply(&mut s).is_err());
    let moved = direction::apply(
        &s,
        "Move Funding under Operations",
        &[Edit::ReparentTheme {
            theme: "Funding".into(),
            parent: Some("Operations".into()),
        }],
    )
    .unwrap();
    assert_eq!(
        moved.placement_path(&moved.thoughts[0]),
        "Operations › Funding › Shared budgets"
    );
    assert!(moved.thoughts[0].pinned);
    assert!(
        direction::apply(
            &s,
            "Make a cycle",
            &[Edit::ReparentTheme {
                theme: "Research".into(),
                parent: Some("Shared budgets".into())
            }]
        )
        .is_err()
    );
}
#[test]
fn merges_preserve_children_and_legacy_categories_remain_flat() {
    let mut s = session();
    s.categories.push(category("Grants", Some("Research")));
    let merged = direction::apply(
        &s,
        "Merge Funding into Grants",
        &[Edit::MergeThemes {
            source: "Funding".into(),
            target: "Grants".into(),
        }],
    )
    .unwrap();
    assert_eq!(
        merged.placement_path(&merged.thoughts[0]),
        "Research › Grants › Shared budgets"
    );
    let old: Category =
        serde_json::from_str(r#"{"id":"old","name":"Old theme","motif":null,"illustration":null}"#)
            .unwrap();
    assert!(old.parent.is_none());
    assert!(serde_json::to_value(&old).unwrap().get("parent").is_none());
}
#[test]
fn nested_organization_and_reparenting_replay_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let s = session();
    let mut j = Journal::open(&path, s.clone()).unwrap();
    j.append(
        100,
        Event::SessionChanged {
            change: SessionEvent::Organized {
                interpretation: interpretation(&s),
                pass: "review".into(),
            },
        },
    )
    .unwrap();
    let nested = j.projection.session.clone();
    j.append(
        200,
        Event::SessionChanged {
            change: SessionEvent::Directed {
                instruction: "Make Funding a root theme".into(),
                edits: vec![Edit::ReparentTheme {
                    theme: "Funding".into(),
                    parent: None,
                }],
            },
        },
    )
    .unwrap();
    let moved = j.projection.session.clone();
    drop(j);
    let j = Journal::read(&path).unwrap();
    assert_eq!(j.at(100).unwrap().session, nested);
    assert_eq!(j.at(200).unwrap().session, moved);
}
