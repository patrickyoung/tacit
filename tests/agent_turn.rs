//! A turn against a scripted model: what the agent is shown, and when it is asked again.
use openrouter_rs::api::chat::Message;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use tacit::agent::turn::Stands;
use tacit::agent::{Op, Tier, Turn, brief, guided, health, prompt, replay, turn};
use tacit::model::{ParkKind, Relationship, Role, Session};
use tacit::session_event::SessionEvent;

fn none() -> BTreeSet<u64> {
    BTreeSet::new()
}
fn talk(lines: &[&str]) -> Session {
    let mut session = Session::default();
    for line in lines {
        session.add(line);
    }
    session
}
fn branch(id: &str, name: &str, parent: Option<&str>) -> Op {
    Op::Branch {
        id: id.into(),
        name: name.into(),
        parent: parent.map(Into::into),
    }
}
fn place(thought: u64, branch: &str, concept: &str) -> Op {
    Op::Place {
        thought,
        branch: branch.into(),
        concept: Some(concept.into()),
        role: None,
    }
}
/// Two main branches with two thoughts each.
fn map() -> Session {
    let session = talk(&[
        "Nobody knows who decides.",
        "Decisions get reopened every week.",
        "Our meetings run long.",
        "Half the invitees never speak.",
    ]);
    let ops = [
        branch("decisions", "Decisions", None),
        branch("meetings", "Meetings", None),
        place(1, "decisions", "Unclear decision owner"),
        place(2, "decisions", "Reopened decisions"),
        place(3, "meetings", "Long meetings"),
        place(4, "meetings", "Silent invitees"),
    ];
    replay(&session, &ops).unwrap()
}
/// The same map with two thoughts just said.
fn fresh() -> Session {
    let mut session = map();
    session.add("Agendas arrive late.");
    session.add("No one keeps time.");
    session
}
/// One branch carrying `n` thoughts directly.
fn crowd(n: u64) -> Session {
    let mut session = Session::default();
    let mut ops = vec![branch("meetings", "Meetings", None)];
    for id in 1..=n {
        session.add(&format!("Meeting problem number {id}."));
        ops.push(place(id, "meetings", &format!("Problem {id}")));
    }
    replay(&session, &ops).unwrap()
}

type Answer = Result<Value, &'static str>;
/// One turn with a model that gives `answers` in order: the tier's own, or with `instruction`
/// one that carries it out. Returns the turn and what the model was shown on each call.
/// `stands` is what the steward is not sent back to: what it left, and what the user asked for.
fn scripted(
    tier: Tier,
    session: &Session,
    holds: &BTreeSet<u64>,
    instruction: Option<&str>,
    stands: &mut Stands,
    answers: Vec<Answer>,
) -> (Result<Turn, String>, Vec<Value>) {
    let mut answers = VecDeque::from(answers);
    let mut heard = vec![];
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let model = async |messages: &[Message], budget: u32| {
        heard.push(json!({"budget": budget, "messages": messages}));
        let answer = answers.pop_front().expect("asked more often than scripted");
        answer.map(|answer| answer.to_string()).map_err(Into::into)
    };
    let result = match instruction {
        Some(wish) => runtime.block_on(turn::follow(session, holds, wish, stands, model)),
        None => runtime.block_on(turn::drive(tier, session, holds, stands, model)),
    };
    (result, heard)
}
fn drive(
    tier: Tier,
    session: &Session,
    holds: &BTreeSet<u64>,
    answers: Vec<Answer>,
) -> (Result<Turn, String>, Vec<Value>) {
    scripted(tier, session, holds, None, &mut Stands::default(), answers)
}
/// A turn that carries out what the user asked of the map.
fn follow(
    session: &Session,
    holds: &BTreeSet<u64>,
    instruction: &str,
    answers: Vec<Answer>,
) -> (Result<Turn, String>, Vec<Value>) {
    let stands = &mut Stands::default();
    scripted(
        Tier::Direct,
        session,
        holds,
        Some(instruction),
        stands,
        answers,
    )
}
/// The last thing the model was told on a call.
fn told(heard: &[Value], call: usize) -> &str {
    let messages = heard[call]["messages"].as_array().unwrap();
    messages.last().unwrap()["content"].as_str().unwrap()
}
/// A placer's answer filing each `(thought, branch, idea)`.
fn filing(entries: &[(u64, &str, &str)]) -> Value {
    let file: Vec<Value> = entries
        .iter()
        .map(|(id, parent, idea)| {
            json!({"id": id, "idea": idea, "outcome": "place", "parent": parent,
                "reason": null, "role": null, "topic": null})
        })
        .collect();
    json!({"about": null, "branches": null, "file": file, "steward": null})
}

#[test]
fn the_placer_is_asked_again_only_for_a_thought_left_waiting() {
    let session = fresh();
    let first = filing(&[(5, "meetings", "Late agendas")]);
    let second = filing(&[(6, "meetings", "No timekeeper")]);
    let script = vec![Ok(first.clone()), Ok(second)];
    let (turn, heard) = drive(Tier::Place, &session, &none(), script);
    let turn = turn.unwrap();
    assert_eq!(
        turn.ops,
        [
            place(5, "meetings", "Late agendas"),
            place(6, "meetings", "No timekeeper")
        ]
    );
    assert!(replay(&session, &turn.ops).is_ok());
    assert_eq!(heard.len(), 2);

    let opening = heard[0]["messages"].as_array().unwrap();
    assert_eq!(opening.len(), 2);
    assert_eq!(opening[0]["role"], "system");
    assert_eq!(opening[0]["content"], prompt::PLACER);
    assert_eq!(opening[1]["role"], "user");
    assert_eq!(
        opening[1]["content"],
        brief::place(&session, &none(), &Stands::default())
            .unwrap()
            .0
    );
    assert_eq!(heard[0]["budget"], 2000 + 160 * 2);

    // The correction continues the same conversation: its own answer, then what the map said.
    let again = heard[1]["messages"].as_array().unwrap();
    assert_eq!(again.len(), 4);
    assert_eq!(again[2]["role"], "assistant");
    assert_eq!(again[2]["content"], first.to_string());
    let feedback = told(&heard, 1);
    assert!(feedback.starts_with("Checked against the map: 1 edit(s) applied.\n"));
    assert!(feedback.contains("Still without a branch: #6. Place or park each one.\n"));
    assert!(feedback.ends_with(prompt::REPAIR));

    // Filing everything at once costs one call.
    let both = filing(&[
        (5, "meetings", "Late agendas"),
        (6, "meetings", "No timekeeper"),
    ]);
    let (turn, heard) = drive(Tier::Place, &session, &none(), vec![Ok(both)]);
    assert_eq!((turn.unwrap().ops.len(), heard.len()), (2, 1));
}

#[test]
fn a_refusal_that_strands_nothing_is_not_worth_a_second_call() {
    // Re-filing a settled thought is outside the placer's licence. The new ones are home.
    let answer = filing(&[
        (1, "meetings", "Unclear decision owner"),
        (5, "meetings", "Late agendas"),
        (6, "meetings", "No timekeeper"),
    ]);
    let (turn, heard) = drive(Tier::Place, &fresh(), &none(), vec![Ok(answer)]);
    assert_eq!(heard.len(), 1);
    assert_eq!(
        turn.unwrap().ops,
        [
            place(5, "meetings", "Late agendas"),
            place(6, "meetings", "No timekeeper")
        ]
    );
}

