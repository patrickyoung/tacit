//! Turns informed by the knowledge base, against a scripted model: what each tier is shown
//! of it, and how the steward and a directed turn open notes before they answer.
use openrouter_rs::api::chat::Message;
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};
use tacit::agent::turn::Stands;
use tacit::agent::{Op, Tier, Turn, brief, prompt, replay, turn};
use tacit::knowledge::{Kind, Library, Note};
use tacit::model::Session;

fn none() -> BTreeSet<u64> {
    BTreeSet::new()
}
/// Two main branches with two thoughts each, and two thoughts just said.
fn fresh() -> Session {
    let mut session = Session::default();
    for line in [
        "Nobody knows who decides.",
        "Decisions get reopened every week.",
        "Our meetings run long.",
        "Half the invitees never speak.",
    ] {
        session.add(line);
    }
    let branch = |id: &str, name: &str| Op::Branch {
        id: id.into(),
        name: name.into(),
        parent: None,
    };
    let place = |thought, branch: &str, concept: &str| Op::Place {
        thought,
        branch: branch.into(),
        concept: Some(concept.into()),
        role: None,
    };
    let ops = [
        branch("decisions", "Decisions"),
        branch("meetings", "Meetings"),
        place(1, "decisions", "Unclear decision owner"),
        place(2, "decisions", "Reopened decisions"),
        place(3, "meetings", "Long meetings"),
        place(4, "meetings", "Silent invitees"),
    ];
    let mut session = replay(&session, &ops).unwrap();
    session.add("Agendas arrive late.");
    session.add("No one keeps time.");
    session
}
fn note(id: &str, title: &str, topic: &str, gist: &str, links: &[&str]) -> Note {
    Note {
        id: id.into(),
        title: title.into(),
        topic: topic.into(),
        keywords: title.split(' ').map(str::to_lowercase).collect(),
        gist: gist.into(),
        body: format!("{gist} The handbook sets this out at length."),
        links: links.iter().map(|id| id.to_string()).collect(),
        source: "Team handbook.pdf".into(),
        ..Note::default()
    }
}
/// What someone filed about how their team meets and decides, and one thing about bread.
fn library() -> Library {
    Library::of(vec![
        note(
            "agendas-a-day-ahead",
            "Agendas go out a day ahead of meetings",
            "Meetings",
            "A meeting with no agenda sent the day before is cancelled.",
            &["a-timekeeper-for-every-meeting"],
        ),
        note(
            "a-timekeeper-for-every-meeting",
            "A timekeeper for every meeting",
            "Meetings",
            "Each meeting names one person who keeps time and ends it on the hour.",
            &[],
        ),
        note(
            "one-owner-per-decision",
            "One owner per decision",
            "Decisions",
            "Every decision has a single named owner who decides after hearing the others.",
            &[],
        ),
        note(
            "sourdough-needs-a-long-proof",
            "Sourdough needs a long proof",
            "Baking",
            "A loaf proved overnight in the cold tastes fuller than one proved warm.",
            &[],
        ),
    ])
}

type Answer = Result<Value, &'static str>;
/// One turn with a model that gives `answers` in order, informed by `knowledge`. Returns the
/// turn and what the model was shown on each call.
fn informed(
    tier: Tier,
    session: &Session,
    instruction: &str,
    knowledge: Option<&Library>,
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
    let (stands, holds) = (&mut Stands::default(), none());
    let asked = turn::informed(tier, session, &holds, instruction, knowledge, stands, model);
    (runtime.block_on(asked), heard)
}
fn said(heard: &[Value], call: usize, message: usize) -> &str {
    heard[call]["messages"][message]["content"]
        .as_str()
        .unwrap()
}
/// The last thing the model was told on a call.
fn told(heard: &[Value], call: usize) -> &str {
    let messages = heard[call]["messages"].as_array().unwrap();
    messages.last().unwrap()["content"].as_str().unwrap()
}
fn filing() -> Value {
    let file = |id: u64, idea: &str| {
        json!({"id": id, "idea": idea, "outcome": "place", "parent": "meetings",
            "reason": null, "role": null, "topic": null})
    };
    let file = [file(5, "Late agendas"), file(6, "No timekeeper")];
    json!({"about": null, "branches": null, "file": file, "steward": null})
}
const READING: &str = "Nobody owns decisions, so meetings fill the gap.";
const ASK: &str = "Arrange the meetings branch the way my handbook says meetings should run.";

