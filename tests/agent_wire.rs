//! What the models are asked for, and how their answers become edits.
use openrouter_rs::{api::chat::Message, types::Role as Speaker};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tacit::agent::{Op, Tier, apply, prompt, replay, turn, wire};
use tacit::model::{ParkKind, Relationship, Role, Session};

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
fn place(thought: u64, branch: &str, concept: &str, role: Option<Role>) -> Op {
    Op::Place {
        thought,
        branch: branch.into(),
        concept: (!concept.is_empty()).then(|| concept.into()),
        role,
    }
}
fn park(thought: u64, kind: ParkKind, topic: &str, reason: &str, concept: &str) -> Op {
    Op::Park {
        thought,
        kind,
        topic: topic.into(),
        reason: reason.into(),
        concept: (!concept.is_empty()).then(|| concept.into()),
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
        place(1, "decisions", "Unclear decision owner", None),
        place(2, "decisions", "Reopened decisions", None),
        place(3, "meetings", "Long meetings", None),
        place(4, "meetings", "Silent invitees", None),
    ];
    replay(&session, &ops).unwrap()
}
fn read(tier: Tier, session: &Session, answer: Value) -> wire::Reply {
    wire::read(&answer.to_string(), session, tier).unwrap()
}
fn entry(id: u64, idea: Value, outcome: &str, parent: Value, reason: Value, topic: Value) -> Value {
    json!({"id": id, "idea": idea, "outcome": outcome, "parent": parent, "reason": reason,
        "role": if outcome == "place" { "Tension" } else { "Keep" }, "topic": topic})
}

#[test]
fn a_placers_answer_becomes_edits() {
    let session = talk(&[
        "Our meetings run long.",
        "Nobody here trusts the plan.",
        "Can you hear me?",
        "I must call the dentist.",
    ]);
    let wordy = "Missing trust among the people on this team right now today";
    let answer = json!({
        "branches": [{"id": "Team Trust", "name": "Team trust", "parent": null},
            {"id": "meetings", "name": "Meetings", "parent": null}],
        "file": [
            entry(1, json!("Long  meetings"), "place", json!("meetings"), Value::Null, Value::Null),
            entry(2, json!(wordy), "place", json!("Team Trust"), Value::Null, Value::Null),
            entry(3, Value::Null, "aside", Value::Null, json!("Said to the room"), Value::Null),
            entry(4, json!("Dentist call"), "off_topic", Value::Null, json!("An errand"), json!("Dentist")),
        ],
        "steward": "  ",
        "about": "Meeting  overload"
    });
    let reply = read(Tier::Place, &session, answer);
    let tension = Some(Role::Tension);
    let expected = [
        // A new branch gets a slug whatever the model called it, and references follow.
        branch("team-trust", "Team trust", None),
        branch("meetings", "Meetings", None),
        place(1, "meetings", "Long meetings", tension),
        // A label is cut to the whole words that fit rather than refused.
        place(
            2,
            "team-trust",
            "Missing trust among the people on this team",
            tension,
        ),
        park(3, ParkKind::Aside, "Aside", "Said to the room", ""),
        // Nothing is off topic before the map has a subject: it waits as "not yet".
        park(4, ParkKind::Later, "Dentist", "An errand", "Dentist call"),
        Op::Title {
            title: "Meeting overload".into(),
        },
    ];
    assert_eq!(reply.ops, expected);
    assert_eq!((reply.escalate, reply.note.as_str()), (None, ""));
    let out = apply(&session, &reply.ops, Tier::Place, &BTreeSet::new());
    assert!(out.rejected.is_empty());
    assert_eq!(out.session.title, "Meeting overload");

    // Once the map has a subject, a different one is off topic, and the title is the steward's.
    let mut settled = out.session;
    for line in ["Agendas arrive late.", "No one keeps time."] {
        settled.add(line);
    }
    let settled = replay(
        &settled,
        &[
            place(5, "meetings", "Late agendas", None),
            place(6, "meetings", "No timekeeper", None),
        ],
    )
    .unwrap();
    let mut settled = settled;
    settled.add("My car needs a service.");
    let answer = json!({
        "branches": null,
        "file": [entry(7, Value::Null, "off_topic", Value::Null, json!("An errand"), json!("Car"))],
        "steward": "Meetings has outgrown its name",
        "about": "Something else"
    });
    let reply = read(Tier::Place, &settled, answer);
    assert_eq!(
        reply.ops,
        [park(7, ParkKind::OffTopic, "Car", "An errand", "")]
    );
    assert_eq!(
        reply.escalate.as_deref(),
        Some("Meetings has outgrown its name")
    );
}