#[test]
fn a_failed_correction_keeps_what_already_held() {
    let first = filing(&[(5, "meetings", "Late agendas")]);
    let unreadable = json!("Here is the rest of the map");
    for second in [
        Err("The map agent timed out; your map is safe"),
        Ok(unreadable),
    ] {
        let script = vec![Ok(first.clone()), second];
        let (turn, heard) = drive(Tier::Place, &fresh(), &none(), script);
        assert_eq!(heard.len(), 2);
        assert_eq!(turn.unwrap().ops, [place(5, "meetings", "Late agendas")]);
    }
}

#[test]
fn a_turn_that_cannot_start_says_why() {
    let failed = |tier, session: &Session, holds: &BTreeSet<u64>, script: Vec<Answer>| {
        let (turn, heard) = drive(tier, session, holds, script);
        (turn.unwrap_err(), heard.len())
    };
    let down = "The map agent is unavailable; check key, credits and model availability";
    for tier in [Tier::Place, Tier::Steward] {
        assert_eq!(
            failed(tier, &fresh(), &none(), vec![Err(down)]),
            (down.into(), 1)
        );
        // A garbled answer is asked for once more, and no more than that.
        let unreadable = || Ok(json!(["not", "an", "answer"]));
        assert_eq!(
            failed(tier, &fresh(), &none(), vec![unreadable(), unreadable()]),
            ("The map agent returned an unreadable answer".into(), 2)
        );
    }
    let both = filing(&[
        (5, "meetings", "Late agendas"),
        (6, "meetings", "No timekeeper"),
    ]);
    let script = vec![Ok(json!("Sure, here it is")), Ok(both)];
    let (turn, heard) = drive(Tier::Place, &fresh(), &none(), script);
    let turn = turn.unwrap();
    assert!(turn.garbled && turn.ops.len() == 2 && heard.len() == 2);
    // The second asking is the first again: nothing was said that could be answered.
    assert_eq!(heard[0], heard[1]);
    // No model is asked when there is nothing to ask about.
    let idle = "No thoughts are waiting for a branch";
    assert_eq!(
        failed(Tier::Place, &map(), &none(), vec![]),
        (idle.into(), 0)
    );
    let held = BTreeSet::from([5, 6]);
    assert_eq!(
        failed(Tier::Place, &fresh(), &held, vec![]),
        (idle.into(), 0)
    );
    let mut vast = Session::default();
    for n in 0..401 {
        vast.add(&format!("Thought {n}"));
    }
    let limit = "Map limit reached (400 thoughts); editing by hand remains available";
    for tier in [Tier::Place, Tier::Steward] {
        assert_eq!(failed(tier, &vast, &none(), vec![]), (limit.into(), 0));
    }
}

#[test]
fn the_placer_can_call_for_the_steward() {
    let mut answer = filing(&[
        (5, "meetings", "Late agendas"),
        (6, "meetings", "No timekeeper"),
    ]);
    answer["steward"] = json!("Meetings now mixes agendas with attendance");
    let (turn, _) = drive(Tier::Place, &fresh(), &none(), vec![Ok(answer)]);
    let turn = turn.unwrap();
    assert_eq!(
        turn.escalate.as_deref(),
        Some("Meetings now mixes agendas with attendance")
    );
    assert_eq!((turn.tier, turn.note.as_str()), (Tier::Place, ""));
}

const READING: &str = "Nobody owns decisions, so meetings fill the gap.";
fn kinds(ops: &[Op]) -> Vec<String> {
    let kind = |op| {
        serde_json::to_value(op).unwrap()["op"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    ops.iter().map(kind).collect()
}

#[test]
fn the_steward_stops_when_the_map_is_sound() {
    let session = map();
    let answer = json!({
        "aim": {"focus": "Why decisions drag", "principle": "Causes of delay", "title": "Slow decisions"},
        "audit": "Sound; named the subject.",
        "narrative": READING
    });
    let (turn, heard) = drive(Tier::Steward, &session, &none(), vec![Ok(answer)]);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0]["messages"][0]["content"], prompt::STEWARD);
    assert_eq!(
        told(&heard, 0),
        brief::steward(&session, &none(), &Stands::default())
    );
    assert_eq!(heard[0]["budget"], 12_000 + 120 * 4);
    assert_eq!(kinds(&turn.ops), ["focus", "title", "narrative"]);
    assert_eq!(turn.note, "Sound; named the subject.");
    assert_eq!((turn.tier, turn.escalate), (Tier::Steward, None));
    let after = replay(&session, &turn.ops).unwrap();
    assert_eq!(after.title, "Slow decisions");
    assert_eq!(after.narrative(), READING);
}

#[test]
fn the_steward_is_asked_for_a_missing_narrative() {
    // The offline placeholder is not a narrative: a map no model has read still needs one.
    assert!(!map().narrative().is_empty());
    assert_eq!(brief::narrative(&map()), "");
    let script = vec![
        Ok(json!({"audit": "Sound; no change."})),
        Ok(json!({"narrative": READING})),
    ];
    let (turn, heard) = drive(Tier::Steward, &map(), &none(), script);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 2);
    let expected = format!(
        "Checked against the map: 0 edit(s) applied.\nThe map has no narrative yet. Give one.\n{}",
        prompt::REPAIR
    );
    assert_eq!(told(&heard, 1), expected);
    let narrative = READING.into();
    assert_eq!(turn.ops, [Op::Narrative { narrative }]);
    assert_eq!(turn.note, "Sound; no change.");
}

#[test]
fn an_edit_the_map_refused_earns_one_correction() {
    let rename = json!({"rename": [{"branch": "nowhere", "name": "Elsewhere"}]});
    let first = json!({"audit": "Renamed a branch.", "branches": rename, "narrative": READING});
    let again = json!({"audit": "Tried again.", "branches": rename});
    let script = vec![Ok(first), Ok(again.clone()), Ok(again)];
    let (turn, heard) = drive(Tier::Steward, &map(), &none(), script);
    let turn = turn.unwrap();
    // Worth one more try, not a loop.
    assert_eq!(heard.len(), 2);
    let expected = format!(
        "Checked against the map: 1 edit(s) applied.\nRefused:\n- {}: There is no branch nowhere\n{}",
        r#"{"op":"rename","branch":"nowhere","name":"Elsewhere"}"#,
        prompt::REPAIR
    );
    assert_eq!(told(&heard, 1), expected);
    assert_eq!(kinds(&turn.ops), ["narrative"]);
    // A correction that changed nothing does not get to describe the turn.
    assert_eq!(turn.note, "Renamed a branch.");
}

