use std::collections::BTreeSet;
use tacit::agent::pace::{Due, LULL, PAUSE, PRESSURE, Pace, REST, STEADY, STUCK, WORTH};
use tacit::agent::{MAX_THOUGHTS, Op, Tier, at_limit, replay};
use tacit::model::Session;

fn none() -> BTreeSet<u64> {
    BTreeSet::new()
}
fn say(session: &mut Session, count: u64) {
    for _ in 0..count {
        session.add(&format!("Thought {} of the talk.", session.next_id));
    }
}
/// What a placer turn does: each thought goes to one of `parts` main branches.
fn file(session: &Session, ids: std::ops::RangeInclusive<u64>, parts: u64) -> Session {
    let mut ops = vec![];
    for thought in ids {
        let branch = format!("part-{}", thought % parts);
        let grown = |op: &Op| matches!(op, Op::Branch { id, .. } if *id == branch);
        if !session.categories.iter().any(|c| c.id == branch) && !ops.iter().any(grown) {
            ops.push(Op::Branch {
                id: branch.clone(),
                name: format!("Part {}", thought % parts),
                parent: None,
            });
        }
        ops.push(Op::Place {
            thought,
            branch,
            concept: Some(format!("Idea {thought}")),
            role: None,
        });
    }
    replay(session, &ops).unwrap()
}
fn read(session: &Session) -> Session {
    let narrative = "The talk so far, in a sentence.".into();
    replay(session, &[Op::Narrative { narrative }]).unwrap()
}
/// A talk that has reached `said` thoughts, all filed, heard at `now` by a pace that began at 0.
fn heard(pace: &mut Pace, session: &mut Session, more: u64, now: f64) {
    let first = session.next_id;
    say(session, more);
    assert_eq!(pace.due(session, &none(), now), Due::Ask(Tier::Place));
    pace.sent(Tier::Place, session, now);
    *session = file(session, first..=session.next_id - 1, 3);
    pace.landed(Tier::Place, session, true, now);
}
fn look(pace: &mut Pace, session: &Session, from: f64, until: f64) {
    assert_eq!(pace.due(session, &none(), from), Due::Ask(Tier::Steward));
    pace.sent(Tier::Steward, session, from);
    pace.landed(Tier::Steward, session, false, until);
}

#[test]
fn the_placer_is_asked_the_moment_a_thought_waits_and_one_turn_at_a_time() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    assert_eq!(pace.due(&session, &none(), 0.0), Due::Rest);
    say(&mut session, 1);
    assert_eq!(pace.due(&session, &none(), 1.0), Due::Ask(Tier::Place));
    pace.sent(Tier::Place, &session, 1.0);
    assert!(pace.busy() && pace.flying(Tier::Place) && !pace.flying(Tier::Steward));
    // A second thought waits for the turn in flight instead of racing it.
    say(&mut session, 1);
    assert_eq!(pace.due(&session, &none(), 1.5), Due::Rest);
    let session = file(&session, 1..=1, 3);
    pace.landed(Tier::Place, &session, true, 2.0);
    assert!(!pace.busy());
    assert_eq!(pace.due(&session, &none(), 2.0), Due::Ask(Tier::Place));
}

#[test]
fn a_capture_still_being_split_is_left_alone_by_both_tiers() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    heard(&mut pace, &mut session, 3, 0.0);
    say(&mut session, 1);
    let holds = BTreeSet::from([4]);
    assert_eq!(pace.due(&session, &holds, 60.0), Due::Rest);
    assert_eq!(pace.due(&session, &none(), 60.0), Due::Ask(Tier::Place));
}

