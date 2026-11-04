use tacit::{
    model::{Role, Session, classify},
    storage,
};
#[test]
fn captures_words_and_interprets_possibilities_before_questions() {
    let mut s = Session::default();
    assert_eq!(s.add("  "), None);
    s.add("What if we invited fewer people?");
    assert_eq!(s.thoughts[0].text, "What if we invited fewer people?");
    assert_eq!(s.thoughts[0].role, Role::Possibility);
    assert_eq!(classify("The butterfly is blue."), Role::Observation);
    assert_eq!(classify("But everyone wants a voice."), Role::Tension);
}
#[test]
fn corrections_are_pinned_and_survive_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/session.json");
    let mut s = Session::demo();
    s.move_thought(1);
    assert!(s.thoughts[0].pinned);
    storage::save(&path, &s).unwrap();
    assert_eq!(storage::load(&path).unwrap(), s);
}
#[test]
fn corrupt_or_future_sessions_are_not_silently_reset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    std::fs::write(&path, b"not json").unwrap();
    assert!(storage::load(&path).is_err());
    let s = Session {
        version: 2,
        ..Session::default()
    };
    assert!(storage::save(&path, &s).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"not json");
}

#[test]
fn a_second_writer_cannot_open_the_same_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let lease = storage::lock(&path).unwrap();
    assert!(storage::lock(&path).is_err());
    drop(lease);
    assert!(storage::lock(&path).is_ok());
}