#[test]
fn what_must_be_done_gets_three_calls_and_no_more() {
    // Two thoughts still have no branch after every answer.
    let script = vec![
        Ok(json!({"narrative": READING})),
        Ok(json!({})),
        Ok(json!({})),
        Ok(json!({})),
    ];
    let (turn, heard) = drive(Tier::Steward, &fresh(), &none(), script);
    assert_eq!(heard.len(), 3);
    assert!(told(&heard, 2).contains("Still without a branch: #5, #6. Place or park each one.\n"));
    assert_eq!(kinds(&turn.unwrap().ops), ["narrative"]);
    // A capture still being split is not the steward's to place, and is not asked for.
    let held = BTreeSet::from([5, 6]);
    let script = vec![Ok(json!({"narrative": READING}))];
    let (_, heard) = drive(Tier::Steward, &fresh(), &held, script);
    assert_eq!(heard.len(), 1);
    assert!(!told(&heard, 0).contains("UNFILED"));
}

#[test]
fn a_map_that_still_strains_gets_one_more_look() {
    let session = crowd(8);
    let file: Vec<Value> = (1..=8)
        .map(|id| {
            let parent = if id <= 4 { "agendas" } else { "attendance" };
            json!({"id": id, "idea": null, "parent": parent, "role": null})
        })
        .collect();
    let split = json!({
        "audit": "Meetings carried too much; divided it.",
        "branches": {"grow": [{"id": "agendas", "name": "Agendas", "parent": "meetings"},
            {"id": "attendance", "name": "Attendance", "parent": "meetings"}]},
        "ideas": {"file": file}
    });
    let script = vec![
        Ok(json!({"narrative": "Eight complaints about meetings."})),
        Ok(split),
        Ok(json!({})),
    ];
    let (turn, heard) = drive(Tier::Steward, &session, &none(), script);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 2);
    let feedback = told(&heard, 1);
    assert!(feedback.contains("The map still strains. Fix what a seasoned mapper would fix"));
    assert!(feedback.contains("\n- Branch meetings (Meetings) has 8 items directly under it."));
    assert_eq!(turn.note, "Meetings carried too much; divided it.");
    assert_eq!(turn.ops.len(), 11);
    let after = replay(&session, &turn.ops).unwrap();
    assert_eq!(health::shape(&after).crowded, 0);
    assert_eq!(health::items(&after, "meetings"), (0, 2));
}

#[test]
fn a_strain_the_steward_chose_to_leave_is_not_put_to_it_twice() {
    // Eight thoughts in one list strain the map, and here a list is what was wanted.
    let (mut session, stands) = (crowd(8), &mut Stands::default());
    let tend = |session: &Session, stands: &mut Stands, answers| {
        scripted(Tier::Steward, session, &none(), None, stands, answers)
    };
    let left = json!({"audit": "One list, as asked.", "narrative": "Eight complaints."});
    let (turn, heard) = tend(&session, stands, vec![Ok(left), Ok(json!({}))]);
    // Told in the brief and sent back once, it left the list a list.
    assert_eq!(heard.len(), 2);
    assert!(told(&heard, 1).contains("\n- Branch meetings (Meetings) has 8 items directly"));
    let chosen = ["few".to_owned(), "packed meetings".to_owned()];
    assert_eq!(stands.left, BTreeSet::from(chosen));
    session = replay(&session, &turn.unwrap().ops).unwrap();
    // The next look is told of the same strains, and is taken at its word.
    let sound = json!({"audit": "Sound; no change."});
    let (_, heard) = tend(&session, stands, vec![Ok(sound)]);
    assert_eq!(heard.len(), 1);
    assert!(told(&heard, 0).contains("\n- STRAIN: Branch meetings (Meetings) has 8 items"));
    // A strain of its own making is still put to it, and that one alone.
    let part = |id: u64| format!("part-{id}");
    let grow: Vec<Value> = (1..=3)
        .map(|id| json!({"id": part(id), "name": format!("Part {id}"), "parent": "meetings"}))
        .collect();
    let file: Vec<Value> = (1..=3)
        .map(|id| json!({"id": id, "idea": null, "parent": part(id), "role": null}))
        .collect();
    let apart = json!({"branches": {"grow": grow}, "ideas": {"file": file}});
    let (turn, heard) = tend(&session, stands, vec![Ok(apart), Ok(json!({}))]);
    assert_eq!(heard.len(), 2);
    let feedback = told(&heard, 1);
    assert!(feedback.contains("\n- Branch meetings (Meetings) has 3 sub-branches holding one"));
    assert!(!feedback.contains("items directly under it"));
    assert!(!feedback.contains("thoughts hang from"));
    assert_eq!(stands.left.len(), 3);
    assert!(stands.left.contains("thin meetings"));
    session = replay(&session, &turn.unwrap().ops).unwrap();
    // What is mended is forgotten: should it come back, it is a new strain.
    let fold = [
        json!({"from": "part-2", "into": "part-1"}),
        json!({"from": "part-3", "into": "part-1"}),
    ];
    let fold = json!({"branches": {"merge": fold}});
    let (turn, heard) = tend(&session, stands, vec![Ok(fold)]);
    assert_eq!(heard.len(), 1);
    assert_eq!(kinds(&turn.unwrap().ops), ["merge", "merge"]);
    assert_eq!(stands.left, BTreeSet::from(["few".to_owned()]));
}

/// Twelve thoughts under three main branches, and nothing strained.
fn thirds() -> Session {
    let mut session = Session::default();
    let roots = ["theory", "journeys", "endings"];
    let mut ops: Vec<Op> = roots.iter().map(|id| branch(id, id, None)).collect();
    for id in 1..=12 {
        session.add(&format!("Thought number {id}."));
        let home = roots[(id > 6) as usize + (id > 9) as usize];
        ops.push(place(id, home, &format!("Idea {id}")));
    }
    let session = replay(&session, &ops).unwrap();
    assert!(!health::strained(&session));
    session
}
/// Each of the thoughts filed under `parent`, as the steward or a directed turn writes it.
fn file(ids: std::ops::RangeInclusive<u64>, parent: &str) -> Vec<Value> {
    ids.map(|id| json!({"id": id, "idea": null, "parent": parent, "role": null}))
        .collect()
}

