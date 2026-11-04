use tacit::{
    model::{Assignment, Interpretation, Relationship, Role, Session},
    organizer,
};
fn proposal(s: &Session) -> Interpretation {
    Interpretation {
        title: "Making space for work".into(),
        narrative: "Meeting load [1] may reflect uncertainty about decisions [2].".into(),
        assignments: s
            .thoughts
            .iter()
            .map(|t| Assignment {
                id: t.id,
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