#[test]
fn the_steward_steps_back_when_the_talk_pauses() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    assert_eq!(pace.due(&session, &none(), 0.0), Due::Rest);
    heard(&mut pace, &mut session, WORTH, 10.0);
    assert_eq!(pace.due(&session, &none(), 11.0), Due::At(10.0 + PAUSE));
    // Words still arriving are not a pause, though no thought has landed.
    pace.speaking(12.0);
    assert_eq!(pace.due(&session, &none(), 13.5), Due::At(12.0 + PAUSE));
    look(&mut pace, &session, 15.0, 19.0);
    assert_eq!(pace.due(&session, &none(), 500.0), Due::Rest);
    // One more thought is worth a look only once the talk has really stopped.
    heard(&mut pace, &mut session, 1, 30.0);
    assert_eq!(pace.due(&session, &none(), 34.0), Due::At(30.0 + LULL));
    look(&mut pace, &session, 40.0, 44.0);
    // What was said while the steward was reading is new to it, after a breath.
    let mut pace = Pace::default();
    let mut session = Session::default();
    pace.due(&session, &none(), 0.0);
    heard(&mut pace, &mut session, WORTH, 10.0);
    assert_eq!(pace.due(&session, &none(), 13.0), Due::Ask(Tier::Steward));
    pace.sent(Tier::Steward, &session, 13.0);
    heard(&mut pace, &mut session, WORTH, 14.0);
    assert_eq!(pace.due(&session, &none(), 15.0), Due::Rest);
    pace.landed(Tier::Steward, &session, false, 18.0);
    assert_eq!(pace.due(&session, &none(), 18.0), Due::At(18.0 + REST));
}

#[test]
fn a_long_run_of_talk_or_a_straining_map_does_not_wait_for_a_pause() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    pace.due(&session, &none(), 0.0);
    heard(&mut pace, &mut session, PRESSURE, 5.0);
    look(&mut pace, &session, 5.0, 9.0);
    // Still talking: the map is given time to be read before it moves again.
    heard(&mut pace, &mut session, PRESSURE, 12.0);
    assert_eq!(pace.due(&session, &none(), 12.0), Due::At(9.0 + STEADY));
    look(&mut pace, &session, 9.0 + STEADY, 33.0);

    // One branch holding the whole map is a strain worth a turn at once.
    let (mut pace, mut session) = (Pace::default(), Session::default());
    pace.due(&session, &none(), 0.0);
    say(&mut session, 5);
    let mut session = file(&session, 1..=5, 1);
    look(&mut pace, &session, 1.0, 5.0);
    // A strain the steward chose to leave is not asked about again with every thought.
    say(&mut session, 1);
    let session = file(&session, 6..=6, 1);
    assert_eq!(pace.due(&session, &none(), 30.0), Due::At(30.0 + LULL));
}

#[test]
fn the_placer_can_call_the_steward() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    pace.due(&session, &none(), 0.0);
    heard(&mut pace, &mut session, 1, 10.0);
    pace.call();
    // A call after one thought waits for a short pause, not a long one.
    assert_eq!(pace.due(&session, &none(), 11.0), Due::At(10.0 + PAUSE));
    heard(&mut pace, &mut session, 2, 12.0);
    assert_eq!(pace.due(&session, &none(), 12.0), Due::Ask(Tier::Steward));
    pace.sent(Tier::Steward, &session, 12.0);
    // A failed turn has not answered the call.
    pace.failed(Tier::Steward, 13.0);
    assert_eq!(pace.due(&session, &none(), 13.0), Due::At(13.0 + REST));
    look(&mut pace, &session, 13.0 + REST, 25.0);
    assert_eq!(pace.due(&session, &none(), 500.0), Due::Rest);
}

#[test]
fn a_user_edit_drops_turns_in_flight_and_rests_the_steward() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    pace.due(&session, &none(), 0.0);
    heard(&mut pace, &mut session, WORTH, 1.0);
    say(&mut session, 1);
    assert_eq!(pace.due(&session, &none(), 2.0), Due::Ask(Tier::Place));
    pace.sent(Tier::Place, &session, 2.0);
    let asked = pace.epoch();
    pace.take_over(&session);
    assert!(!pace.current(asked) && pace.current(pace.epoch()));
    pace.dropped(Tier::Place);
    // What still waits is filed, but their map is not rearranged behind them.
    assert_eq!(pace.due(&session, &none(), 3.0), Due::Ask(Tier::Place));
    pace.sent(Tier::Place, &session, 3.0);
    let mut session = file(&session, 4..=4, 3);
    pace.landed(Tier::Place, &session, true, 4.0);
    assert_eq!(pace.due(&session, &none(), 500.0), Due::Rest);
    heard(&mut pace, &mut session, 1, 600.0);
    assert_eq!(pace.due(&session, &none(), 600.0), Due::At(600.0 + LULL));
}