#[test]
fn a_branch_the_user_arranged_is_theirs_until_it_outgrows_what_they_left() {
    let mut session = thirds();
    // The user gathers seven of them under one branch, which leaves the map two.
    let ask = "Put all seven plots together under one branch called Plots.";
    let gather = json!({
        "branches": {"grow": [{"id": "plots", "name": "Plots", "parent": null}],
            "remove": ["journeys", "endings"]},
        "ideas": {"file": file(6..=12, "plots")},
        "reply": "All seven plots are under Plots."
    });
    let (lane, stands) = (&mut Stands::default(), &mut Stands::default());
    let script = vec![Ok(gather)];
    let (turn, _) = scripted(Tier::Direct, &session, &none(), Some(ask), lane, script);
    session = guided(&session, ask, &turn.unwrap().ops).unwrap();
    assert_eq!(health::shape(&session).roots, 2);
    // Theirs, handed to the steward's lane: the strain the instruction made and the branch it
    // filled. The branch it only took from is not.
    stands.absorb(lane);
    assert_eq!(*lane, Stands::default());
    let made = BTreeMap::from([("few".to_owned(), ask.to_owned())]);
    let theirs = BTreeMap::from([("plots".to_owned(), (ask.to_owned(), 7))]);
    assert_eq!((&stands.made, &stands.asked), (&made, &theirs));
    let tend = |session: &Session, stands: &mut Stands, answers| {
        scripted(Tier::Steward, session, &none(), None, stands, answers)
    };
    // The steward grows a third main branch, and Plots now holds most of the map: a strain
    // of the steward's making on a branch that is as the user left it. It is not sent back.
    let third = json!({
        "audit": "Grew Uses.",
        "branches": {"grow": [{"id": "uses", "name": "Uses", "parent": null}]},
        "ideas": {"file": file(4..=5, "uses")},
        "narrative": READING
    });
    let (turn, heard) = tend(&session, stands, vec![Ok(third)]);
    assert_eq!(heard.len(), 1);
    session = replay(&session, &turn.unwrap().ops).unwrap();
    assert!(stands.left.is_empty() && stands.made.is_empty());
    // From then on the steward is told to leave it and the placer is told nothing. Only a
    // lane that never heard of the instruction calls it a fault.
    let heavy = "Main branch plots (Plots) holds 7 of 12 thoughts.";
    let theirs = "This stands on the user's word: leave it as they arranged it.";
    let unheard = &Stands::default();
    let read = brief::steward(&session, &none(), stands);
    assert!(read.contains(&format!("\nHEALTH\n- {heavy} {theirs}\n")));
    assert!(!read.contains("STRAIN"));
    let fault = format!("\nHEALTH\n- STRAIN: {heavy} Split it along a real distinction");
    assert!(brief::steward(&session, &none(), unheard).contains(&fault));
    session.add("Thought number 13.");
    let (text, _) = brief::place(&session, &none(), stands).unwrap();
    assert!(!text.contains("HEALTH"));
    let (text, _) = brief::place(&session, &none(), unheard).unwrap();
    assert!(text.contains(&format!(
        "\nHEALTH\n- {heavy} Split it along a real distinction"
    )));
    // One more plot, and the branch is no longer what they asked for.
    session = replay(&session, &[place(13, "plots", "Idea 13")]).unwrap();
    let sound = json!({"audit": "Sound; no change."});
    let (_, heard) = tend(&session, stands, vec![Ok(sound), Ok(json!({}))]);
    assert_eq!(heard.len(), 2);
    assert!(told(&heard, 0).contains("\n- STRAIN: Main branch plots (Plots) holds 8 of 13"));
    let feedback = told(&heard, 1);
    assert!(feedback.contains("\n- Main branch plots (Plots) holds 8 of 13 thoughts"));
    assert!(feedback.contains("\n- Branch plots (Plots) has 8 items directly under it."));
    let left = BTreeSet::from(["heavy plots".to_owned(), "packed plots".to_owned()]);
    let chosen = Stands {
        left,
        ..Stands::default()
    };
    assert_eq!(*stands, chosen);
}

#[test]
fn an_instruction_that_was_taken_back_arranged_nothing() {
    let before = thirds();
    // The user empties two main branches into the third: one list of twelve, on their word.
    let ask = "Put everything under theory.";
    let gather = json!({
        "branches": {"remove": ["journeys", "endings"]},
        "ideas": {"file": file(7..=12, "theory")},
        "reply": "Everything is under theory."
    });
    let (lane, stands) = (&mut Stands::default(), &mut Stands::default());
    let script = vec![Ok(gather)];
    let (turn, _) = scripted(Tier::Direct, &before, &none(), Some(ask), lane, script);
    let after = guided(&before, ask, &turn.unwrap().ops).unwrap();
    assert_eq!(health::shape(&after).roots, 1);
    let made = ["few", "packed theory"].map(|key| (key.to_owned(), ask.to_owned()));
    let theirs = BTreeMap::from([("theory".to_owned(), (ask.to_owned(), 12))]);
    assert_eq!((&lane.made, &lane.asked), (&BTreeMap::from(made), &theirs));
    stands.absorb(lane);
    let tend = |session: &Session, stands: &mut Stands, answers| {
        scripted(Tier::Steward, session, &none(), None, stands, answers)
    };
    // While the instruction stands, so does the list: the steward is told to leave it, and
    // is not sent back to what the instruction strained.
    let word = "This stands on the user's word: leave it as they arranged it.";
    let sound = || json!({"audit": "Sound; no change.", "narrative": READING});
    let (_, heard) = tend(&after, stands, vec![Ok(sound())]);
    assert_eq!(heard.len(), 1);
    let list = "Branch theory (theory) has 12 items directly under it.";
    assert!(told(&heard, 0).contains(&format!("\n- {list} {word}\n")));
    assert_eq!((stands.made.len(), &stands.asked), (2, &theirs));
    // The user undoes it, and the map is as it was. Then theory fills by itself.
    let (mut session, mut ops) = (before, vec![]);
    for id in 13..=15 {
        session.add(&format!("Thought number {id}."));
        let home = if id == 15 { "journeys" } else { "theory" };
        ops.push(place(id, home, &format!("Idea {id}")));
    }
    session = replay(&session, &ops).unwrap();
    // Nothing the instruction did is left to stand on. The crowding is the map's own, and the
    // steward answers for it as for any other.
    let (_, heard) = tend(&session, stands, vec![Ok(sound()), Ok(json!({}))]);
    assert_eq!(heard.len(), 2);
    let list = "Branch theory (theory) has 8 items directly under it. Group them";
    assert!(told(&heard, 0).contains(&format!("\n- STRAIN: {list}")));
    assert!(!told(&heard, 0).contains(word));
    assert!(told(&heard, 1).contains(&format!("\n- {list}")));
    let left = BTreeSet::from(["packed theory".to_owned()]);
    let chosen = Stands {
        left,
        ..Stands::default()
    };
    assert_eq!(*stands, chosen);
}

#[test]
fn the_steward_is_told_first_what_it_will_be_held_to() {
    // An empty branch the user asked for, ahead of one that carries too much.
    let ops = [
        branch("spare", "Spare", None),
        Op::Order {
            branches: vec!["spare".into(), "meetings".into()],
        },
    ];
    let session = guided(&crowd(8), "Add a spare branch, first.", &ops).unwrap();
    // The checklist reads the map top to bottom: a remark, then a fault.
    let notes = health::notes(&session);
    assert_eq!(notes.len(), 4);
    assert!(notes[1].starts_with("Branch spare (Spare) is empty."));
    assert!(notes[2].starts_with("Branch meetings (Meetings) has 8 items"));
    // The brief puts the faults before the remarks, and says which they are.
    let expected = format!(
        "\nHEALTH\n- STRAIN: {}\n- STRAIN: {}\n- {}\n- {}\n",
        notes[0], notes[2], notes[1], notes[3]
    );
    assert!(brief::steward(&session, &none(), &Stands::default()).ends_with(&expected));
    // The placer hears of the faults alone, as before.
    let mut more = session.clone();
    more.add("One more thing.");
    let (text, _) = brief::place(&more, &none(), &Stands::default()).unwrap();
    assert!(text.contains(&format!("\nHEALTH\n- {}\n- {}\n", notes[0], notes[2])));
    assert!(!text.contains("STRAIN"));
}