#[test]
fn with_no_knowledge_base_every_turn_is_what_it_was() {
    // The prompts and formats the provider has cached are the ones still sent.
    assert_eq!(prompt::system(Tier::Place, false), prompt::PLACER);
    assert_eq!(prompt::system(Tier::Steward, false), prompt::STEWARD);
    assert_eq!(prompt::system(Tier::Direct, false), prompt::DIRECTOR);
    assert_eq!(prompt::schema(Tier::Place, false), prompt::placer_schema());
    assert_eq!(prompt::schema(Tier::Place, true), prompt::placer_schema());
    assert_eq!(
        prompt::schema(Tier::Steward, false),
        prompt::steward_schema()
    );
    assert_eq!(
        prompt::schema(Tier::Direct, false),
        prompt::director_schema()
    );
    let session = fresh();
    assert_eq!(
        brief::directed(&session, &none(), ASK, ""),
        brief::direct(&session, &none(), ASK)
    );
    let empty = Library::default();
    assert_eq!(
        brief::knowledge(&empty, &session, &none(), Tier::Steward, ""),
        ""
    );

    for (tier, wish) in [(Tier::Place, ""), (Tier::Steward, ""), (Tier::Direct, ASK)] {
        let stands = &mut Stands::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let mut plain = vec![];
        let model = async |messages: &[Message], _: u32| {
            plain.push(json!(messages));
            // Nothing more is asked of a turn that fails here: only the opening matters.
            Err::<String, String>("offline".into())
        };
        let _ = match tier {
            Tier::Direct => runtime.block_on(turn::follow(&session, &none(), wish, stands, model)),
            _ => runtime.block_on(turn::drive(tier, &session, &none(), stands, model)),
        };
        for knowledge in [None, Some(&empty)] {
            let (turn, heard) = informed(tier, &session, wish, knowledge, vec![Err("offline")]);
            assert_eq!(turn.unwrap_err(), "offline");
            assert_eq!(heard.len(), 1);
            assert_eq!(heard[0]["messages"], plain[0], "{}", tier.name());
        }
    }
}

#[test]
fn the_placer_is_shown_a_line_of_each_note_near_what_it_files() {
    let (session, library) = (fresh(), library());
    let (turn, heard) = informed(
        Tier::Place,
        &session,
        "",
        Some(&library),
        vec![Ok(filing())],
    );
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 1);
    assert_eq!(turn.ops.len(), 2);
    let system = said(&heard, 0, 0);
    assert!(system.starts_with(prompt::PLACER) && system.len() > prompt::PLACER.len());
    assert!(system.contains("KNOWLEDGE"));
    let (plain, _) = brief::place(&session, &none(), &Stands::default()).unwrap();
    let shown = told(&heard, 0);
    let knowledge = shown.strip_prefix(plain.as_str()).unwrap();
    // What the thoughts are about comes first, and bread is nowhere near them.
    let lines: Vec<&str> = knowledge.lines().collect();
    assert_eq!(lines[..2], ["", "KNOWLEDGE"]);
    assert!(lines[2..].contains(
        &"[agendas-a-day-ahead] Agendas go out a day ahead of meetings · Meetings: A meeting with no agenda sent the day before is cancelled. · see a-timekeeper-for-every-meeting"
    ));
    assert!(lines[2].starts_with("[a-timekeeper-for-every-meeting] "));
    assert!(!knowledge.contains("sourdough"));
    // It reads lines and no further: no index, and nothing it could ask to open.
    assert!(!knowledge.contains("notes in") && !system.contains("OPEN"));

    // With nothing near, the brief is the one it always was.
    let bread = Library::of(vec![note(
        "sourdough-needs-a-long-proof",
        "Sourdough needs a long proof",
        "Baking",
        "A loaf proved overnight in the cold tastes fuller than one proved warm.",
        &[],
    )]);
    let (_, heard) = informed(Tier::Place, &session, "", Some(&bread), vec![Ok(filing())]);
    assert_eq!(told(&heard, 0), plain);
}