#[test]
fn a_stewards_answer_becomes_edits_in_a_mappers_order() {
    let mut session = map();
    session.add("Honestly nobody can say who makes the call.");
    session.add("I must call the dentist.");
    let answer = json!({
        "aim": {"focus": "Why the team's time goes to meetings", "principle": "Causes of lost time", "title": null},
        "audit": "Ownership deserves its own branch;  one repeat to fold.",
        "branches": {
            "grow": [{"id": "Ownership", "name": "Ownership", "parent": "[decisions] Decisions"}],
            "merge": [],
            // A model sometimes answers with a branch's name where its ID was asked for.
            "move": [{"branch": "Meetings", "parent": null}],
            "order": ["Meetings", "[decisions] Decisions", "nowhere", "meetings"],
            "remove": null,
            "rename": [{"branch": "decisions", "name": "Decision making"}]
        },
        "ideas": {
            "file": [{"id": 5, "idea": "Unclear decision owner", "parent": "Ownership", "role": "Keep"},
                {"id": 1, "idea": null, "parent": "ownership", "role": "Tension"}],
            "merge": [{"from": [5], "idea": "No clear decision owner", "into": 1}],
            "park": [{"id": 6, "kind": "off_topic", "reason": "A personal errand", "topic": ""}]
        },
        "links": [{"from": 1, "label": "prolongs", "to": 3}, {"from": 2, "label": " ", "to": 4}],
        "narrative": " Nobody owns decisions, so meetings fill the gap. "
    });
    let reply = read(Tier::Steward, &session, answer);
    assert_eq!(
        reply.note,
        "Ownership deserves its own branch; one repeat to fold."
    );
    let kinds: Vec<String> = reply
        .ops
        .iter()
        .map(|op| {
            serde_json::to_value(op).unwrap()["op"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    // Structure first, then the thoughts, then how the branches read and what is said about
    // the whole.
    assert_eq!(
        kinds,
        [
            "branch",
            "move",
            "rename",
            "place",
            "place",
            "park",
            "combine",
            "order",
            "links",
            "focus",
            "narrative"
        ]
    );
    let out = apply(&session, &reply.ops, Tier::Steward, &BTreeSet::new());
    assert!(out.rejected.is_empty());
    let after = out.session;
    let names: Vec<_> = after.categories.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Meetings", "Decision making", "Ownership"]);
    let one = after.thoughts.iter().find(|t| t.id == 1).unwrap();
    assert_eq!(one.category.as_deref(), Some("ownership"));
    assert_eq!(one.concept.as_deref(), Some("No clear decision owner"));
    assert_eq!(one.role, Role::Tension);
    assert_eq!(after.parked[0].topic, "Off topic");
    let charter = after.charter.as_ref().unwrap();
    assert_eq!(charter.principle, "Causes of lost time");
    assert_eq!(
        charter.narrative,
        "Nobody owns decisions, so meetings fill the gap."
    );
    let link = Relationship {
        from: 1,
        to: 3,
        label: "prolongs".into(),
    };
    assert_eq!(charter.links, [link]);
    assert!(after.interpretation.is_some());
}

#[test]
fn an_answer_with_nothing_to_change_is_no_edits() {
    let empty = json!({"aim": null, "audit": null, "branches": null, "ideas": null,
        "links": null, "narrative": null});
    assert!(read(Tier::Steward, &map(), empty).ops.is_empty());
    let empty = json!({"about": null, "branches": null, "file": null, "steward": null});
    assert!(read(Tier::Place, &map(), empty).ops.is_empty());
    // An empty list of links is an instruction to clear them; null keeps them.
    let clear = json!({"audit": "Sound; no change.", "links": []});
    assert_eq!(
        read(Tier::Steward, &map(), clear).ops,
        [Op::Links { links: vec![] }]
    );
    // A directed turn that changes nothing still has its word for the user.
    let nothing = json!({"about": "Meetings is a main branch already.", "aim": null,
        "branches": null, "ideas": null, "links": null, "narrative": null, "remember": false,
        "reply": "Meetings is already a main branch."});
    let reply = read(Tier::Direct, &map(), nothing);
    assert!(reply.ops.is_empty() && !reply.remember);
    assert_eq!(reply.note, "Meetings is already a main branch.");
    for tier in [Tier::Place, Tier::Steward, Tier::Direct] {
        for garbled in ["Here is the map you asked for", "{\"file\": [{\"id\": 5,"] {
            let error = wire::read(garbled, &map(), tier).err();
            assert_eq!(
                error.as_deref(),
                Some("The map agent returned an unreadable answer")
            );
        }
    }
}

#[test]
fn an_answer_is_read_through_whatever_surrounds_it() {
    let mut session = map();
    session.add("Agendas arrive late.");
    let file = [entry(
        5,
        json!("Late agendas"),
        "place",
        json!("meetings"),
        Value::Null,
        Value::Null,
    )];
    let answer = json!({"about": null, "branches": null, "file": file, "steward": null});
    let expected = read(Tier::Place, &session, answer.clone()).ops;
    assert_eq!(
        expected,
        [place(5, "meetings", "Late agendas", Some(Role::Tension))]
    );
    // A fence, an introduction, and the whole answer said twice over.
    for raw in [
        format!("```json\n{answer}\n```"),
        format!("Here is the placement:\n{answer}\nLet me know if it helps."),
        format!("{answer} </json {answer}"),
    ] {
        assert_eq!(
            wire::read(&raw, &session, Tier::Place).unwrap().ops,
            expected
        );
    }
}

fn keys(schema: &Value) -> Vec<&str> {
    let properties = schema["properties"].as_object().unwrap();
    properties.keys().map(String::as_str).collect()
}
/// Strict structured output wants every object closed and every property required. A choice
/// of words is a plain string: providers disagree on how a nullable one is written.
fn closed(schema: &Value) {
    assert!(schema.get("type").is_some(), "{schema} has no type");
    if let Some(words) = schema.get("enum") {
        assert_eq!(schema["type"], "string");
        assert!(words.as_array().unwrap().iter().all(Value::is_string));
    }
    if schema.get("properties").is_some() {
        assert_eq!(schema["additionalProperties"], false);
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        assert_eq!(keys(schema), required);
        for property in schema["properties"].as_object().unwrap().values() {
            closed(property);
        }
    }
    if let Some(items) = schema.get("items") {
        closed(items);
    }
}
/// What a steward's or a director's answer says of each thought it files.
fn filing(schema: &Value) -> Vec<&str> {
    keys(&schema["properties"]["ideas"]["properties"]["file"]["items"])
}

#[test]
fn an_answer_is_written_in_the_order_a_mapper_thinks() {
    // A model fills the fields in the order the schema lists them, so the order is the method:
    // name the subject before its parts, an idea before judging it, and judge it before
    // choosing its branch.
    let placer = prompt::placer_schema();
    closed(&placer);
    assert_eq!(keys(&placer), ["about", "branches", "file", "steward"]);
    assert_eq!(
        keys(&placer["properties"]["file"]["items"]),
        ["id", "idea", "outcome", "parent", "reason", "role", "topic"]
    );
    // Settle the aim before the structure, and the structure before the reading.
    let steward = prompt::steward_schema();
    closed(&steward);
    assert_eq!(
        keys(&steward),
        ["aim", "audit", "branches", "ideas", "links", "narrative"]
    );
    let sent = steward.to_string();
    let at = |name: &str| sent.find(&format!("{name:?}:{{")).unwrap();
    assert!(at("aim") < at("branches") && at("branches") < at("ideas"));
    // A directed turn reads what was asked before it edits, and answers the user last of all.
    let director = prompt::director_schema();
    closed(&director);
    assert_eq!(
        keys(&director),
        [
            "about",
            "aim",
            "branches",
            "ideas",
            "links",
            "narrative",
            "remember",
            "reply"
        ]
    );
    let sent = director.to_string();
    let at = |name: &str| sent.find(&format!("{name:?}:{{")).unwrap();
    assert!(at("about") < at("aim") && at("ideas") < at("reply"));
    // Structure is settled in one place: what grows and folds, then how the branches read.
    for (text, schema) in [(prompt::STEWARD, &steward), (prompt::DIRECTOR, &director)] {
        let branches = &schema["properties"]["branches"];
        assert_eq!(
            keys(branches),
            ["grow", "merge", "move", "order", "remove", "rename"]
        );
        for key in keys(branches) {
            assert!(
                text.contains(&format!(" {key} (")),
                "{key} is not described"
            );
        }
    }
    // Only the user pins a thought where it is, so only their turn is offered the choice.
    assert_eq!(filing(&steward), ["id", "idea", "parent", "role"]);
    assert_eq!(filing(&director), ["id", "idea", "parent", "pin", "role"]);
    // The listener names a thought before it says where the thought ends.
    let listener = prompt::listener_schema();
    closed(&listener);
    assert_eq!(
        keys(&listener["properties"]["thoughts"]["items"]),
        ["about", "closes", "end"]
    );
    // Every word the prompts promise is a field the schema has.
    for (text, schema) in [
        (prompt::PLACER, &placer),
        (prompt::STEWARD, &steward),
        (prompt::DIRECTOR, &director),
        (prompt::LISTENER, &listener),
    ] {
        for key in keys(schema) {
            assert!(
                text.contains(&format!("\n{key}: ")),
                "{key} is not described"
            );
        }
    }
}

#[test]
fn each_tier_is_asked_in_its_own_way() {
    let messages = || vec![Message::new(Speaker::User, "MAP")];
    let request = turn::request(Tier::Place, "fast/model", "off", messages(), 2160).unwrap();
    let json = serde_json::to_value(request).unwrap();
    assert_eq!(json["model"], "fast/model");
    assert_eq!(json["max_tokens"], 2160);
    assert_eq!(json["messages"][0]["content"], "MAP");
    // The placer's answer is short, so the first token is what the speaker waits for.
    assert_eq!(json["provider"]["sort"], "latency");
    assert_eq!(json["provider"]["require_parameters"], true);
    assert_eq!(json["reasoning"], json!({"enabled": false}));
    let format = &json["response_format"]["json_schema"];
    assert_eq!(
        (&format["name"], &format["strict"]),
        (&json!("placement"), &json!(true))
    );
    assert_eq!(format["schema"], prompt::placer_schema());

    let request = turn::request(Tier::Steward, "strong/model", "medium", messages(), 12_000);
    let json = serde_json::to_value(request.unwrap()).unwrap();
    assert_eq!(json["provider"]["sort"], "throughput");
    assert_eq!(json["reasoning"], json!({"effort": "medium"}));
    let format = &json["response_format"]["json_schema"];
    assert_eq!(format["name"], "stewardship");
    assert_eq!(format["schema"], prompt::steward_schema());

    // A directed turn is as long an answer, in its own format.
    let request = turn::request(Tier::Direct, "strong/model", "low", messages(), 12_600);
    let json = serde_json::to_value(request.unwrap()).unwrap();
    assert_eq!(
        (&json["max_tokens"], &json["provider"]["sort"]),
        (&json!(12_600), &json!("throughput"))
    );
    assert_eq!(json["reasoning"], json!({"effort": "low"}));
    let format = &json["response_format"]["json_schema"];
    assert_eq!(
        (&format["name"], &format["strict"]),
        (&json!("direction"), &json!(true))
    );
    assert_eq!(format["schema"], prompt::director_schema());

    // An empty effort leaves the model's own default alone.
    let request = turn::request(Tier::Steward, "strong/model", "", messages(), 12_000);
    let json = serde_json::to_value(request.unwrap()).unwrap();
    assert!(json.get("reasoning").is_none());
}

#[test]
fn a_directors_answer_becomes_edits_made_on_the_users_word() {
    let mut session = map();
    session.add("Agendas arrive late.");
    let answer = json!({
        "about": "The agenda thought belongs under Meetings; the silent invitees can wait.",
        "aim": {"focus": null, "principle": null, "title": null},
        "audit": "Filed the agenda thought.",
        "branches": null,
        "ideas": {
            // The user named the first and said where it goes; the second moves along with it.
            "file": [{"id": 5, "idea": "Late agendas", "parent": "meetings", "pin": true, "role": "Tension"},
                {"id": 3, "idea": null, "parent": "[decisions] Decisions", "pin": false, "role": "Keep"}],
            "merge": [],
            "park": [{"id": 4, "kind": "later", "reason": "", "topic": ""}]
        },
        "links": null,
        "narrative": null,
        "remember": true,
        "reply": "Late agendas  now stays under Meetings, and Silent invitees is set aside."
    });
    let reply = read(Tier::Direct, &session, answer.clone());
    let pin = Op::Pin {
        thought: 5,
        branch: "meetings".into(),
    };
    let edits = [
        place(5, "meetings", "Late agendas", Some(Role::Tension)),
        place(3, "decisions", "", None),
        park(
            4,
            ParkKind::Later,
            "Not yet",
            "Waiting for its place on the map",
            "",
        ),
    ];
    let [filed, moved, parked] = edits.clone();
    assert_eq!(reply.ops, [filed, pin, moved, parked]);
    // The user reads the reply, never the audit.
    assert_eq!(
        reply.note,
        "Late agendas now stays under Meetings, and Silent invitees is set aside."
    );
    assert_eq!((reply.remember, reply.escalate), (true, None));
    let out = apply(&session, &reply.ops, Tier::Direct, &BTreeSet::new());
    assert!(out.rejected.is_empty());
    let state = |after: &Session| {
        let at = |id: u64| after.thoughts.iter().find(|t| t.id == id).unwrap();
        let (five, three, set) = (at(5), at(3), &after.parked[0]);
        (
            (five.category.clone(), five.role, five.pinned),
            (three.category.clone(), three.concept.clone(), three.pinned),
            (set.thought.id, set.kind, set.held),
        )
    };
    let (meetings, decisions) = (Some("meetings".to_owned()), Some("decisions".to_owned()));
    let long = Some("Long meetings".to_owned());
    // What the user decided is theirs: the pin holds and the parked thought stays parked.
    assert_eq!(
        state(&out.session),
        (
            (meetings.clone(), Role::Tension, true),
            (decisions.clone(), long.clone(), false),
            (4, ParkKind::Later, true)
        )
    );

    // The steward makes the same edits on its own word: nothing pinned, held or remembered.
    let reply = read(Tier::Steward, &session, answer);
    assert_eq!(reply.ops, edits);
    assert_eq!(
        (reply.note.as_str(), reply.remember),
        ("Filed the agenda thought.", false)
    );
    let out = apply(&session, &reply.ops, Tier::Steward, &BTreeSet::new());
    assert!(out.rejected.is_empty());
    assert_eq!(
        state(&out.session),
        (
            (meetings, Role::Tension, false),
            (decisions, long, false),
            (4, ParkKind::Later, false)
        )
    );
}

#[test]
fn a_reading_order_names_only_the_branches_left_standing() {
    let mut session = map();
    session.add("Agendas arrive late.");
    let answer = |merge: Value, order: Value| {
        json!({
            "about": "The user wants the agenda thoughts first.",
            "aim": null,
            "branches": {
                "grow": [{"id": "Agendas", "name": "Agendas", "parent": null}],
                "merge": merge,
                "move": [],
                "order": order,
                "remove": [],
                "rename": []
            },
            "ideas": {
                "file": [{"id": 5, "idea": "Late agendas", "parent": "Agendas", "pin": false, "role": "Tension"}],
                "merge": [],
                "park": []
            },
            "links": null,
            "narrative": null,
            "remember": false,
            "reply": "Agendas now comes first."
        })
    };
    let order = |branches: &[&str]| Op::Order {
        branches: branches.iter().map(|id| (*id).into()).collect(),
    };
    let seats = |after: &Session| -> Vec<String> {
        let ids = after.categories.iter().map(|c| c.id.clone());
        ids.collect()
    };
    // A name, a bracketed id and a branch grown in the same answer all resolve. What names no
    // branch is dropped, and so is a second mention.
    let said = json!([
        "Agendas",
        "[meetings] Meetings",
        "nowhere",
        "meetings",
        "Decisions"
    ]);
    let reply = read(Tier::Direct, &session, answer(json!([]), said));
    assert_eq!(
        reply.ops.last(),
        Some(&order(&["agendas", "meetings", "decisions"]))
    );
    let out = apply(&session, &reply.ops, Tier::Direct, &BTreeSet::new());
    assert!(out.rejected.is_empty());
    assert_eq!(seats(&out.session), ["agendas", "meetings", "decisions"]);
    assert_eq!(replay(&session, &out.ops).unwrap(), out.session);
    // A branch the same answer folds away has no seat to take.
    let fold = json!([{"from": "Meetings", "into": "decisions"}]);
    let said = json!(["agendas", "meetings", "decisions"]);
    let reply = read(Tier::Direct, &session, answer(fold.clone(), said));
    assert_eq!(reply.ops.last(), Some(&order(&["agendas", "decisions"])));
    let out = apply(&session, &reply.ops, Tier::Direct, &BTreeSet::new());
    assert!(out.rejected.is_empty());
    assert_eq!(seats(&out.session), ["agendas", "decisions"]);
    // One branch alone has no order to take.
    let reply = read(
        Tier::Direct,
        &session,
        answer(fold, json!(["meetings", "agendas"])),
    );
    assert!(!reply.ops.iter().any(|op| matches!(op, Op::Order { .. })));
    let reply = read(Tier::Steward, &session, answer(json!([]), Value::Null));
    assert!(!reply.ops.iter().any(|op| matches!(op, Op::Order { .. })));
}