#[test]
fn an_eighth_main_branch_stands_on_the_users_word() {
    let mut session = Session::default();
    let mut ops = vec![];
    for n in 1..=8 {
        let id = format!("part-{n}");
        session.add(&format!("Thought {n}."));
        ops.push(branch(&id, &format!("Part {n}"), None));
        ops.push(place(n, &id, &format!("Idea {n}")));
    }
    let seven = replay(&session, &ops[..14]).unwrap();
    let eight = guided(&seven, "Give part eight a main branch.", &ops[14..]).unwrap();
    assert_eq!(health::shape(&eight).roots, 8);
    // Wider than the agent grows a map, and no fault for it to repair.
    assert!(!health::strained(&eight));
    let note = "8 main branches, where a map reads best with three to seven. The eighth stands on the user's word: merge two only if they cover the same part of the subject.";
    assert_eq!(health::notes(&eight)[0], note);
    let brief = brief::steward(&eight, &none(), &Stands::default());
    assert!(brief.contains("\nMAP (8 main branches: full, it takes no more)\n"));
    assert!(brief.contains(&format!("\nHEALTH\n- {note}\n")));
    // So a steward that finds the map sound is not sent back to narrow it.
    let script = vec![Ok(
        json!({"audit": "Sound; no change.", "narrative": READING}),
    )];
    let (turn, heard) = drive(Tier::Steward, &eight, &none(), script);
    assert_eq!(heard.len(), 1);
    assert_eq!(kinds(&turn.unwrap().ops), ["narrative"]);
    // The placer is told the map is full, and nothing of what is not its to fix.
    let mut more = eight.clone();
    more.add("One more thing.");
    let (text, targets) = brief::place(&more, &none(), &Stands::default()).unwrap();
    assert_eq!(targets, [9]);
    assert!(text.contains("\nMAP (8 main branches: full, it takes no more)\n"));
    assert!(!text.contains("HEALTH"));
}

fn park(thought: u64, kind: ParkKind, topic: &str, reason: &str) -> Op {
    Op::Park {
        thought,
        kind,
        topic: topic.into(),
        reason: reason.into(),
        concept: None,
    }
}

#[test]
fn the_checklist_reads_a_map_the_way_a_mapper_would() {
    let sound = map();
    assert!(!health::strained(&sound));
    let shape = health::Shape {
        filed: 4,
        unfiled: 0,
        parked: 0,
        roots: 2,
        branches: 2,
        heaviest: 2,
        crowded: 0,
        thin: 0,
    };
    assert_eq!(health::shape(&sound), shape);
    let aimless = "The map has no stated focus or ordering principle yet.";
    assert_eq!(health::notes(&sound), [aimless]);
    // Thoughts waiting for the placer are not a fault in the map.
    assert_eq!(health::notes(&fresh()), [aimless]);
    assert_eq!(health::shape(&fresh()).unfiled, 2);

    // One branch is the subject again, not a part of it: four thoughts pass, the fifth tells.
    assert!(!health::strained(&crowd(4)));
    assert!(health::notes(&crowd(5))[0].starts_with("5 thoughts hang from 1 main branch(es)."));
    // Seven items read at a glance; the eighth does not.
    assert_eq!(health::shape(&crowd(7)).crowded, 0);
    let crowded = crowd(8);
    assert!(health::strained(&crowded));
    assert_eq!(health::items(&crowded, "meetings"), (8, 0));
    let shape = health::shape(&crowded);
    assert_eq!((shape.crowded, shape.heaviest), (1, 8));
    let notes = health::notes(&crowded);
    assert!(notes[0].starts_with("8 thoughts hang from 1 main branch(es)."));
    assert!(notes[1].starts_with("Branch meetings (Meetings) has 8 items directly under it."));

    // One main branch holding most of the map is a list, not a map.
    let mut lopsided = crowd(6);
    for line in ["People are tired.", "Morale is low.", "The tools are slow."] {
        lopsided.add(line);
    }
    let ops = [
        branch("people", "People", None),
        branch("tools", "Tools", None),
        place(7, "people", "Tired people"),
        place(8, "people", "Low morale"),
        place(9, "tools", "Slow tools"),
    ];
    let lopsided = replay(&lopsided, &ops).unwrap();
    assert!(health::strained(&lopsided));
    let heavy = "Main branch meetings (Meetings) holds 6 of 9 thoughts.";
    assert!(health::notes(&lopsided)[0].starts_with(heavy));
    let ops = [5, 6].map(|id| place(id, "tools", &format!("Problem {id}")));
    assert!(!health::strained(&replay(&lopsided, &ops).unwrap()));

    // A sub-branch earns its place with two thoughts; an empty branch earns nothing.
    let ops = [
        branch("agendas", "Agendas", Some("meetings")),
        branch("breaks", "Breaks", Some("meetings")),
        branch("chairs", "Chairs", Some("meetings")),
        place(1, "agendas", "Problem 1"),
        place(2, "breaks", "Problem 2"),
    ];
    let thin = replay(&crowd(4), &ops).unwrap();
    assert_eq!(health::shape(&thin).thin, 2);
    assert!(!health::strained(&thin));
    let notes = health::notes(&thin);
    let lone = "Branch meetings (Meetings) has 2 sub-branches holding one thought each.";
    assert!(notes.iter().any(|note| note.starts_with(lone)));
    let empty = "Branch chairs (Chairs) is empty. Remove it unless the user asked for it.";
    assert!(notes.iter().any(|note| note == empty));
    let thinner = replay(&thin, &[place(3, "chairs", "Problem 3")]).unwrap();
    assert!(health::strained(&thinner));

    // A thought parked for later is a standing question to the steward.
    let later = park(5, ParkKind::Later, "Agendas", "Deferred by the speaker");
    let waiting = replay(&fresh(), &[later]).unwrap();
    assert_eq!(health::shape(&waiting).parked, 1);
    let back = "1 parked thought(s) are marked later. Bring back any the map now has a home for.";
    assert!(health::notes(&waiting).iter().any(|note| note == back));

    // The same words on two thoughts are one idea heard twice or two not yet told apart,
    // wherever they hang and however they are capitalised. Worth saying, not worth a turn.
    let echo = replay(&sound, &[place(4, "meetings", "unclear decision owner")]).unwrap();
    assert!(!health::strained(&echo));
    let twins = "#1 and #4 carry the same label, \"Unclear decision owner\". Merge them if they state the same idea; otherwise label each by what sets it apart.";
    assert_eq!(health::notes(&echo), [twins, aimless]);
    assert!(
        brief::steward(&echo, &none(), &Stands::default())
            .contains(&format!("\nHEALTH\n- {twins}\n"))
    );
    let mut more = echo.clone();
    more.add("One more thing.");
    let (text, _) = brief::place(&more, &none(), &Stands::default()).unwrap();
    assert!(!text.contains("HEALTH"));
}