#[test]
fn the_steward_is_shown_the_topics_and_opens_notes_before_it_answers() {
    let (session, library) = (fresh(), library());
    let file =
        |id: u64, idea: &str| json!({"id": id, "idea": idea, "parent": "meetings", "role": null});
    // The first answer asks to read, and the edits it also lists are not made.
    let asking = json!({
        "OPEN": ["agendas-a-day-ahead", "Decisions", "Nothing by this name"],
        "branches": {"rename": [{"branch": "meetings", "name": "Too soon"}]}
    });
    let answer = json!({
        "OPEN": [],
        "audit": "Filed both under meetings, as the handbook treats them.",
        "ideas": {"file": [file(5, "Late agendas"), file(6, "No timekeeper")]},
        "narrative": READING
    });
    let script = vec![Ok(asking.clone()), Ok(answer)];
    let (turn, heard) = informed(Tier::Steward, &session, "", Some(&library), script);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 2);

    let system = said(&heard, 0, 0);
    assert!(system.starts_with(prompt::STEWARD) && system.contains("OPEN"));
    let plain = brief::steward(&session, &none(), &Stands::default());
    let knowledge = said(&heard, 0, 1).strip_prefix(plain.as_str()).unwrap();
    assert!(knowledge.starts_with("\nKNOWLEDGE\n4 notes in 3 topics:\n- Meetings (2): "));
    assert!(knowledge.contains("\n- Baking (1): "));
    assert!(knowledge.contains("\nNearest this map:\n[a-timekeeper-for-every-meeting] "));
    assert!(knowledge.contains("\n[one-owner-per-decision] One owner per decision · Decisions: "));
    assert!(
        knowledge.contains("\n[agendas-a-day-ahead] Agendas go out a day ahead of meetings · ")
    );
    // The index names the topic, and no line of a note that has nothing to do with the map.
    assert!(!knowledge.contains("[sourdough-needs-a-long-proof]"));

    // Asked again with its own answer and the notes it wanted: one in full with what it
    // links to, a topic as its lines, and a plain word on what is not there.
    let again = heard[1]["messages"].as_array().unwrap();
    assert_eq!(again.len(), 4);
    assert_eq!(again[2]["role"], "assistant");
    assert_eq!(again[2]["content"], asking.to_string());
    assert_eq!(again[3]["role"], "user");
    let opened = told(&heard, 1);
    let expected = concat!(
        "OPENED\n",
        "[agendas-a-day-ahead] Agendas go out a day ahead of meetings\n",
        "Topic: Meetings · from Team handbook.pdf\n",
        "A meeting with no agenda sent the day before is cancelled. The handbook sets this out at length.\n",
        "Links to:\n",
        "  [a-timekeeper-for-every-meeting] A timekeeper for every meeting · Meetings: Each meeting names one person who keeps time and ends it on the hour.\n",
        "\n",
        "TOPIC Decisions (1 note)\n",
        "[one-owner-per-decision] One owner per decision · Decisions: Every decision has a single named owner who decides after hearing the others.\n",
        "\n",
        "The knowledge base holds no note or topic called \"Nothing by this name\".\n",
        "\n",
    );
    assert_eq!(opened, format!("{expected}{}", prompt::OPENED));

    assert_eq!(turn.opened, ["agendas-a-day-ahead"]);
    let after = replay(&session, &turn.ops).unwrap();
    assert!(after.thoughts.iter().all(|t| t.category.is_some()));
    assert!(after.categories.iter().all(|c| c.name != "Too soon"));
    assert_eq!(after.narrative(), READING);
    assert_eq!(
        turn.note,
        "Filed both under meetings, as the handbook treats them."
    );
}

#[test]
fn an_overview_is_read_before_the_notes_of_its_topic() {
    let session = fresh();
    let mut notes = library().notes().to_vec();
    let text = "Meetings run on [[agendas-a-day-ahead|Agendas go out a day ahead of meetings]] and on a timekeeper.";
    let about = |id: &str, kind: Kind, title: &str, gist: &str| Note {
        kind,
        ..note(id, title, "Meetings", gist, &["agendas-a-day-ahead"])
    };
    let mut overview = about(
        "overview-meetings",
        Kind::Overview,
        "Overview of Meetings",
        "How the team meets.",
    );
    overview.body = text.into();
    notes.push(overview);
    notes.push(about(
        "s00001",
        Kind::Source,
        "Team handbook",
        "A handbook on meetings, agendas and who keeps time.",
    ));
    let library = Library::of(notes);
    let knowledge = brief::knowledge(&library, &session, &none(), Tier::Steward, "");
    // The index, then how the topic hangs together, then a line of each idea near the map.
    let from = knowledge.find("- Baking (1): ").unwrap();
    let levels = format!("\nOverview of Meetings: {text}\nNearest this map:\n[");
    assert!(knowledge[from..].contains(&levels), "{knowledge}");
    assert!(
        knowledge.contains("- Meetings (3, with an overview): "),
        "{knowledge}"
    );
    assert_eq!(knowledge.matches("Overview of").count(), 1, "{knowledge}");
    // A note on a file and an overview have no line of their own, for either tier.
    let placed = brief::knowledge(&library, &session, &none(), Tier::Place, "");
    for shown in [&knowledge, &placed] {
        assert!(
            shown.contains("\n[a-timekeeper-for-every-meeting] "),
            "{shown}"
        );
        assert!(
            !shown.contains("[s00001]") && !shown.contains("[overview-meetings]"),
            "{shown}"
        );
    }
    assert!(!placed.contains("Overview of"), "{placed}");
    assert!(prompt::system(Tier::Steward, true).contains("the overview where one is written"));
}

