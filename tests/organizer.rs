use tacit::{
    model::{Assignment, Interpretation, Relationship, Role, Session},
    organizer,
};
fn proposal(s: &Session) -> Interpretation {
    Interpretation {
        categories: vec![],
        title: "Making space for work".into(),
        narrative: "Meeting load [1] may reflect uncertainty about decisions [2].".into(),
        assignments: s
            .thoughts
            .iter()
            .map(|t| Assignment {
                id: t.id,
                category: None,
                role: Role::Observation,
            })
            .collect(),
        relationships: vec![Relationship {
            from: 1,
            to: 2,
            label: "may reflect".into(),
        }],
    }
}
#[test]
fn model_updates_never_rewrite_words_or_override_user_placement() {
    let mut s = Session::demo();
    s.move_thought(1);
    let original = s.thoughts[0].clone();
    let raw = serde_json::to_string(&proposal(&s)).unwrap();
    organizer::parse(&raw, &s).unwrap().apply(&mut s).unwrap();
    assert_eq!(s.thoughts[0], original);
    assert!(s.interpretation.is_some());
    s.validate().unwrap();
}
#[test]
fn malformed_unknown_duplicate_and_missing_references_are_rejected_atomically() {
    let mut s = Session::demo();
    let before = s.clone();
    let mut p = proposal(&s);
    p.relationships[0].to = 999;
    assert!(p.apply(&mut s).is_err());
    assert_eq!(s, before);
    let mut p = proposal(&s);
    p.assignments.push(p.assignments[0].clone());
    assert!(p.apply(&mut s).is_err());
    assert_eq!(s, before);
    let mut p = proposal(&s);
    p.assignments.pop();
    assert!(p.apply(&mut s).is_err());
    assert_eq!(s, before);
    assert!(organizer::parse("not JSON", &s).is_err());
}
#[test]
fn new_capture_invalidates_old_story() {
    let mut s = Session::demo();
    proposal(&s).apply(&mut s).unwrap();
    s.add("But people may still want a meeting.");
    assert!(s.interpretation.is_none());
}

#[test]
fn themes_validate_references_and_protect_pinned_names_and_placements() {
    use tacit::model::Category;
    let mut s = Session::demo();
    let mut p = proposal(&s);
    p.categories = vec![
        Category {
            id: "decisions".into(),
            name: "Decisions".into(),
        },
        Category {
            id: "time".into(),
            name: "Time to think".into(),
        },
    ];
    for a in &mut p.assignments {
        a.category = Some("decisions".into());
    }
    p.apply(&mut s).unwrap();
    s.move_thought(1);
    let before = s.clone();
    let mut p = s.interpretation.clone().unwrap_or_else(|| proposal(&s));
    p.categories = s.categories.clone();
    for a in &mut p.assignments {
        a.category = Some("decisions".into());
    }
    p.clone().apply(&mut s).unwrap();
    assert_eq!(s.thoughts[0], before.thoughts[0]);
    let before = s.clone();
    p.categories[1].name = "Changed pinned meaning".into();
    assert!(p.apply(&mut s).is_err());
    assert_eq!(s, before);
    s.thoughts[1].category = Some("unknown".into());
    assert!(s.validate().is_err());
}

#[test]
fn old_notebooks_load_without_themes() {
    let mut raw = serde_json::to_value(Session::demo()).unwrap();
    raw.as_object_mut().unwrap().remove("categories");
    for t in raw["thoughts"].as_array_mut().unwrap() {
        t.as_object_mut().unwrap().remove("category");
    }
    let s: Session = serde_json::from_value(raw).unwrap();
    s.validate().unwrap();
    assert!(s.categories.is_empty());
}