#[test]
fn the_placer_is_shown_the_map_as_an_outline() {
    // A capture still being split into thoughts is not offered for filing.
    let (text, targets) = brief::place(&fresh(), &BTreeSet::from([6]), &Stands::default()).unwrap();
    assert_eq!(targets, [5]);
    let expected = r#"SUBJECT
Title: (placeholder, no title yet)
Focus: not settled yet
Ordering idea: not settled yet

MAP
[decisions] Decisions
  #1 Unclear decision owner
  #2 Reopened decisions
[meetings] Meetings
  #3 Long meetings
  #4 Silent invitees

RECENT
#1 in decisions: "Nobody knows who decides."
#2 in decisions: "Decisions get reopened every week."
#3 in meetings: "Our meetings run long."
#4 in meetings: "Half the invitees never speak."

NEW
#5: "Agendas arrive late."
"#;
    assert_eq!(text, expected);

    let (text, targets) = brief::place(
        &talk(&["One thing.", "Another."]),
        &none(),
        &Stands::default(),
    )
    .unwrap();
    assert_eq!(targets, [1, 2]);
    let empty =
        "\nMAP\n(empty: its first main branches name parts of the subject, never the subject)\n";
    assert!(text.contains(&format!("{empty}\nNEW\n#1: \"One thing.\"\n")));
    // A turn takes on a dozen thoughts, oldest first; the rest wait for the next.
    let mut long = map();
    for n in 0..13 {
        long.add(&format!("Thought {n}"));
    }
    let (_, targets) = brief::place(&long, &none(), &Stands::default()).unwrap();
    assert_eq!(targets, (5..=16).collect::<Vec<u64>>());

    // The placer hears of strain, sees what could come back, and knows what the user decided.
    let mut session = crowd(8);
    for line in [
        "Pricing can wait.",
        "I must call the dentist.",
        "Budgets too.",
        "Hm.",
    ] {
        session.add(line);
    }
    let ops = [
        park(9, ParkKind::Later, "Pricing", "Deferred by the speaker"),
        park(10, ParkKind::OffTopic, "Dentist", "A personal errand"),
        park(11, ParkKind::Later, "Budgets", "Deferred by the speaker"),
        Op::Title {
            title: "Meeting trouble".into(),
        },
        Op::Focus {
            focus: Some("What is wrong with our meetings".into()),
            principle: Some("Kinds of problem".into()),
        },
    ];
    let session = replay(&session, &ops).unwrap();
    let session = SessionEvent::Held { id: 11 }.reduce(&session).unwrap();
    let session = SessionEvent::Revived { id: 9 }.reduce(&session).unwrap();
    let mut session = SessionEvent::Parked { id: 12 }.reduce(&session).unwrap();
    session.add("Pricing depends on the meeting load.");
    session
        .directions
        .push("Keep risks on their own branch".into());
    let (text, targets) = brief::place(&session, &none(), &Stands::default()).unwrap();
    assert_eq!(targets, [9, 13]);
    let subject = "SUBJECT\nTitle: Meeting trouble\nFocus: What is wrong with our meetings\nOrdering idea: Kinds of problem\n\nGUIDANCE\n- Keep risks on their own branch\n\nMAP\n";
    assert!(text.starts_with(subject));
    assert!(text.contains("\nPARKED\noff_topic topics so far: Dentist, Budgets, Set aside\n"));
    let new = "\nNEW\n#9 (the user wants this on the map: place it): \"Pricing can wait.\"\n#13: \"Pricing depends on the meeting load.\"\n";
    assert!(text.contains(new));
    assert!(text.contains("\nHEALTH\n- 8 thoughts hang from 1 main branch(es)."));
    assert!(!text.contains("no stated focus"));
    let later = replay(
        &session,
        &[park(13, ParkKind::Later, "Pricing", "Needs the price list")],
    );
    let mut later = later.unwrap();
    later.add("Here is the price list.");
    let (text, _) = brief::place(&later, &none(), &Stands::default()).unwrap();
    let line = "\n#13 later · topic \"Pricing\" · said: \"Pricing depends on the meeting load.\"\n";
    assert!(text.contains(line));
}

/// A map the user has had a hand in: a thought pinned, one waiting, three parked and a link.
fn tended() -> Session {
    let mut session = map();
    for line in [
        "Agendas arrive late.",
        "I must call the dentist.",
        "Pricing can wait.",
        "The weather is awful.",
    ] {
        session.add(line);
    }
    let pin = SessionEvent::PlacementCorrected {
        id: 1,
        role: Role::Tension,
        category: Some("decisions".into()),
    };
    let session = pin.reduce(&session).unwrap();
    let session = SessionEvent::Parked { id: 8 }.reduce(&session).unwrap();
    let link = Relationship {
        from: 1,
        to: 3,
        label: "prolongs".into(),
    };
    let ops = [
        Op::Park {
            thought: 6,
            kind: ParkKind::OffTopic,
            topic: "Dentist".into(),
            reason: "A personal errand".into(),
            concept: Some("Dentist call".into()),
        },
        park(7, ParkKind::Later, "Pricing", "Deferred by the speaker"),
        Op::Links { links: vec![link] },
        Op::Focus {
            focus: Some("Why decisions drag".into()),
            principle: Some("Causes of delay".into()),
        },
        Op::Title {
            title: "Slow decisions".into(),
        },
        Op::Narrative {
            narrative: READING.into(),
        },
    ];
    let mut session = replay(&session, &ops).unwrap();
    session
        .directions
        .push("Keep risks on their own branch".into());
    session
}

#[test]
fn the_steward_is_shown_the_words_behind_the_map() {
    let session = tended();
    let expected = r#"SUBJECT
Title: Slow decisions
Focus: Why decisions drag
Ordering idea: Causes of delay
Narrative so far: Nobody owns decisions, so meetings fill the gap.

GUIDANCE
- Keep risks on their own branch

MAP
[decisions] Decisions
  #1 Unclear decision owner · Tension · pinned
     said: "Nobody knows who decides."
  #2 Reopened decisions · Observation
     said: "Decisions get reopened every week."
[meetings] Meetings
  #3 Long meetings · Observation
     said: "Our meetings run long."
  #4 Silent invitees · Observation
     said: "Half the invitees never speak."

UNFILED
#5: "Agendas arrive late."

PARKED
#8 off_topic · topic "Set aside" · The weather is awful. · held by the user: leave it
#6 off_topic · topic "Dentist" · Dentist call
     why: A personal errand
     said: "I must call the dentist."
#7 later · topic "Pricing" · Pricing can wait.
     why: Deferred by the speaker
     said: "Pricing can wait."

LINKS
#1 → #3: prolongs

HEALTH
- 1 parked thought(s) are marked later. Bring back any the map now has a home for.
"#;
    assert_eq!(
        brief::steward(&session, &none(), &Stands::default()),
        expected
    );
}

const ASK: &str = "Set the meetings talk aside for another day and call this Slow decisions.";