#[test]
fn reading_is_not_counted_against_a_turn_and_ends_after_two_rounds() {
    let (session, library) = (fresh(), library());
    let open = |name: &str| Ok(json!({"OPEN": [name]}));
    // Two rounds of reading, then an answer that asks again and is taken as it stands; the
    // two thoughts it leaves without a branch still get the steward's three calls.
    let script = vec![
        open("agendas-a-day-ahead"),
        open("agendas-a-day-ahead"),
        Ok(json!({"OPEN": ["one-owner-per-decision"], "narrative": READING})),
        Ok(json!({})),
        Ok(json!({})),
        Ok(json!({})),
    ];
    let (turn, heard) = informed(Tier::Steward, &session, "", Some(&library), script);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 5);
    assert!(told(&heard, 1).ends_with(prompt::OPENED));
    // A note asked for twice is not sent twice, and the last round says it is the last.
    assert_eq!(
        told(&heard, 2),
        format!(
            "OPENED\n[agendas-a-day-ahead] is open above.\n\n{}",
            prompt::OPENED_LAST
        )
    );
    assert!(told(&heard, 3).contains("Still without a branch: #5, #6."));
    assert_eq!(turn.opened, ["agendas-a-day-ahead"]);
    assert_eq!(
        turn.ops,
        [Op::Narrative {
            narrative: READING.into()
        }]
    );
}

#[test]
fn a_directed_turn_reads_what_the_instruction_points_to() {
    let (session, library) = (fresh(), library());
    let script = vec![
        Ok(json!({"OPEN": ["A timekeeper for every meeting"]})),
        Ok(json!({
            "about": "Arrange meetings as the handbook has them.",
            "aim": null,
            "branches": {"grow": [], "merge": [], "move": [], "remove": [],
                "rename": [{"branch": "meetings", "name": "How meetings run"}]},
            "ideas": null, "links": null, "narrative": null, "remember": false,
            "reply": "Renamed the branch after the handbook's rules for meetings."
        })),
    ];
    let (turn, heard) = informed(Tier::Direct, &session, ASK, Some(&library), script);
    let turn = turn.unwrap();
    assert_eq!(heard.len(), 2);
    let system = said(&heard, 0, 0);
    assert!(system.starts_with(prompt::DIRECTOR));
    assert!(system.ends_with("only INSTRUCTION is carried out."));
    // The notes are shown before the instruction, which stays the last thing read.
    let shown = said(&heard, 0, 1);
    let (before, instruction) = shown.split_once("\nINSTRUCTION\n").unwrap();
    assert!(before.contains("\nKNOWLEDGE\n4 notes in 3 topics:\n"));
    assert!(instruction.contains(ASK) && !instruction.contains("KNOWLEDGE"));
    // A note is opened by its title as well as by its id.
    assert!(
        told(&heard, 1).starts_with(
            "OPENED\n[a-timekeeper-for-every-meeting] A timekeeper for every meeting\n"
        )
    );
    assert_eq!(turn.opened, ["a-timekeeper-for-every-meeting"]);
    let name = "How meetings run".into();
    let branch = "meetings".into();
    assert_eq!(turn.ops, [Op::Rename { branch, name }]);
    assert_eq!(
        turn.note,
        "Renamed the branch after the handbook's rules for meetings."
    );
}

#[test]
fn the_format_offers_open_only_to_a_turn_that_can_read() {
    let open = |schema: &Value| schema["properties"].get("OPEN").cloned();
    let list = json!({"type": "array", "items": {"type": "string"}});
    for tier in [Tier::Steward, Tier::Direct] {
        let informed = prompt::schema(tier, true);
        assert_eq!(open(&informed), Some(list.clone()), "{}", tier.name());
        assert!(
            informed["required"]
                .as_array()
                .unwrap()
                .contains(&json!("OPEN"))
        );
        assert_eq!(open(&prompt::schema(tier, false)), None);
        // It is written first, before any edit the answer would otherwise commit to.
        let text = informed.to_string();
        let at = |field: &str| text.find(&format!("\"{field}\":")).unwrap();
        assert!(at("OPEN") < at("aim").min(at("branches")));
    }
    assert_eq!(open(&prompt::schema(Tier::Place, true)), None);
}