#[test]
fn failures_wait_longer_each_time_and_a_dead_lane_is_noticed() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    say(&mut session, 1);
    let mut now = 0.0;
    for wait in [2.0, 4.0, 8.0, 16.0, 30.0, 30.0] {
        assert_eq!(pace.due(&session, &none(), now), Due::Ask(Tier::Place));
        pace.sent(Tier::Place, &session, now);
        pace.failed(Tier::Place, now + 1.0);
        assert_eq!(
            pace.due(&session, &none(), now + 1.0),
            Due::At(now + 1.0 + wait)
        );
        now += 1.0 + wait;
    }
    pace.sent(Tier::Place, &session, now);
    let filed = file(&session, 1..=1, 3);
    pace.landed(Tier::Place, &filed, true, now + 1.0);
    say(&mut session, 1);
    assert_eq!(
        pace.due(&session, &none(), now + 1.0),
        Due::Ask(Tier::Place)
    );
    // A turn that never comes back gives its tier up, so the next ask can find the lane gone.
    pace.sent(Tier::Place, &session, 1000.0);
    assert_eq!(pace.due(&session, &none(), 1040.0), Due::Rest);
    assert_eq!(pace.due(&session, &none(), 1049.0), Due::At(1051.0));
    assert!(!pace.busy());
}

#[test]
fn a_placer_that_cannot_file_a_thought_hands_it_to_the_steward_once() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    say(&mut session, 1);
    let mut now = 0.0;
    for _ in 0..STUCK {
        if let Due::At(at) = pace.due(&session, &none(), now) {
            now = at;
        }
        assert_eq!(pace.due(&session, &none(), now), Due::Ask(Tier::Place));
        pace.sent(Tier::Place, &session, now);
        now += 1.0;
        pace.landed(Tier::Place, &session, false, now);
    }
    look(&mut pace, &session, now + PAUSE, now + 9.0);
    // Nobody is paid to try the same thought again until there is something new.
    assert_eq!(pace.due(&session, &none(), 5000.0), Due::Rest);
    say(&mut session, 1);
    assert_eq!(pace.due(&session, &none(), 5001.0), Due::Ask(Tier::Place));
}

#[test]
fn an_opened_map_is_left_as_found_unless_nobody_read_it() {
    let mut session = Session::default();
    say(&mut session, 4);
    let filed = file(&session, 1..=4, 3);
    let mut pace = Pace::default();
    assert_eq!(pace.due(&read(&filed), &none(), 100.0), Due::Rest);
    assert_eq!(pace.due(&read(&filed), &none(), 900.0), Due::Rest);
    let mut pace = Pace::default();
    assert_eq!(pace.due(&filed, &none(), 100.0), Due::At(100.0 + PAUSE));
    look(&mut pace, &filed, 100.0 + PAUSE, 108.0);
    assert_eq!(pace.due(&filed, &none(), 900.0), Due::Rest);
    // Thoughts nobody filed are filed, whenever they were said.
    let mut pace = Pace::default();
    assert_eq!(pace.due(&session, &none(), 100.0), Due::Ask(Tier::Place));
}

#[test]
fn a_map_past_its_limit_is_left_in_the_users_hands() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    say(&mut session, MAX_THOUGHTS as u64);
    assert!(!at_limit(&session));
    assert_eq!(pace.due(&session, &none(), 0.0), Due::Ask(Tier::Place));
    say(&mut session, 1);
    assert!(at_limit(&session));
    assert_eq!(pace.due(&session, &none(), 0.0), Due::Rest);
    assert_eq!(pace.due(&session, &none(), 900.0), Due::Rest);
}