#[test]
fn an_instruction_is_carried_out_in_one_call_on_the_users_word() {
    let session = fresh();
    let aside = |id: u64| json!({"id": id, "kind": "later", "reason": "Set aside by the user", "topic": "Meetings"});
    let said = "Both meetings thoughts wait for another day, and the map is now Slow decisions.";
    let answer = json!({
        "about": "Park both meetings thoughts and rename the map.",
        "aim": {"focus": null, "principle": null, "title": "Slow decisions"},
        "branches": {"grow": [], "merge": [], "move": [], "remove": ["meetings"], "rename": []},
        "ideas": {"file": [], "merge": [], "park": [aside(3), aside(4)]},
        "links": null,
        "narrative": null,
        "remember": false,
        "reply": said
    });
    let (turn, heard) = follow(&session, &none(), ASK, vec![Ok(answer)]);
    let turn = turn.unwrap();
    // Two thoughts still wait and no model has read the map: neither is what was asked.
    assert_eq!(heard.len(), 1);
    let opening = heard[0]["messages"].as_array().unwrap();
    assert_eq!(opening.len(), 2);
    assert_eq!(opening[0]["role"], "system");
    assert_eq!(opening[0]["content"], prompt::DIRECTOR);
    assert_eq!(opening[1]["role"], "user");
    assert_eq!(opening[1]["content"], brief::direct(&session, &none(), ASK));
    assert_eq!(heard[0]["budget"], 12_000 + 120 * 6);

    let later = |id| park(id, ParkKind::Later, "Meetings", "Set aside by the user");
    let branch = "meetings".into();
    let title = "Slow decisions".into();
    assert_eq!(
        turn.ops,
        [
            later(3),
            later(4),
            Op::Remove { branch },
            Op::Title { title }
        ]
    );
    assert_eq!(turn.note, said);
    assert_eq!(
        (turn.tier, turn.unmet, turn.remember),
        (Tier::Direct, 0, false)
    );
    assert!(turn.refused.is_empty() && turn.escalate.is_none() && !turn.garbled);

    // Journaled as the user's own: what they set aside stays there until they say otherwise.
    let after = guided(&session, ASK, &turn.ops).unwrap();
    assert_eq!(after.title, "Slow decisions");
    assert_eq!(after.directions, [ASK]);
    assert_eq!(after.parked.len(), 2);
    assert!(
        after
            .parked
            .iter()
            .all(|p| p.held && p.kind == ParkKind::Later)
    );
    // The placer is told the topic, and nothing it could bring back.
    let (text, targets) = brief::place(&after, &none(), &Stands::default()).unwrap();
    assert_eq!(targets, [5, 6]);
    assert!(text.contains("\nPARKED\nlater topics held by the user: Meetings\n\nRECENT\n"));
}

#[test]
fn a_directed_turn_answers_for_what_was_asked_and_no_more() {
    // A crowded branch is the steward's to divide: an instruction is not held up for it.
    let session = crowd(8);
    assert!(health::strained(&session));
    let answer = json!({"aim": {"title": "Meeting trouble"}, "reply": "Renamed the map."});
    let script = vec![Ok(answer)];
    let (turn, heard) = follow(&session, &none(), "Call this Meeting trouble.", script);
    assert_eq!(heard.len(), 1);
    assert!(brief::steward(&session, &none(), &Stands::default()).contains("\nHEALTH\n"));
    assert!(!told(&heard, 0).contains("HEALTH"));
    assert_eq!(kinds(&turn.unwrap().ops), ["title"]);

    // A standing instruction may change nothing today and still be worth keeping.
    let lasting = "From now on, keep every branch name to one or two words.";
    let answer = json!({"remember": true, "reply": "Branch names will stay to two words."});
    let (turn, heard) = follow(&fresh(), &none(), lasting, vec![Ok(answer)]);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 1);
    assert!(turn.remember && turn.ops.is_empty());
    assert_eq!(turn.note, "Branch names will stay to two words.");
    // One the map already satisfies says so, and leaves no trace.
    let ask = "Put the long meetings under Meetings.";
    let answer = json!({"reply": "Long meetings is already under Meetings."});
    let (turn, heard) = follow(&map(), &none(), ask, vec![Ok(answer)]);
    let turn = turn.unwrap();
    assert!(!turn.remember && turn.ops.is_empty() && heard.len() == 1);
    assert_eq!(turn.note, "Long meetings is already under Meetings.");
}

#[test]
fn a_directed_turn_that_divides_the_map_anew_answers_for_a_crowded_branch() {
    // Asked to reorganise, it was asked for the whole method, not for one edit.
    let session = crowd(8);
    let ask = "Reorganise the whole map around what goes wrong in meetings.";
    let anew = json!({"aim": {"principle": "Kinds of meeting trouble"}, "reply": "Reorganised."});
    let file: Vec<Value> = (1..=8)
        .map(|id| {
            let parent = if id <= 4 { "agendas" } else { "attendance" };
            json!({"id": id, "idea": null, "parent": parent, "pin": false, "role": "Keep"})
        })
        .collect();
    let grow = json!([{"id": "agendas", "name": "Agendas", "parent": "meetings"},
        {"id": "attendance", "name": "Attendance", "parent": "meetings"}]);
    let done = "Meetings is now divided into Agendas and Attendance.";
    let split = json!({"branches": {"grow": grow}, "ideas": {"file": file}, "reply": done});
    let (turn, heard) = follow(&session, &none(), ask, vec![Ok(anew.clone()), Ok(split)]);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 2);
    let expected = format!(
        "Checked against the map: 1 edit(s) applied.\n{}\n- {}\n{}",
        "The map as you divided it still strains. Mend it within what the user asked for:",
        "Branch meetings (Meetings) has 8 items directly under it. Group them into two or three sub-branches named for what they share.",
        prompt::REDIRECT
    );
    assert_eq!(told(&heard, 1), expected);
    assert_eq!(kinds(&turn.ops)[..3], ["focus", "branch", "branch"]);
    assert_eq!(turn.note, done);
    let after = guided(&session, ask, &turn.ops).unwrap();
    assert_eq!(health::shape(&after).crowded, 0);
    assert_eq!(health::items(&after, "meetings"), (0, 2));

    // Told once. A branch still crowded after that waits for the steward.
    let shrug = "Reorganised; Meetings stays one list.";
    let script = vec![Ok(anew), Ok(json!({"reply": shrug})), Ok(json!({}))];
    let (turn, heard) = follow(&session, &none(), ask, script);
    assert_eq!(heard.len(), 2);
    assert_eq!(turn.unwrap().note, shrug);
}

