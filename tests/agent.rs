use std::collections::BTreeSet;
use tacit::agent::turn::Stands;
use tacit::agent::{MAX_GUIDANCE, MAX_INSTRUCTION, Op, Tier, apply, brief, guided, health, replay};
use tacit::model::{Category, ParkKind, Relationship, Role, Session, Thought};
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
        concept: (!concept.is_empty()).then(|| concept.into()),
        role: None,
    }
}
fn park(thought: u64, kind: ParkKind) -> Op {
    Op::Park {
        thought,
        kind,
        topic: "Dentist".into(),
        reason: "A different subject".into(),
        concept: None,
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
fn thought(session: &Session, id: u64) -> &Thought {
    session.thoughts.iter().find(|t| t.id == id).unwrap()
}
/// The reason the map gives for refusing one edit.
fn refused(session: &Session, tier: Tier, op: Op) -> String {
    let out = apply(session, &[op], tier, &none());
    assert!(out.ops.is_empty());
    assert_eq!(out.session, *session);
    out.rejected[0].reason.clone()
}

#[test]
fn a_turn_files_parks_and_replays_exactly() {
    let session = talk(&[
        "Our meetings run long.",
        "Can you hear me okay?",
        "Nobody owns the agenda.",
        "I still need to book the dentist.",
    ]);
    let ops = vec![
        branch("meetings", "Meetings", None),
        place(1, "meetings", "Long meetings"),
        Op::Park {
            thought: 2,
            kind: ParkKind::Aside,
            topic: "Sound check".into(),
            reason: "Said to the room".into(),
            concept: Some("Sound check".into()),
        },
        place(3, "meetings", "Unowned agenda"),
        park(4, ParkKind::OffTopic),
    ];
    let out = apply(&session, &ops, Tier::Place, &none());
    assert!(out.rejected.is_empty());
    assert_eq!(out.ops, ops);
    assert_eq!(out.session.thoughts.len(), 2);
    // Parked words are kept whole, beside the map.
    let parked: Vec<_> = out.session.parked.iter().map(|p| &p.thought.text).collect();
    assert_eq!(
        parked,
        ["Can you hear me okay?", "I still need to book the dentist."]
    );
    // The journal replays the same edits with no model in the loop.
    let event = SessionEvent::Mapped {
        ops: out.ops.clone(),
        pass: "place".into(),
        note: String::new(),
    };
    assert_eq!(event.reduce(&session).unwrap(), out.session);
    let json = serde_json::to_string(&event).unwrap();
    assert_eq!(serde_json::from_str::<SessionEvent>(&json).unwrap(), event);
}

#[test]
fn one_bad_edit_costs_only_itself_but_replay_is_strict() {
    let session = talk(&["Rent went up.", "Travel is over budget."]);
    let ops = vec![
        branch("costs", "Costs", None),
        place(1, "nowhere", "Rising rent"),
        place(2, "costs", "Travel overspend"),
    ];
    let out = apply(&session, &ops, Tier::Place, &none());
    assert_eq!(out.ops, [ops[0].clone(), ops[2].clone()]);
    assert_eq!(out.rejected.len(), 1);
    assert_eq!(out.rejected[0].reason, "There is no branch nowhere");
    assert!(replay(&session, &ops).is_err());
    assert_eq!(replay(&session, &out.ops).unwrap(), out.session);
}

#[test]
fn a_turn_briefed_on_an_older_map_is_rebased_onto_the_current_one() {
    let before = talk(&["Rent went up."]);
    let ops = [
        branch("costs", "Costs", None),
        place(1, "costs", "Rising rent"),
    ];
    // While the model was thinking, the speaker said more and another turn grew the branch.
    let mut now = replay(&before, &[branch("costs", "Costs", None)]).unwrap();
    now.add("Travel is over budget.");
    let out = apply(&now, &ops, Tier::Place, &none());
    assert_eq!(out.ops, [ops[1].clone()]);
    assert!(out.rejected.is_empty());
    assert_eq!(thought(&out.session, 1).category.as_deref(), Some("costs"));
    assert_eq!(thought(&out.session, 2).category, None);
}

#[test]
fn an_edit_waits_for_the_edit_it_depends_on() {
    let ops = [
        Op::Remove {
            branch: "meetings".into(),
        },
        place(3, "decisions", ""),
        place(4, "decisions", ""),
    ];
    let out = apply(&map(), &ops, Tier::Steward, &none());
    assert!(out.rejected.is_empty());
    assert_eq!(out.ops, [ops[1].clone(), ops[2].clone(), ops[0].clone()]);
    assert_eq!(out.session.categories.len(), 1);
}

#[test]
fn a_branch_already_folded_away_needs_no_removing() {
    let fold = Op::Merge {
        from: "meetings".into(),
        into: "decisions".into(),
    };
    let remove = |branch: &str| Op::Remove {
        branch: branch.into(),
    };
    // Belt and braces from a model is no fault: the branch is gone, which is what it asked.
    let ops = [fold, remove("meetings")];
    for tier in [Tier::Steward, Tier::Direct] {
        let out = apply(&map(), &ops, tier, &none());
        assert!(out.rejected.is_empty());
        assert_eq!(out.ops, ops[..1]);
    }
    // A branch that was never there is still a mistake to hear about.
    assert_eq!(
        refused(&map(), Tier::Steward, remove("tools")),
        "There is no branch tools"
    );
    assert!(replay(&map(), &ops).is_err());
}

#[test]
fn removing_a_branch_takes_its_empty_sub_branches_with_it() {
    let remove = |branch: &str| Op::Remove {
        branch: branch.into(),
    };
    // Meetings gets a part with a part of its own, and its thoughts move to the deepest.
    let nest = [
        branch("length", "Length", Some("meetings")),
        branch("overrun", "Overrun", Some("length")),
        place(3, "overrun", ""),
        place(4, "overrun", ""),
    ];
    let nested = replay(&map(), &nest).unwrap();
    // A thought anywhere beneath it keeps a branch standing, and the reason says how many.
    assert_eq!(
        refused(&nested, Tier::Direct, remove("meetings")),
        "Branch meetings still holds 2 thought(s), its sub-branches counted; merge it or move them first"
    );
    // Once they have moved on, all of it goes on one word: what is left is scaffolding.
    let ops = [
        place(3, "decisions", ""),
        place(4, "decisions", ""),
        remove("meetings"),
        remove("overrun"),
    ];
    for tier in [Tier::Steward, Tier::Direct] {
        let out = apply(&nested, &ops, tier, &none());
        assert!(out.rejected.is_empty());
        assert_eq!(out.ops, ops[..3]);
        let left: Vec<_> = out.session.categories.iter().map(|c| &c.id).collect();
        assert_eq!(left, ["decisions"]);
        assert_eq!(replay(&nested, &out.ops).unwrap(), out.session);
    }
}

#[test]
fn a_branch_grown_and_left_bare_is_dropped() {
    let ops = [
        branch("tools", "Tools", None),
        place(99, "tools", "Ghost idea"),
    ];
    let out = apply(&map(), &ops, Tier::Steward, &none());
    assert!(out.ops.is_empty());
    assert_eq!(out.session, map());
    assert_eq!(out.rejected.len(), 1);
}

#[test]
fn a_branch_grown_used_and_taken_down_again_in_one_turn_costs_no_edit() {
    let remove = |branch: &str| Op::Remove {
        branch: branch.into(),
    };
    // A second answer often dismantles what the first one built.
    let ops = [
        branch("length", "Length", Some("meetings")),
        place(3, "length", ""),
        place(3, "decisions", ""),
        remove("length"),
    ];
    let out = apply(&map(), &ops, Tier::Steward, &none());
    assert!(out.rejected.is_empty());
    assert_eq!(out.ops, ops);
    assert_eq!(
        thought(&out.session, 3).category.as_deref(),
        Some("decisions")
    );
    assert_eq!(out.session.categories.len(), 2);
    assert_eq!(replay(&map(), &out.ops).unwrap(), out.session);
}

#[test]
fn a_map_keeps_keyword_branches_and_at_most_seven_main_ones() {
    let mut session = map();
    for n in 0..5 {
        let op = branch(&format!("part-{n}"), &format!("Part {n}"), None);
        session = replay(&session, &[op]).unwrap();
    }
    assert_eq!(
        replay(&session, &[branch("part-8", "Part 8", None)]).unwrap_err(),
        "A map keeps at most 7 main branches; merge two or nest this one"
    );
    assert!(replay(&session, &[branch("part-8", "Part 8", Some("meetings"))]).is_ok());
    // Regrouping a full map passes through a wider one: the new main branch arrives before
    // the old ones nest beneath it.
    let nest = |branch: &str| Op::Move {
        branch: branch.into(),
        parent: Some("people".into()),
    };
    let regroup = [
        branch("people", "People", None),
        nest("decisions"),
        nest("meetings"),
    ];
    let out = apply(&session, &regroup, Tier::Steward, &none());
    assert!(out.rejected.is_empty());
    assert_eq!(health::shape(&out.session).roots, 6);
    assert_eq!(replay(&session, &out.ops).unwrap(), out.session);
    // An eighth that would stay is refused, and so is whatever was to hang from it.
    session.add("Hiring has stalled.");
    let eighth = [
        branch("hiring", "Hiring", None),
        place(5, "hiring", "Stalled hiring"),
    ];
    let out = apply(&session, &eighth, Tier::Place, &none());
    let reasons: Vec<_> = out.rejected.iter().map(|r| r.reason.as_str()).collect();
    assert_eq!(out.session, session);
    assert_eq!(
        reasons,
        [
            "A map keeps at most 7 main branches; file the thought in the closest branch and tell the steward",
            "There is no branch hiring"
        ]
    );
    // A map a person made wider can be narrowed, though not widened again.
    let mut wider = session.clone();
    wider.categories.push(Category {
        id: "part-9".into(),
        name: "Part 9".into(),
        parent: None,
        illustration: None,
        motif: None,
    });
    let fold = Op::Merge {
        from: "part-9".into(),
        into: "part-0".into(),
    };
    assert!(replay(&wider, &[branch("part-10", "Part 10", None)]).is_err());
    assert_eq!(replay(&wider, &[fold]).unwrap(), session);
    for (id, name) in [
        ("Team Trust", "Trust"),
        ("trust", "What people said about trusting each other"),
        ("trust", "decisions"),
    ] {
        assert!(replay(&map(), &[branch(id, name, None)]).is_err());
    }
}

#[test]
fn the_placer_works_under_a_narrow_licence() {
    let mut session = map();
    session.title = "Meeting overload".into();
    session.add("We could try a standing check-in.");
    let no = |op| refused(&session, Tier::Place, op);
    let rename = Op::Rename {
        branch: "meetings".into(),
        name: "Gatherings".into(),
    };
    let title = Op::Title {
        title: "Something else".into(),
    };
    assert_eq!(no(rename), "Only the steward restructures the map");
    assert_eq!(no(title), "Only the steward restructures the map");
    let stay = "The placer re-files a thought only into a main branch it has just grown, or a new sub-branch of the thought's own main branch";
    assert_eq!(no(place(1, "meetings", "")), stay);
    assert_eq!(
        no(park(1, ParkKind::OffTopic)),
        "The placer parks only thoughts that are not filed yet"
    );
    // A crowded branch can be divided: thoughts follow into a branch grown this turn.
    let ops = [
        branch("rituals", "Rituals", Some("meetings")),
        place(3, "rituals", ""),
        place(5, "rituals", "Possible standing check-in"),
    ];
    assert_eq!(apply(&session, &ops, Tier::Place, &none()).ops.len(), 3);
    // Thoughts from another main branch do not: regrouping across the map is the steward's.
    let across = [ops[0].clone(), place(1, "rituals", ""), ops[2].clone()];
    let out = apply(&session, &across, Tier::Place, &none());
    assert_eq!((out.ops.len(), out.rejected[0].reason.as_str()), (2, stay));
    // A new main branch takes thoughts from anywhere, and a branch that empties into it goes.
    let ops = [
        branch("ownership", "Ownership", None),
        place(1, "ownership", ""),
        place(2, "ownership", ""),
        place(5, "ownership", "Possible standing check-in"),
    ];
    let out = apply(&session, &ops, Tier::Place, &none());
    let gone = Op::Remove {
        branch: "decisions".into(),
    };
    assert_eq!((out.ops.len(), out.ops.last()), (5, Some(&gone)));
    assert_eq!(replay(&session, &out.ops).unwrap(), out.session);
    // Rebased, that housekeeping is worked out afresh, not refused as a restructuring.
    let again = apply(&session, &out.ops, Tier::Place, &none());
    assert!(again.ops == out.ops && again.rejected.is_empty());
    // A "not yet" thought comes back when there is a home for it; an off-topic one does not.
    let back = [place(5, "meetings", "Possible standing check-in")];
    let later = replay(&session, &[park(5, ParkKind::Later)]).unwrap();
    let out = apply(&later, &back, Tier::Place, &none());
    assert!(out.session.parked.is_empty() && out.ops.len() == 1);
    let off = replay(&session, &[park(5, ParkKind::OffTopic)]).unwrap();
    assert!(apply(&off, &back, Tier::Place, &none()).ops.is_empty());
    assert_eq!(apply(&off, &back, Tier::Steward, &none()).ops.len(), 1);
}

#[test]
fn what_the_user_pinned_stays_where_they_put_it() {
    let pin = SessionEvent::PlacementCorrected {
        id: 1,
        role: Role::Tension,
        category: Some("decisions".into()),
    };
    let session = pin.reduce(&map()).unwrap();
    let no = |op| refused(&session, Tier::Steward, op);
    assert_eq!(
        no(place(1, "meetings", "")),
        "The user pinned thought 1 where it is; do not move it"
    );
    assert_eq!(
        no(park(1, ParkKind::OffTopic)),
        "The user wants thought 1 on the map; place it instead"
    );
    let combine = Op::Combine {
        into: 2,
        from: vec![1],
        concept: "One idea".into(),
    };
    assert_eq!(no(combine), "The user pinned thought 1; do not combine it");
    let fixed =
        "Branch decisions holds a thought the user pinned; leave its name and position alone";
    let rename = Op::Rename {
        branch: "decisions".into(),
        name: "Choices".into(),
    };
    let merge = Op::Merge {
        from: "decisions".into(),
        into: "meetings".into(),
    };
    assert_eq!(no(rename), fixed);
    assert_eq!(no(merge), fixed);
    // A sharper label is welcome; the role the user chose is not the agent's to change.
    let relabel = Op::Place {
        thought: 1,
        branch: "decisions".into(),
        concept: Some("Missing decision owner".into()),
        role: Some(Role::Question),
    };
    let out = apply(&session, &[relabel], Tier::Steward, &none());
    let t = thought(&out.session, 1);
    assert_eq!(t.concept.as_deref(), Some("Missing decision owner"));
    assert_eq!(t.role, Role::Tension);
}

#[test]
fn the_users_parking_decisions_bind_the_agent() {
    let mut session = map();
    session.add("I should call the dentist.");
    let home = [place(5, "meetings", "Dentist call")];
    // Set aside by the user: off topic, and the agent may not bring it back or re-file it.
    let parked = SessionEvent::Parked { id: 5 }.reduce(&session).unwrap();
    let p = &parked.parked[0];
    assert!(p.held && p.kind == ParkKind::OffTopic);
    assert_eq!(p.thought.text, "I should call the dentist.");
    assert_eq!(
        refused(&parked, Tier::Steward, home[0].clone()),
        "The user keeps thought 5 parked"
    );
    assert_eq!(
        refused(&parked, Tier::Steward, park(5, ParkKind::Later)),
        "The user decided thought 5 stays parked"
    );
    // Brought back by the user: it waits for a branch, and the agent may not park it again.
    let back = SessionEvent::Revived { id: 5 }.reduce(&parked).unwrap();
    assert!(back.parked.is_empty());
    assert!(thought(&back, 5).kept && thought(&back, 5).category.is_none());
    assert_eq!(
        refused(&back, Tier::Place, park(5, ParkKind::OffTopic)),
        "The user wants thought 5 on the map; place it instead"
    );
    let out = apply(&back, &home, Tier::Place, &none());
    assert_eq!(
        thought(&out.session, 5).category.as_deref(),
        Some("meetings")
    );
    // "Keep it parked" turns the agent's "not yet" into the user's "off topic".
    let later = replay(&session, &[park(5, ParkKind::Later)]).unwrap();
    let held = SessionEvent::Held { id: 5 }.reduce(&later).unwrap();
    assert!(held.parked[0].held && held.parked[0].kind == ParkKind::OffTopic);
    assert!(apply(&held, &home, Tier::Steward, &none()).ops.is_empty());
    // Removing a parked thought is the one way its place in the lot ends.
    let gone = SessionEvent::Removed { id: 5 }.reduce(&held).unwrap();
    assert!(gone.parked.is_empty() && gone.thoughts.len() == 4);
}

#[test]
fn a_capture_still_being_split_is_left_alone() {
    let mut session = map();
    session.add("A long ramble that will become several thoughts.");
    let holds = BTreeSet::from([5]);
    for tier in [Tier::Place, Tier::Steward, Tier::Direct] {
        for op in [place(5, "meetings", "Ramble"), park(5, ParkKind::Aside)] {
            let out = apply(&session, &[op], tier, &holds);
            assert!(out.ops.is_empty());
            assert_eq!(
                out.rejected[0].reason,
                "That capture is still being split into thoughts; leave it alone"
            );
        }
    }
}

#[test]
fn repeats_become_one_idea_that_keeps_everyones_words() {
    let mut session = map();
    session.add("Honestly nobody can say who makes the call.");
    let link = |from, to| Relationship {
        from,
        to,
        label: "feeds".into(),
    };
    let ops = [
        place(5, "decisions", "Unclear decision owner"),
        Op::Links {
            links: vec![link(5, 3), link(1, 5)],
        },
        Op::Combine {
            into: 1,
            from: vec![5],
            concept: "No clear decision owner".into(),
        },
    ];
    let out = apply(&session, &ops, Tier::Steward, &none());
    assert!(out.rejected.is_empty());
    let t = thought(&out.session, 1);
    assert_eq!(
        t.text,
        "Nobody knows who decides.\n\nHonestly nobody can say who makes the call."
    );
    assert_eq!(t.evidence, ["thought-1", "thought-5"]);
    assert_eq!(t.concept.as_deref(), Some("No clear decision owner"));
    // The repeat leaves the map for the history, with its own words and where they went.
    assert_eq!(out.session.thoughts.len(), 4);
    let retired = &out.session.retired[0];
    assert_eq!((retired.thought.id, &retired.replaced_by), (5, &vec![1]));
    assert_eq!(
        out.session.sources["thought-5"],
        "Honestly nobody can say who makes the call."
    );
    // Links follow the idea, and a link between the two halves of one idea goes.
    assert_eq!(out.session.charter.unwrap().links, [link(1, 3)]);
    let alone = Op::Combine {
        into: 1,
        from: vec![1],
        concept: "One idea".into(),
    };
    assert_eq!(
        refused(&session, Tier::Steward, alone),
        "Combine needs two or more different thoughts"
    );
}

#[test]
fn the_reading_appears_once_every_thought_has_a_home() {
    let words = "Decisions have no owner, so meetings fill the gap.";
    let narrative = Op::Narrative {
        narrative: words.into(),
    };
    let session = replay(&map(), &[narrative]).unwrap();
    let reading = session.interpretation.as_ref().unwrap();
    assert_eq!(reading.narrative, words);
    assert_eq!(reading.assignments.len(), 4);
    // A new capture is not yet part of the reading, but the reader keeps the last one.
    let capture = SessionEvent::capture(&session, "We could rotate the chair.");
    let more = capture.reduce(&session).unwrap();
    assert!(more.interpretation.is_none());
    assert_eq!(more.narrative(), words);
    // Filing it, or parking it, makes the map whole again.
    let filed = [place(5, "meetings", "Possible rotating chair")];
    for ops in [&filed[..], &[park(5, ParkKind::Later)][..]] {
        let out = apply(&more, ops, Tier::Place, &none());
        assert!(out.session.interpretation.is_some());
    }
    // So does anything the user does by hand.
    let gone = SessionEvent::Removed { id: 5 }.reduce(&more).unwrap();
    assert_eq!(gone.interpretation.unwrap().assignments.len(), 4);
}

#[test]
fn sessions_the_agent_never_touched_are_stored_as_before() {
    let json = serde_json::to_value(Session::demo()).unwrap();
    assert!(json.get("parked").is_none() && json.get("charter").is_none());
    assert!(json["thoughts"][0].get("kept").is_none());
    let old = r#"{"version":1,"title":"Old","next_id":2,
        "thoughts":[{"id":1,"text":"Hello","role":"Observation","pinned":false}]}"#;
    let session: Session = serde_json::from_str(old).unwrap();
    assert!(session.parked.is_empty() && session.charter.is_none());
    session.validate().unwrap();
    // A map the agent tends round-trips whole.
    let mut tended = map();
    tended.add("I should call the dentist.");
    let tended = replay(&tended, &[park(5, ParkKind::OffTopic)]).unwrap();
    let json = serde_json::to_string(&tended).unwrap();
    assert_eq!(serde_json::from_str::<Session>(&json).unwrap(), tended);
}

#[test]
fn an_instruction_carries_the_users_own_authority() {
    let pin = SessionEvent::PlacementCorrected {
        id: 1,
        role: Role::Tension,
        category: Some("decisions".into()),
    };
    let mut session = pin.reduce(&map()).unwrap();
    session.add("I should call the dentist.");
    let session = SessionEvent::Parked { id: 5 }.reduce(&session).unwrap();
    // What binds the agent's own turns is the user's to change by saying so.
    let ops = [
        Op::Rename {
            branch: "decisions".into(),
            name: "Choices".into(),
        },
        place(1, "meetings", ""),
        place(5, "meetings", "Dentist call"),
        park(3, ParkKind::Later),
    ];
    for op in &ops[..3] {
        assert!(!refused(&session, Tier::Steward, op.clone()).is_empty());
    }
    let out = apply(&session, &ops, Tier::Direct, &none());
    assert!(out.rejected.is_empty());
    assert_eq!(out.ops, ops);
    assert_eq!(out.session.categories[0].name, "Choices");
    let moved = thought(&out.session, 1);
    assert!(moved.pinned && moved.category.as_deref() == Some("meetings"));
    // Brought back on their word, a thought is kept: the agent does not park it again.
    assert!(thought(&out.session, 5).kept);
    assert_eq!(
        refused(&out.session, Tier::Steward, park(5, ParkKind::Later)),
        "The user wants thought 5 on the map; place it instead"
    );
    // Parked on their word, it is held: the agent does not bring it back.
    let p = &out.session.parked[0];
    assert_eq!((p.thought.id, p.kind, p.held), (3, ParkKind::Later, true));
    for tier in [Tier::Place, Tier::Steward] {
        assert_eq!(
            refused(&out.session, tier, place(3, "meetings", "")),
            "The user keeps thought 3 parked"
        );
    }
    // They may change their mind about why it waits.
    let off = [park(3, ParkKind::OffTopic)];
    let off = apply(&out.session, &off, Tier::Direct, &none());
    let p = &off.session.parked[0];
    assert_eq!((p.kind, p.held), (ParkKind::OffTopic, true));
    // The agent's own "not yet" becomes theirs when they confirm it.
    let later = replay(&map(), &[park(4, ParkKind::Later)]).unwrap();
    assert!(!later.parked[0].held);
    let kept = apply(&later, &[park(4, ParkKind::Later)], Tier::Direct, &none());
    assert!(kept.session.parked[0].held && kept.ops.len() == 1);
    // Even what they pinned folds into another idea when they say it is the same one.
    let combine = Op::Combine {
        into: 2,
        from: vec![1],
        concept: "One idea".into(),
    };
    assert_eq!(
        refused(&session, Tier::Steward, combine.clone()),
        "The user pinned thought 1; do not combine it"
    );
    let one = apply(&session, &[combine], Tier::Direct, &none());
    assert!(one.rejected.is_empty() && one.session.thoughts.len() == 3);
    // Journaled as guidance, the edits replay with the same authority and no model.
    let event = SessionEvent::Guided {
        instruction: "Tidy as I said".into(),
        ops: out.ops.clone(),
        note: String::new(),
    };
    let mut journaled = event.reduce(&session).unwrap();
    assert_eq!(journaled.directions, ["Tidy as I said"]);
    journaled.directions.clear();
    assert_eq!(journaled, out.session);
    // The same edits without that authority are no journal entry.
    assert!(replay(&session, &out.ops).is_err());
    let mapped = SessionEvent::Mapped {
        ops: out.ops,
        pass: "steward".into(),
        note: String::new(),
    };
    assert!(mapped.reduce(&session).is_err());
}

#[test]
fn only_the_user_pins_and_only_where_the_thought_already_is() {
    let session = map();
    let pin = |branch: &str| Op::Pin {
        thought: 3,
        branch: branch.into(),
    };
    assert_eq!(
        refused(&session, Tier::Steward, pin("meetings")),
        "Only the user pins a thought to a branch"
    );
    assert_eq!(
        refused(&session, Tier::Place, pin("meetings")),
        "Only the steward restructures the map"
    );
    assert_eq!(
        refused(&session, Tier::Direct, pin("decisions")),
        "Thought 3 is not in branch decisions"
    );
    // Moved and pinned in one breath: the pin waits for the move it depends on.
    let ops = [pin("decisions"), place(3, "decisions", "")];
    let out = apply(&session, &ops, Tier::Direct, &none());
    assert!(out.rejected.is_empty());
    assert_eq!(out.ops, [ops[1].clone(), ops[0].clone()]);
    let t = thought(&out.session, 3);
    assert!(t.pinned && t.category.as_deref() == Some("decisions"));
    assert_eq!(
        guided(&session, "Pin it", &out.ops).unwrap().thoughts,
        out.session.thoughts
    );
    // From then on it is where the agent must leave it.
    assert_eq!(
        refused(&out.session, Tier::Steward, place(3, "meetings", "")),
        "The user pinned thought 3 where it is; do not move it"
    );
    // Pinning what is already pinned there changes nothing, and is no refusal either.
    let again = apply(&out.session, &[pin("decisions")], Tier::Direct, &none());
    assert!(again.ops.is_empty() && again.rejected.is_empty());
}

#[test]
fn the_user_may_grow_an_eighth_main_branch_and_an_empty_one() {
    let mut session = map();
    for n in 0..5 {
        let op = branch(&format!("part-{n}"), &format!("Part {n}"), None);
        session = replay(&session, &[op]).unwrap();
    }
    // A branch the user asked for stands though nothing hangs from it yet.
    let eighth = [branch("quick-wins", "Quick wins", None)];
    assert!(
        apply(&session, &eighth, Tier::Steward, &none())
            .ops
            .is_empty()
    );
    let out = apply(&session, &eighth, Tier::Direct, &none());
    assert_eq!((out.ops.len(), health::shape(&out.session).roots), (1, 8));
    let journaled = guided(&session, "Add a Quick wins branch", &eighth).unwrap();
    assert_eq!(journaled.categories, out.session.categories);
    assert!(replay(&session, &eighth).is_err());
    // The agent works on in the wider map without widening it, and may narrow it.
    let fold = Op::Merge {
        from: "part-4".into(),
        into: "part-3".into(),
    };
    let narrowed = apply(&out.session, &[fold], Tier::Steward, &none());
    assert_eq!(health::shape(&narrowed.session).roots, 7);
    // Eight is as wide as a map is left, whoever asks.
    let ninth = branch("later", "Later", None);
    assert_eq!(
        refused(&out.session, Tier::Direct, ninth.clone()),
        "A map keeps at most 8 main branches; merge two or nest this one"
    );
    assert_eq!(
        guided(&out.session, "One more", &[ninth]).unwrap_err(),
        "A map keeps at most 8 main branches; merge two or nest this one"
    );
}

#[test]
fn a_regrouping_passes_through_a_map_wider_than_it_leaves() {
    // Eight main branches, each with a thought: as wide as a map gets.
    let mut session = Session::default();
    let mut ops = vec![];
    for n in 1..=8 {
        session.add(&format!("Thought {n}."));
        ops.push(branch(&format!("part-{n}"), &format!("Part {n}"), None));
        ops.push(place(n, &format!("part-{n}"), &format!("Idea {n}")));
    }
    let session = guided(&session, "One branch each.", &ops).unwrap();
    let fold = |from: u64, into: &str| Op::Merge {
        from: format!("part-{from}"),
        into: into.into(),
    };
    // The new main branches arrive first and the old ones fold into them: eleven on the way,
    // three at the end.
    let stages = ["early", "middle", "late"];
    let mut regroup: Vec<Op> = stages
        .iter()
        .map(|id| branch(id, &format!("{id} stage"), None))
        .collect();
    regroup.extend((1..=8).map(|n| fold(n, stages[(n as usize - 1) / 3])));
    for tier in [Tier::Direct, Tier::Steward] {
        let out = apply(&session, &regroup, tier, &none());
        assert!(out.rejected.is_empty());
        assert_eq!(out.ops, regroup);
        let shape = health::shape(&out.session);
        assert_eq!((shape.roots, shape.branches, shape.filed), (3, 3, 8));
        assert_eq!(thought(&out.session, 8).category.as_deref(), Some("late"));
        assert_eq!(replay(&session, &out.ops).unwrap(), out.session);
    }
    // One that would end wider loses its newest main branches, and what was to go into them
    // stays where it was.
    let half = [
        regroup[..3].to_vec(),
        vec![fold(1, "early"), fold(8, "late")],
    ]
    .concat();
    let out = apply(&session, &half, Tier::Direct, &none());
    let reasons: Vec<_> = out.rejected.iter().map(|r| r.reason.as_str()).collect();
    assert_eq!(
        reasons,
        [
            "A map keeps at most 8 main branches; merge two or nest this one",
            "A map keeps at most 8 main branches; merge two or nest this one",
            "There is no branch late"
        ]
    );
    assert_eq!(out.ops, [half[0].clone(), half[3].clone()]);
    assert_eq!(health::shape(&out.session).roots, 8);
    assert_eq!(thought(&out.session, 8).category.as_deref(), Some("part-8"));
    assert_eq!(
        guided(&session, "Stages.", &out.ops).unwrap().categories,
        out.session.categories
    );
    assert!(guided(&session, "Stages.", &half).is_err());
    // The agent's bare branch goes without a word, which leaves the map as wide as it was
    // found: no wider is all that is asked of it.
    let out = apply(&session, &half, Tier::Steward, &none());
    assert!(out.rejected.is_empty());
    assert_eq!(out.ops.len(), 4);
    assert_eq!(health::shape(&out.session).roots, 8);
    assert_eq!(thought(&out.session, 8).category.as_deref(), Some("late"));
    assert_eq!(replay(&session, &out.ops).unwrap(), out.session);
}

#[test]
fn guidance_stays_with_the_map_and_replays_from_the_journal() {
    let session = map();
    let words = "Call the map Meeting overload, and never split the decisions branch";
    let ops = vec![Op::Title {
        title: "Meeting overload".into(),
    }];
    let event = SessionEvent::Guided {
        instruction: words.into(),
        ops: ops.clone(),
        note: "Renamed the map".into(),
    };
    let next = event.reduce(&session).unwrap();
    assert_eq!(next.title, "Meeting overload");
    assert_eq!(next.directions, [words]);
    assert_eq!(next, guided(&session, words, &ops).unwrap());
    let json = serde_json::to_string(&event).unwrap();
    assert_eq!(serde_json::from_str::<SessionEvent>(&json).unwrap(), event);
    // Guidance that changes nothing today is still remembered, and read by every later turn.
    let lasting = guided(&next, "  Keep branch names to one word  ", &[]).unwrap();
    assert_eq!(lasting.directions.len(), 2);
    assert_eq!(lasting.directions[1], "Keep branch names to one word");
    assert_eq!(lasting.thoughts, next.thoughts);
    let mut waiting = lasting.clone();
    waiting.add("Hiring has stalled.");
    for brief in [
        brief::steward(&lasting, &none(), &Stands::default()),
        brief::direct(&lasting, &none(), "Tidy up"),
        brief::place(&waiting, &none(), &Stands::default())
            .unwrap()
            .0,
        brief::listen(&lasting),
    ] {
        assert!(brief.contains("\nGUIDANCE\n"));
        assert!(brief.contains("- Keep branch names to one word\n"));
    }
    // The map remembers the latest twenty.
    let mut long = lasting;
    for n in 0..MAX_GUIDANCE {
        long = guided(&long, &format!("Rule {n}"), &[]).unwrap();
    }
    assert_eq!(long.directions.len(), MAX_GUIDANCE);
    assert_eq!(long.directions[0], "Rule 0");
    // An instruction that cannot be carried out whole is no journal entry.
    assert!(guided(&session, words, &[place(1, "nowhere", "")]).is_err());
    assert!(guided(&session, " ", &ops).is_err());
    assert!(guided(&session, &"x".repeat(MAX_INSTRUCTION + 1), &[]).is_err());
    assert!(guided(&session, &"x".repeat(MAX_INSTRUCTION), &[]).is_ok());
}

fn order(branches: &[&str]) -> Op {
    Op::Order {
        branches: branches.iter().map(|id| (*id).into()).collect(),
    }
}
fn seats(session: &Session) -> Vec<&str> {
    session.categories.iter().map(|c| c.id.as_str()).collect()
}

#[test]
fn branches_read_in_the_order_a_mapper_gives_them() {
    let grown = [
        branch("owners", "Owners", Some("decisions")),
        branch("agendas", "Agendas", None),
        place(1, "owners", ""),
        place(3, "agendas", ""),
    ];
    let session = replay(&map(), &grown).unwrap();
    assert_eq!(
        seats(&session),
        ["decisions", "meetings", "owners", "agendas"]
    );
    let put = |on: &Session, tier: Tier, ids: &[&str]| apply(on, &[order(ids)], tier, &none());
    // The branches named take, between them, the seats they held; the rest stay put.
    let journey = ["agendas", "meetings", "decisions"];
    let read = ["agendas", "meetings", "owners", "decisions"];
    for tier in [Tier::Steward, Tier::Direct] {
        let out = put(&session, tier, &journey);
        assert!(out.rejected.is_empty());
        assert_eq!(out.ops, [order(&journey)]);
        assert_eq!(seats(&out.session), read);
        // Nothing but the order changed, and the journal replays it.
        assert_eq!(out.session.thoughts, session.thoughts);
        assert_eq!(replay(&session, &out.ops).unwrap(), out.session);
        // An order the map already has is no edit at all.
        let again = put(&out.session, tier, &journey);
        assert!(again.ops.is_empty() && again.rejected.is_empty());
    }
    // Two named are enough: one moves ahead of the other and nothing else stirs.
    let out = put(&session, Tier::Steward, &["agendas", "decisions"]);
    assert_eq!(seats(&out.session), read);
    // A branch that went while the turn was out is passed over, and one named twice counts once.
    let stale = ["agendas", "gone", "meetings", "agendas", "decisions"];
    let out = put(&session, Tier::Steward, &stale);
    assert!(out.rejected.is_empty());
    assert_eq!(seats(&out.session), read);
    assert_eq!(replay(&session, &out.ops).unwrap(), out.session);
    let lone = put(&session, Tier::Steward, &["gone", "agendas"]);
    assert!(lone.ops.is_empty() && lone.rejected.is_empty());
    // A branch grown in the same turn takes the seat it is given.
    let mut longer = session.clone();
    longer.add("Nobody writes down what was agreed.");
    let ops = [
        branch("follow-up", "Follow-up", None),
        place(5, "follow-up", "Unrecorded agreements"),
        order(&["follow-up", "decisions"]),
    ];
    let out = apply(&longer, &ops, Tier::Steward, &none());
    assert!(out.rejected.is_empty());
    assert_eq!(
        seats(&out.session),
        ["follow-up", "meetings", "owners", "agendas", "decisions"]
    );
    assert_eq!(replay(&longer, &out.ops).unwrap(), out.session);
    // Reading order is structure, and structure is not the placer's.
    assert_eq!(
        refused(&session, Tier::Place, order(&journey)),
        "Only the steward restructures the map"
    );
}