#[test]
fn an_instruction_takes_the_map_until_it_has_landed() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    pace.due(&session, &none(), 0.0);
    heard(&mut pace, &mut session, WORTH, 1.0);
    // The placer is at work when the user says what the map should do.
    say(&mut session, 1);
    assert_eq!(pace.due(&session, &none(), 2.0), Due::Ask(Tier::Place));
    pace.sent(Tier::Place, &session, 2.0);
    let placing = pace.epoch();
    pace.sent(Tier::Direct, &session, 2.5);
    let asked = pace.epoch();
    assert!(pace.flying(Tier::Direct) && pace.awaited(asked) && pace.current(asked));
    // Whatever the agent was in the middle of no longer speaks for the map.
    assert!(!pace.current(placing) && !pace.awaited(placing));
    pace.dropped(Tier::Place);
    // Nothing else is asked until it lands, whatever waits and however long the pause.
    say(&mut session, 1);
    assert_eq!(pace.due(&session, &none(), 3.0), Due::Rest);
    assert_eq!(pace.due(&session, &none(), 60.0), Due::Rest);
    pace.landed(Tier::Direct, &session, false, 61.0);
    assert!(!pace.busy() && !pace.awaited(asked) && !pace.current(asked));
    // Thoughts still waiting are filed at once.
    assert_eq!(pace.due(&session, &none(), 61.0), Due::Ask(Tier::Place));
    pace.sent(Tier::Place, &session, 61.0);
    let mut session = file(&session, 4..=5, 3);
    pace.landed(Tier::Place, &session, true, 62.0);
    // What the user asked for is not rearranged behind them: the steward waits for the talk.
    assert_eq!(pace.due(&session, &none(), 500.0), Due::Rest);
    heard(&mut pace, &mut session, 1, 600.0);
    assert_eq!(pace.due(&session, &none(), 600.0), Due::At(600.0 + LULL));
}

#[test]
fn a_hand_edit_does_not_unask_an_instruction_but_a_cancel_does() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    pace.due(&session, &none(), 0.0);
    heard(&mut pace, &mut session, WORTH, 1.0);
    pace.sent(Tier::Direct, &session, 2.0);
    let first = pace.epoch();
    // The user moves a thought by hand while their instruction is carried out: its answer is
    // still theirs to have, on top of what they did.
    pace.take_over(&session);
    assert!(pace.awaited(first) && !pace.current(first));
    assert_eq!(pace.due(&session, &none(), 3.0), Due::Rest);
    // Taken back before it answered: nobody waits for it, and the agent is free again.
    pace.dropped(Tier::Direct);
    assert!(!pace.awaited(first) && !pace.busy());
    say(&mut session, 1);
    assert_eq!(pace.due(&session, &none(), 4.0), Due::Ask(Tier::Place));
    // Asked again: a late answer to the first is not mistaken for the answer to the second.
    pace.sent(Tier::Direct, &session, 5.0);
    let second = pace.epoch();
    assert!(pace.awaited(second) && !pace.awaited(first));
}

#[test]
fn a_failed_instruction_waits_for_the_user_and_a_dead_lane_is_given_up() {
    let (mut pace, mut session) = (Pace::default(), Session::default());
    pace.due(&session, &none(), 0.0);
    heard(&mut pace, &mut session, WORTH, 1.0);
    say(&mut session, 1);
    // A failure is the user's to retry, at once if they like: nothing is asked again for
    // them, and nothing else is held back.
    for now in [2.0, 3.0, 4.0] {
        pace.sent(Tier::Direct, &session, now);
        assert_eq!(pace.due(&session, &none(), now), Due::Rest);
        pace.failed(Tier::Direct, now + 0.5);
        assert!(!pace.busy());
        assert_eq!(
            pace.due(&session, &none(), now + 0.5),
            Due::Ask(Tier::Place)
        );
    }
    // An instruction that never comes back is given up after four times its patience.
    pace.sent(Tier::Direct, &session, 1000.0);
    let asked = pace.epoch();
    let limit = 4.0 * Tier::Direct.patience().as_secs_f64();
    assert_eq!(pace.due(&session, &none(), 1000.0 + limit), Due::Rest);
    assert!(pace.awaited(asked));
    assert_eq!(
        pace.due(&session, &none(), 1001.0 + limit),
        Due::Ask(Tier::Place)
    );
    assert!(!pace.busy() && !pace.awaited(asked));
}