#[test]
fn what_the_map_refused_of_an_instruction_earns_one_correction() {
    let ask = "Rename Meetings to Gatherings and the other branch to Ownership.";
    let lost = json!({"branch": "owners", "name": "Ownership"});
    let both = json!({"rename": [lost, {"branch": "meetings", "name": "Gatherings"}]});
    let first = json!({"branches": both, "reply": "Renamed both branches."});
    let found = json!({"rename": [{"branch": "decisions", "name": "Ownership"}]});
    let done = "Meetings is now Gatherings and Decisions is now Ownership.";
    let second = json!({"branches": found, "reply": done});
    let script = vec![Ok(first.clone()), Ok(second.clone())];
    let (turn, heard) = follow(&map(), &none(), ask, script);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 2);
    let again = heard[1]["messages"].as_array().unwrap();
    assert_eq!(again.len(), 4);
    assert_eq!(again[2]["role"], "assistant");
    assert_eq!(again[2]["content"], first.to_string());
    // The correction is asked to account for the whole turn, not only for what it adds.
    let expected = format!(
        "Checked against the map: 1 edit(s) applied.\nRefused:\n- {}: There is no branch owners\n{}",
        r#"{"op":"rename","branch":"owners","name":"Ownership"}"#,
        prompt::REDIRECT
    );
    assert_eq!(told(&heard, 1), expected);
    assert_eq!(kinds(&turn.ops), ["rename", "rename"]);
    assert_eq!(turn.note, done);
    assert_eq!((turn.unmet, turn.refused.len()), (0, 1));
    let after = guided(&map(), ask, &turn.ops).unwrap();
    let names: Vec<&str> = after.categories.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Ownership", "Gatherings"]);

    // Refused again, the user is told what did not fit. The model is not asked a third time.
    let sorry = "Meetings is now Gatherings; I found no second branch to rename.";
    let still = json!({"branches": {"rename": [lost]}, "reply": sorry});
    let script = vec![Ok(first.clone()), Ok(still.clone()), Ok(still)];
    let (turn, heard) = follow(&map(), &none(), ask, script);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 2);
    assert_eq!(kinds(&turn.ops), ["rename"]);
    // Its last word stands for the turn even when it changed nothing more.
    assert_eq!(turn.note, sorry);
    assert_eq!((turn.unmet, turn.refused.len()), (1, 2));

    // A correction that never arrives leaves the first answer's word, and what it left undone.
    let script = vec![
        Ok(first.clone()),
        Err("The map agent timed out; your map is safe"),
    ];
    let (turn, heard) = follow(&map(), &none(), ask, script);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 2);
    assert_eq!(turn.note, "Renamed both branches.");
    assert_eq!((turn.unmet, turn.refused.len()), (1, 1));

    // An answer that could not be read does not use up the correction.
    let script = vec![Ok(json!("Certainly!")), Ok(first), Ok(second)];
    let (turn, heard) = follow(&map(), &none(), ask, script);
    let turn = turn.unwrap();
    assert!(turn.garbled && heard.len() == 3 && heard[0] == heard[1]);
    assert_eq!(
        (turn.ops.len(), turn.unmet, turn.note.as_str()),
        (2, 0, done)
    );
}

#[test]
fn an_instruction_that_cannot_be_carried_out_says_why() {
    let failed = |session: &Session, instruction: &str, script: Vec<Answer>| {
        let (turn, heard) = follow(session, &none(), instruction, script);
        (turn.unwrap_err(), heard.len())
    };
    let ask = "Call this Slow decisions.";
    // No model is asked without something to ask it.
    let blank = "Guidance must be present and under 8000 characters";
    assert_eq!(failed(&map(), " \n ", vec![]), (blank.into(), 0));
    assert_eq!(failed(&map(), &"x".repeat(8001), vec![]), (blank.into(), 0));
    let (turn, heard) = drive(Tier::Direct, &map(), &none(), vec![]);
    assert_eq!((turn.unwrap_err(), heard.len()), (blank.into(), 0));
    let mut vast = Session::default();
    for n in 0..401 {
        vast.add(&format!("Thought {n}"));
    }
    let limit = "Map limit reached (400 thoughts); editing by hand remains available";
    assert_eq!(failed(&vast, ask, vec![]), (limit.into(), 0));

    let down = "The map agent is unavailable; check key, credits and model availability";
    assert_eq!(failed(&map(), ask, vec![Err(down)]), (down.into(), 1));
    let unreadable = || Ok(json!(["not", "an", "answer"]));
    assert_eq!(
        failed(&map(), ask, vec![unreadable(), unreadable()]),
        ("The map agent returned an unreadable answer".into(), 2)
    );
    let named = json!({"aim": {"title": "Slow decisions"}, "reply": "Renamed the map."});
    let (turn, heard) = follow(&map(), &none(), ask, vec![unreadable(), Ok(named)]);
    let turn = turn.unwrap();
    assert!(turn.garbled && heard.len() == 2 && heard[0] == heard[1]);
    assert_eq!(kinds(&turn.ops), ["title"]);
    // The instruction reaches the model as the user wrote it, less the space around it.
    assert!(told(&heard, 0).ends_with("\nINSTRUCTION\nCall this Slow decisions.\n"));
    let padded = "\n  Call this Slow decisions.  ";
    let (_, spaced) = follow(&map(), &none(), padded, vec![Err(down)]);
    assert_eq!(told(&spaced, 0), told(&heard, 0));
}

#[test]
fn a_directed_turn_is_shown_what_the_user_decided_and_what_they_ask() {
    let mut session = tended();
    session.add("Lunch was cold.");
    let aside = park(9, ParkKind::Aside, "Lunch", "Small talk");
    let session = replay(&session, &[aside]).unwrap();
    // The user brought two thoughts back; the agent has found a branch for one of them.
    let session = SessionEvent::Revived { id: 6 }.reduce(&session).unwrap();
    let session = SessionEvent::Revived { id: 7 }.reduce(&session).unwrap();
    let back = Op::Place {
        thought: 7,
        branch: "decisions".into(),
        concept: Some("Pricing decision".into()),
        role: Some(Role::Question),
    };
    let session = replay(&session, &[back]).unwrap();
    let ask = "Put the dentist call under Meetings.";
    // No checklist: the instruction decides what changes. What the user holds is theirs to
    // overrule, so its reason and its words are shown.
    let expected = r#"SUBJECT
Title: Slow decisions
Focus: Why decisions drag
Ordering idea: Causes of delay
Narrative so far: Nobody owns decisions, so meetings fill the gap.

GUIDANCE
- Keep risks on their own branch

MAP
[decisions] Decisions
  #1 Unclear decision owner · Tension · pinned
     said: "Nobody knows who decides."
  #2 Reopened decisions · Observation
     said: "Decisions get reopened every week."
  #7 Pricing decision · Question · kept
     said: "Pricing can wait."
[meetings] Meetings
  #3 Long meetings · Observation
     said: "Our meetings run long."
  #4 Silent invitees · Observation
     said: "Half the invitees never speak."

UNFILED
#5: "Agendas arrive late."
#6 · kept: "I must call the dentist."

PARKED
#8 off_topic · topic "Set aside" · The weather is awful. · held by the user
     why: Parked by you
     said: "The weather is awful."
#9 aside · topic "Lunch" · Lunch was cold.
     why: Small talk
     said: "Lunch was cold."

LINKS
#1 → #3: prolongs

INSTRUCTION
Put the dentist call under Meetings.
"#;
    assert_eq!(brief::direct(&session, &none(), ask), expected);
    // A capture still being split is nobody's to file yet.
    let held = brief::direct(&session, &BTreeSet::from([5]), ask);
    assert!(held.contains("\nUNFILED\n#6 · kept: \"I must call the dentist.\"\n"));

    // The agent stops at seven main branches. The user may ask for an eighth, and no more.
    let mut wide = Session::default();
    let mut ops = vec![];
    for n in 1..=7 {
        let id = format!("part-{n}");
        wide.add(&format!("Thought {n}."));
        ops.push(branch(&id, &format!("Part {n}"), None));
        ops.push(place(n, &id, &format!("Idea {n}")));
    }
    let wide = replay(&wide, &ops).unwrap();
    let ask = "Add a main branch for risks.";
    let full = "\nMAP (7 main branches: full, it takes no more)\n";
    assert!(brief::steward(&wide, &none(), &Stands::default()).contains(full));
    assert!(brief::direct(&wide, &none(), ask).contains("\nMAP\n[part-1] Part 1\n"));
    let eighth = guided(&wide, ask, &[branch("risks", "Risks", None)]).unwrap();
    let full = "\nMAP (8 main branches: full, it takes no more)\n";
    assert!(brief::direct(&eighth, &none(), ask).contains(full));
}
