//! Synthetic-only check of the mind map agent. A made-up talk with tangents, a deferral, filler
//! and one thought the speaker parks until it has a home is placed capture by capture and
//! stewarded, then scored against what a facilitator would have done with it. A second talk
//! explains six things in turn, which is one more part than a map has room for side by side.
//! A third walks through one framework of seven parts: the kind of talk that tempts a map into
//! a single branch.
//!
//! It spends OpenRouter credits, so keep runs few:
//!   cargo run --release --example agent_eval -- [runs=1] [mode=full|place|steward|direct]
//!       [every=3] [talk=onboarding|machines|plots] [place=MODEL] [place_effort=off|low|…]
//!       [steward=MODEL] [steward_effort=…] [direct=MODEL] [direct_effort=…] [save=FILE]
//!       [frames=DIR] [load=FILE] [ask=INSTRUCTION] [quiet=1] [trace=1]
//! `mode=place` never calls the steward; `mode=steward` stewards a session saved by `save=`.
//! `mode=direct` gives a saved onboarding map the instructions a user might, one directed turn
//! each (one of them a request no map can hold, which must change nothing), then lets the
//! steward loose on it to see that it leaves what they decided alone. With `ask=`, it gives
//! any saved map that one instruction instead, then the steward two looks with nothing new
//! said: the second shows what the first chose to leave. The map it leaves is read by eye.
use openrouter_rs::{api::chat::Message, types::Role};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use tacit::agent::{self, Op, Tier, Turn, Usage, brief, health, prompt, turn, wire};
use tacit::model::{ParkKind, Session};
use tacit::organizer::CloudConfig;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Want {
    /// Belongs on the map.
    On,
    /// Another subject: parked as off topic.
    Off,
    /// Filler with nothing to map: parked as an aside.
    Aside,
    /// The speaker set it aside for another day: parked.
    Defer,
    /// Parked by the speaker until it has a home, which arrives later in the talk.
    Back,
}
use Want::*;

/// A product lead thinking aloud about slow onboarding, one capture per slice.
const TALK: &[&[(Want, &str)]] = &[
    &[
        (
            Aside,
            "Okay, is this recording? Right, let me just think out loud for a bit.",
        ),
        (
            On,
            "New customers take about three weeks before they build their first real report, and I think that is why trials are not converting.",
        ),
        (
            On,
            "The signup form asks for fourteen fields before anyone sees the product.",
        ),
        (
            On,
            "People have to verify their email and then wait for a sales call before the workspace even opens.",
        ),
    ],
    &[
        (
            On,
            "Connecting a data source is where most people stall, because it needs an admin on their side to approve access.",
        ),
        (
            On,
            "Our sample data is about a shoe shop, which means nothing to a hospital or a bank.",
        ),
        (
            Off,
            "Oh, that reminds me, I still need to book the dentist for Thursday.",
        ),
        (
            Back,
            "I want to come back to how we would measure any of this, but I have nowhere to put that yet, so park it for now.",
        ),
    ],
    &[
        (
            On,
            "Nobody owns onboarding end to end: sales hands off to support and product only hears about it in the churn review.",
        ),
        (
            On,
            "Support answers the same five setup questions every single week.",
        ),
        (
            On,
            "The help articles still show the old interface from two versions ago.",
        ),
        (Aside, "Hmm, where was I going with that."),
    ],
    &[
        (
            On,
            "What if the first screen was a working dashboard built from a template for their industry?",
        ),
        (
            On,
            "We could let people explore with sample data first and connect their own source later.",
        ),
        (
            Defer,
            "Pricing is tangled up in this too, but that is a separate conversation and I do not want to open it today.",
        ),
        (
            On,
            "A checklist inside the product could show the four steps to a first report.",
        ),
    ],
    &[
        (
            Off,
            "The coffee machine on the third floor is broken again, somebody should really tell facilities.",
        ),
        (
            On,
            "Should the sales call happen before the trial starts or only after someone has seen value?",
        ),
        (
            On,
            "Sales wants to qualify every lead, but making people wait for a call is exactly what slows them down.",
        ),
        (
            On,
            "Is three weeks actually bad for our kind of product, or is that normal for the category?",
        ),
    ],
    &[
        (
            On,
            "Enterprise customers cannot start at all until single sign-on is configured, and that takes their IT team days.",
        ),
        (
            On,
            "Like I said, there is no single owner for onboarding, and that is probably the root of it.",
        ),
        (
            Off,
            "Did anyone watch the match last night? Unbelievable ending.",
        ),
        (
            On,
            "We could name one onboarding lead who is accountable from signup to first report.",
        ),
    ],
    &[
        (
            On,
            "On measurement: only thirty-one percent of trials create a report in their first week.",
        ),
        (
            On,
            "Median time from signup to first report is nine days for self-serve and twenty-two for enterprise.",
        ),
        (
            On,
            "We should track time to first report as the one number everyone looks at.",
        ),
        (Aside, "Okay, good, good."),
    ],
    &[
        (
            On,
            "Guided setup calls work well, but we only offer them to the biggest accounts.",
        ),
        (
            On,
            "A risk with templates is that people judge the product by a dashboard that is not really theirs.",
        ),
        (
            Off,
            "I also have to send the board deck to Maria before Friday, completely unrelated.",
        ),
        (
            On,
            "Could we pre-approve data access during the sales process so the admin step is already done on day one?",
        ),
    ],
    &[
        (
            On,
            "Shorter signup would mean worse lead data for sales, so that needs a trade.",
        ),
        (
            On,
            "The quickest experiment is cutting the signup form to three fields and watching week one activation.",
        ),
        (
            On,
            "Next step is to ask Priya for the funnel numbers by segment.",
        ),
        (Aside, "Alright, I think that's everything for now."),
    ],
];

/// An explainer with an enumeration: six parts, a shared principle, and a grouping the speaker
/// only reveals near the end. More parts than a map has main branches.
const MACHINES: &[&[(Want, &str)]] = &[
    &[
        (Aside, "Right, quick test, testing. Okay."),
        (
            On,
            "I want to explain the six simple machines, the classic set that every complicated machine is built out of.",
        ),
        (
            On,
            "What they all do is trade force against distance: you push less hard but you push further.",
        ),
    ],
    &[
        (
            On,
            "The lever is a stiff bar turning on a pivot, which is called the fulcrum.",
        ),
        (
            On,
            "The further from the fulcrum you push, the less force you need, which is why a long crowbar lifts a heavy stone.",
        ),
        (
            On,
            "A seesaw, a wheelbarrow and a pair of tweezers are the three classes of lever, depending on where the fulcrum sits.",
        ),
    ],
    &[
        (
            On,
            "The wheel and axle is really a lever that goes all the way round: a big wheel turning a small axle multiplies force.",
        ),
        (
            On,
            "A doorknob and a screwdriver handle are both wheel and axle.",
        ),
        (
            Off,
            "Speaking of doors, I have to remember to call the locksmith about the back door at home.",
        ),
    ],
    &[
        (
            On,
            "A pulley is a wheel with a rope over it. A single fixed pulley only changes the direction you pull.",
        ),
        (
            On,
            "Add a moving pulley and the load hangs from two rope segments, so you pull with half the force but twice the rope.",
        ),
        (
            On,
            "A block and tackle stacks several pulleys, which is how sailors hoist a heavy sail by hand.",
        ),
    ],
    &[
        (
            On,
            "The inclined plane is just a ramp: it lets you raise a load by pushing it along a longer, gentler path.",
        ),
        (
            On,
            "The Egyptians probably moved pyramid blocks up long ramps.",
        ),
        (
            Defer,
            "There is a whole debate about how exactly the pyramids were built, but I will leave that for another session.",
        ),
    ],
    &[
        (
            On,
            "A wedge is two inclined planes back to back, and unlike a ramp it moves: an axe pushes the wood apart sideways.",
        ),
        (On, "Knives, chisels and even your front teeth are wedges."),
        (Aside, "Hang on, let me find my place."),
    ],
    &[
        (
            On,
            "The screw is an inclined plane wrapped around a cylinder.",
        ),
        (
            On,
            "Each turn moves it forward by just one thread, so a small twist gives a huge squeeze, like in a vice or a jar lid.",
        ),
        (On, "Archimedes used a screw inside a tube to lift water."),
    ],
    &[
        (
            On,
            "So really there are two families: the lever, the wheel and axle and the pulley all turn about a point, and the ramp, the wedge and the screw are all slopes.",
        ),
        (
            On,
            "The number that describes all of them is mechanical advantage: the output force divided by the input force.",
        ),
        (
            On,
            "None of them saves any work, because friction means you always put in a bit more than you get out.",
        ),
    ],
    &[
        (
            On,
            "As I said at the start, every one of these trades force for distance.",
        ),
        (
            On,
            "A bicycle combines several: the pedals and cranks are levers, the gears are wheels and axles, and the brake lever pulls a cable.",
        ),
        (On, "Scissors are two levers with wedges for blades."),
    ],
    &[
        (
            Off,
            "Oh, and I need to send the invoice to the printers today, nothing to do with this.",
        ),
        (
            On,
            "Is a gear its own simple machine or just a wheel and axle with teeth? I am not sure.",
        ),
        (Aside, "Okay, that is all six. Done."),
    ],
];

/// A walkthrough of one framework: seven parts named in turn, then a use for them. Nothing
/// in it leaves the subject, and most of it is one list.
const PLOTS: &[&[(Want, &str)]] = &[
    &[
        (
            On,
            "Every story ever told follows one of seven basic plot structures.",
        ),
        (On, "Christopher Booker described these seven basic plots."),
    ],
    &[
        (
            On,
            "Number one, overcoming the monster: the hero defeats a threat to their home.",
        ),
        (
            On,
            "Number two, rags to riches: someone poor gains wealth, loses it, and grows to deserve it.",
        ),
    ],
    &[
        (
            On,
            "Number three, the quest: the hero sets out to find a treasure or place against obstacles.",
        ),
        (
            On,
            "Number four, voyage and return: the hero visits a strange land and comes back changed.",
        ),
    ],
    &[
        (
            On,
            "Number five, comedy: confusion and misunderstanding resolve into a happy ending.",
        ),
        (
            On,
            "Number six, tragedy: a flaw or mistake leads the hero to downfall.",
        ),
    ],
    &[(
        On,
        "Number seven, rebirth: a dark force traps the hero until an act of redemption frees them.",
    )],
    &[
        (On, "So pretty much every story fits one of these patterns."),
        (
            On,
            "Knowing them lets you master storytelling and plan your own arc.",
        ),
    ],
    &[(
        On,
        "What if we used these structures to shape our product launch story?",
    )],
];

type Talk = &'static [&'static [(Want, &'static str)]];
struct Args {
    talk: Talk,
    runs: usize,
    mode: String,
    every: usize,
    save: Option<String>,
    frames: Option<String>,
    load: Option<String>,
    ask: Option<String>,
    quiet: bool,
    trace: bool,
}
/// `key=value` arguments; a bare number is the run count.
fn args(config: &mut CloudConfig) -> Result<Args, String> {
    let mut args = Args {
        talk: TALK,
        runs: 1,
        mode: "full".into(),
        every: 3,
        save: None,
        frames: None,
        load: None,
        ask: None,
        quiet: false,
        trace: false,
    };
    for arg in std::env::args().skip(1) {
        let (key, value) = arg.split_once('=').unwrap_or(("runs", &arg));
        let number = || {
            value
                .parse::<usize>()
                .map_err(|_| format!("Bad number in {arg}"))
        };
        match key {
            "runs" => args.runs = number()?.clamp(1, 5),
            "talk" if value == "onboarding" => args.talk = TALK,
            "talk" if value == "machines" => args.talk = MACHINES,
            "talk" if value == "plots" => args.talk = PLOTS,
            "every" => args.every = number()?.max(1),
            "mode" if ["full", "place", "steward", "direct"].contains(&value) => {
                args.mode = value.into()
            }
            "place" => config.place_model = value.into(),
            "place_effort" => config.place_effort = value.into(),
            "steward" => config.steward_model = value.into(),
            "steward_effort" => config.steward_effort = value.into(),
            "direct" => config.direct_model = value.into(),
            "direct_effort" => config.direct_effort = value.into(),
            "save" => args.save = Some(value.into()),
            "frames" => args.frames = Some(value.into()),
            "load" => args.load = Some(value.into()),
            "ask" => args.ask = Some(value.into()),
            "quiet" => args.quiet = value != "0",
            "trace" => args.trace = value != "0",
            _ => return Err(format!("Unknown argument {arg}")),
        }
    }
    if ["steward", "direct"].contains(&args.mode.as_str()) && args.load.is_none() {
        let mode = &args.mode;
        return Err(format!(
            "mode={mode} needs load=FILE from an earlier save=FILE"
        ));
    }
    Ok(args)
}

#[derive(Debug, PartialEq)]
enum At {
    Map,
    Unfiled,
    Parked(ParkKind),
    Gone,
}
/// Where a captured thought is now. One combined into another is wherever that one is.
fn at(session: &Session, id: u64) -> At {
    if let Some(t) = session.thoughts.iter().find(|t| t.id == id) {
        return if t.category.is_some() {
            At::Map
        } else {
            At::Unfiled
        };
    }
    if let Some(p) = session.parked.iter().find(|p| p.thought.id == id) {
        return At::Parked(p.kind);
    }
    let into = session
        .retired
        .iter()
        .find(|r| r.thought.id == id)
        .and_then(|r| r.replaced_by.first());
    into.map_or(At::Gone, |into| at(session, *into))
}
/// The main branch each filed thought hangs from.
fn roots(session: &Session) -> BTreeMap<u64, String> {
    let root = |t: &tacit::model::Thought| {
        let path = session.category_path(t.category.as_deref());
        Some((t.id, path.first()?.id.clone()))
    };
    session.thoughts.iter().filter_map(root).collect()
}
fn clip(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    flat.chars().take(max).chain(['…']).collect()
}
fn outline(session: &Session) {
    fn branch(session: &Session, parent: Option<&str>, depth: usize) {
        let pad = "  ".repeat(depth + 1);
        for c in session
            .categories
            .iter()
            .filter(|c| c.parent.as_deref() == parent)
        {
            println!("{pad}{} ({})", c.name, health::weight(session, &c.id));
            for t in session
                .thoughts
                .iter()
                .filter(|t| t.category.as_deref() == Some(&c.id))
            {
                println!(
                    "{pad}  - #{} {} [{:?}]",
                    t.id,
                    clip(t.map_label(), 70),
                    t.role
                );
            }
            branch(session, Some(&c.id), depth + 1);
        }
    }
    println!("{}", session.title);
    branch(session, None, 0);
    for t in session.thoughts.iter().filter(|t| t.category.is_none()) {
        println!("  UNFILED #{} {}", t.id, clip(&t.text, 70));
    }
    for p in &session.parked {
        println!(
            "  parked {:?} · {} · #{} {} ({})",
            p.kind,
            p.topic,
            p.thought.id,
            clip(&p.thought.text, 50),
            clip(&p.reason, 60)
        );
    }
    if let Some(c) = &session.charter {
        println!("  focus: {}\n  ordering idea: {}", c.focus, c.principle);
        for link in &c.links {
            println!("  link #{} → #{}: {}", link.from, link.to, link.label);
        }
    }
    println!("  narrative: {}", brief::narrative(session));
}

#[derive(Default)]
struct Tally {
    place: Vec<Duration>,
    steward: Vec<Duration>,
    direct: Vec<Duration>,
    calls: u32,
    prompt: u64,
    completion: u64,
    cost: f64,
    /// Filed thoughts the steward moved to another main branch.
    churn: usize,
    /// Edits that held in the turn but not on the session it was briefed on: always a bug.
    lost: usize,
    /// Turns whose first answer could not be read and was asked for again.
    garbled: usize,
    /// Print every edit a turn made.
    trace: bool,
    /// What the steward is not sent back to, as its lane keeps it: the strains it left
    /// standing, and what the instructions arranged.
    stands: turn::Stands,
}
impl Tally {
    fn spend(&mut self, turn: &Turn) {
        match turn.tier {
            Tier::Place => self.place.push(turn.usage.elapsed),
            Tier::Steward => self.steward.push(turn.usage.elapsed),
            Tier::Direct => self.direct.push(turn.usage.elapsed),
        }
        self.calls += turn.usage.calls;
        self.prompt += turn.usage.prompt_tokens;
        self.completion += turn.usage.completion_tokens;
        self.cost += turn.usage.cost;
        self.garbled += usize::from(turn.garbled);
    }
}
fn seconds(times: &[Duration]) -> String {
    let mut sorted: Vec<f64> = times.iter().map(Duration::as_secs_f64).collect();
    sorted.sort_by(f64::total_cmp);
    match sorted.as_slice() {
        [] => "none".into(),
        all => format!(
            "{} turn(s), median {:.2}s, slowest {:.2}s",
            all.len(),
            all[all.len() / 2],
            all[all.len() - 1]
        ),
    }
}

/// Ask once more, outside the agent, to see what the provider says. The agent itself never
/// shows this: a provider's error can echo the prompt. Here the prompt is made up.
async fn why(
    client: &openrouter_rs::OpenRouterClient,
    config: &CloudConfig,
    tier: Tier,
    session: &Session,
    instruction: &str,
) -> String {
    let (none, stands) = (BTreeSet::new(), turn::Stands::default());
    let (system, brief, model, effort) = match tier {
        Tier::Place => (
            prompt::PLACER,
            brief::place(session, &none, &stands).map_or(String::new(), |b| b.0),
            &config.place_model,
            &config.place_effort,
        ),
        Tier::Steward => (
            prompt::STEWARD,
            brief::steward(session, &none, &stands),
            &config.steward_model,
            &config.steward_effort,
        ),
        Tier::Direct => (
            prompt::DIRECTOR,
            brief::direct(session, &none, instruction),
            &config.direct_model,
            &config.direct_effort,
        ),
    };
    let messages = vec![
        Message::new(Role::System, system),
        Message::new(Role::User, brief),
    ];
    let request = match turn::request(tier, model, effort, messages, 6000) {
        Ok(request) => request,
        Err(error) => return error,
    };
    match client.chat().create(&request).await {
        Err(error) => format!("provider says: {error:?}"),
        Ok(response) => match response.choices.first() {
            None => "asked again: no choices".into(),
            Some(choice) => format!(
                "asked again: finish {:?}, answer {}",
                choice.finish_reason(),
                clip(choice.content().unwrap_or("(no text)"), 600)
            ),
        },
    }
}

/// One agent turn on the session as it stands: for `Tier::Direct`, the one that carries out
/// `instruction`.
async fn step(
    (client, config): Cloud<'_>,
    tier: Tier,
    session: &mut Session,
    tally: &mut Tally,
    label: &str,
    instruction: &str,
) -> Result<Turn, String> {
    let none = BTreeSet::new();
    let (model, effort) = match tier {
        Tier::Place => (&config.place_model, &config.place_effort),
        Tier::Steward => (&config.steward_model, &config.steward_effort),
        Tier::Direct => (&config.direct_model, &config.direct_effort),
    };
    // The agent never keeps an answer it could not read. Here the talk is made up, so it is shown.
    let (started, mut usage, mut unread) = (Instant::now(), Usage::default(), vec![]);
    let ask = async |messages: &[Message], budget: u32| -> Result<String, String> {
        let request = turn::request(tier, model, effort, messages.to_vec(), budget)?;
        let raw = turn::call(client, &request, tier.patience(), &mut usage).await?;
        if wire::read(&raw, session, tier).is_err() {
            unread.push(raw.clone());
        }
        Ok(raw)
    };
    let driven = match tier {
        Tier::Direct => turn::follow(session, &none, instruction, &mut tally.stands, ask).await,
        _ => turn::drive(tier, session, &none, &mut tally.stands, ask).await,
    };
    let mut turn = match driven {
        Ok(turn) => turn,
        // One failure ends the run: the rest would only spend credits on the same fault.
        Err(error) => {
            let reason = match unread.last() {
                Some(raw) => format!("could not read: {}", clip(raw, 4000)),
                None => why(client, config, tier, session, instruction).await,
            };
            return Err(format!("{label}: {error}\n  {reason}"));
        }
    };
    usage.elapsed = started.elapsed();
    turn.usage = usage;
    for raw in unread.iter().filter(|_| turn.garbled) {
        println!("{:11}asked again after: {}", "", clip(raw, 4000));
    }
    let (was, parked) = (roots(session), session.parked.len());
    let outcome = agent::apply(session, &turn.ops, tier, &none);
    // What the journal would hold must rebuild exactly the map the user sees. Guidance is
    // journaled when it changed the map or is to be remembered.
    let journal = match tier {
        Tier::Direct if outcome.ops.is_empty() && !turn.remember => Ok(session.clone()),
        Tier::Direct => agent::guided(session, instruction, &outcome.ops),
        _ => agent::replay(session, &outcome.ops),
    };
    let mut next = outcome.session;
    if let Ok(journaled) = &journal {
        next.directions.clone_from(&journaled.directions);
    }
    let replayed = journal.as_ref() == Ok(&next);
    tally.lost += outcome.rejected.len() + usize::from(!replayed);
    tally.spend(&turn);
    *session = next;
    let now = roots(session);
    let moved = was
        .iter()
        .filter(|(id, root)| now.get(id).is_some_and(|new| new != *root))
        .count();
    if tier == Tier::Steward {
        tally.churn += moved;
    }
    let mut flags = String::new();
    for (on, flag) in [
        (turn.remember, ", to be remembered"),
        (turn.unmet > 0, ", ENDED ON A REFUSAL"),
        (!replayed, " REPLAY MISMATCH"),
    ] {
        if on {
            flags.push_str(flag);
        }
    }
    println!(
        "{label:<10} {:>5.2}s {} call(s) {:>5}→{:<5} tok ${:.4} | {} edit(s), parked {}→{}, regrouped {moved}{flags}",
        turn.usage.elapsed.as_secs_f64(),
        turn.usage.calls,
        turn.usage.prompt_tokens,
        turn.usage.completion_tokens,
        turn.usage.cost,
        outcome.ops.len(),
        parked,
        session.parked.len(),
    );
    for op in outcome.ops.iter().filter(|_| tally.trace) {
        println!("{:11}{}", "", serde_json::to_string(op).unwrap_or_default());
    }
    for r in turn.refused.iter().filter(|_| tally.trace) {
        let op = serde_json::to_string(&r.op).unwrap_or_default();
        println!("{:11}refused {op}: {}", "", r.reason);
    }
    for r in &outcome.rejected {
        println!("{:11}lost on rebase: {}", "", r.reason);
    }
    if !turn.note.is_empty() {
        println!("{:11}note: {}", "", clip(&turn.note, 300));
    }
    if let Some(reason) = &turn.escalate {
        println!("{:11}calls for the steward: {}", "", clip(reason, 200));
    }
    Ok(turn)
}

/// Whether the map is what a facilitator would have left on the wall.
fn score(
    session: &Session,
    truth: &BTreeMap<u64, Want>,
    returned: Option<bool>,
    steward: bool,
) -> bool {
    let park = |want: &Want| matches!(want, Off | Aside | Defer);
    let on = truth.values().filter(|w| **w == On).count();
    let on_map = truth
        .iter()
        .filter(|(id, w)| **w == On && at(session, **id) == At::Map)
        .count();
    let should = truth.values().filter(|w| park(w)).count();
    let (mut caught, mut kinds, mut wrongly) = (0, 0, vec![]);
    for (id, want) in truth {
        match (at(session, *id), want) {
            (At::Parked(kind), want) if park(want) => {
                caught += 1;
                kinds += usize::from(match want {
                    Off => kind == ParkKind::OffTopic,
                    Aside => kind == ParkKind::Aside,
                    _ => kind != ParkKind::Aside,
                });
            }
            (At::Parked(_), _) => wrongly.push(*id),
            _ => {}
        }
    }
    let missed: Vec<u64> = truth
        .iter()
        .filter(|(id, w)| park(w) && !matches!(at(session, **id), At::Parked(_)))
        .map(|(id, _)| *id)
        .collect();
    let shape = health::shape(session);
    let labels: Vec<usize> = session
        .thoughts
        .iter()
        .filter(|t| t.category.is_some())
        .map(|t| t.map_label().split_whitespace().count())
        .collect();
    let unlabelled = session
        .thoughts
        .iter()
        .filter(|t| t.category.is_some() && t.concept.is_none())
        .count();
    let depth = session
        .categories
        .iter()
        .map(|c| session.category_path(Some(&c.id)).len())
        .max()
        .unwrap_or(0);
    // Nothing parked that should not have been is all of it right, even with nothing to park.
    let precision = match wrongly.len() {
        0 => 100,
        wrong => caught * 100 / (caught + wrong),
    };
    println!("\nshape      {shape:?}, depth {depth}");
    println!(
        "labels     {} filed, {unlabelled} unlabelled, longest {} words, mean {:.1}",
        labels.len(),
        labels.iter().max().unwrap_or(&0),
        labels.iter().sum::<usize>() as f64 / labels.len().max(1) as f64
    );
    println!("on map     {on_map}/{on} of what belongs there");
    println!(
        "parking    caught {caught}/{should} (missed {missed:?}), precision {precision}% (wrongly parked {wrongly:?}), right kind {kinds}/{caught}"
    );
    println!(
        "combined   {} repeat(s) folded into another thought",
        session.retired.len()
    );
    if let Some(parked) = returned {
        println!(
            "comeback   the thought parked until it had a home was {} and is now {:?}",
            if parked {
                "parked at first"
            } else {
                "never parked"
            },
            truth
                .iter()
                .find(|(_, w)| **w == Back)
                .map(|(id, _)| at(session, *id))
        );
    }
    for note in health::notes(session) {
        println!("health     {note}");
    }
    let loose = vague(session);
    if !loose.is_empty() {
        println!("catch-all  {}", loose.join(", "));
    }
    let back = truth
        .iter()
        .all(|(id, w)| *w != Back || at(session, *id) == At::Map);
    let twins = health::notes(session)
        .iter()
        .any(|note| note.contains("carry the same label"));
    let mut short = vec![];
    for (ok, what) in [
        (shape.unfiled == 0, "thoughts left without a branch"),
        (
            on_map * 10 >= on * 9,
            "on-topic thoughts missing from the map",
        ),
        (caught * 4 >= should * 3, "tangents left on the map"),
        (precision >= 80, "on-topic thoughts parked"),
        (
            (3..=agent::MAX_ROOTS).contains(&shape.roots),
            "main branch count",
        ),
        (!steward || !health::strained(session), "a strained map"),
        (loose.is_empty(), "a catch-all branch"),
        (!steward || !twins, "thoughts sharing one label"),
        (
            !steward || !brief::narrative(session).is_empty(),
            "no narrative",
        ),
        (!steward || back, "the parked thought never came back"),
    ] {
        if !ok {
            short.push(what);
        }
    }
    println!(
        "verdict    {}",
        if short.is_empty() {
            "sound".into()
        } else {
            format!("SHORT: {}", short.join("; "))
        }
    );
    short.is_empty()
}

/// Branches named for nothing in particular, or for a kind of thought instead of a part of
/// the subject.
fn vague(session: &Session) -> Vec<&str> {
    const LOOSE: [&str; 7] = [
        "other",
        "others",
        "misc",
        "miscellaneous",
        "general",
        "various",
        "cross-cutting",
    ];
    const KINDS: [&str; 5] = [
        "questions",
        "open questions",
        "ideas",
        "observations",
        "possibilities",
    ];
    let loose = |name: &str| {
        let name = name.to_lowercase();
        let any = name.split_whitespace().any(|word| LOOSE.contains(&word));
        any || KINDS.contains(&name.as_str())
    };
    let names = session.categories.iter().map(|c| c.name.as_str());
    names.filter(|name| loose(name)).collect()
}

type Check = (&'static str, fn(&Session) -> bool);
/// Something a user might ask of the onboarding map once it has been talked through.
struct Ask {
    say: &'static str,
    /// Thoughts it may park, and bring back, at most.
    parks: usize,
    returns: usize,
    /// It says how the map is to be kept from now on.
    lasting: bool,
    /// It asks for what no map can hold: nothing may change, and the reply says so.
    still: bool,
    /// True once the turn has landed, and from then on whatever else is asked.
    holds: &'static [Check],
    /// True once the turn has landed, until the user has the map reorganised.
    then: &'static [Check],
}
/// The thought that speaks for `id` now, through whatever it was folded into.
fn heir(session: &Session, id: u64) -> u64 {
    let retired = session.retired.iter().find(|r| r.thought.id == id);
    let into = retired.and_then(|r| r.replaced_by.first());
    into.map_or(id, |into| heir(session, *into))
}
/// Parked on the user's word, as one of these kinds.
fn held(session: &Session, ids: &[u64], kinds: &[ParkKind]) -> bool {
    ids.iter().all(|id| {
        let id = heir(session, *id);
        let parked = session.parked.iter().find(|p| p.thought.id == id);
        parked.is_some_and(|p| p.held && kinds.contains(&p.kind))
    })
}
fn words(name: &str) -> usize {
    let word = |w: &&str| w.chars().any(char::is_alphanumeric);
    name.split_whitespace().filter(word).count()
}
fn quick(session: &Session) -> bool {
    let root = session
        .categories
        .iter()
        .find(|c| c.parent.is_none() && c.name.eq_ignore_ascii_case("quick wins"));
    root.is_some_and(|root| {
        [16, 34].iter().all(|id| {
            let id = heir(session, *id);
            let filed = session.thoughts.iter().find(|t| t.id == id);
            filed.is_some_and(|t| session.within_category(t, &root.id))
        })
    })
}
/// Where the main branch holding `id` comes among the main branches, if the thought is filed.
fn seat(session: &Session, id: u64) -> Option<usize> {
    let id = heir(session, id);
    let filed = session.thoughts.iter().find(|t| t.id == id)?;
    let path = session.category_path(filed.category.as_deref());
    let main = path.first()?;
    let mut roots = session.categories.iter().filter(|c| c.parent.is_none());
    roots.position(|c| c.id == main.id)
}
const ASKS: &[Ask] = &[
    Ask {
        say: "Bring the pricing thought back onto the map. I do want to deal with it today.",
        parks: 0,
        returns: 1,
        lasting: false,
        still: false,
        holds: &[("the pricing thought is on the map", |s| {
            at(s, 15) == At::Map
        })],
        then: &[],
    },
    Ask {
        say: "Park everything about the sales call for another day.",
        parks: 4,
        returns: 0,
        lasting: false,
        still: false,
        holds: &[("the sales call is parked for later", |s| {
            held(s, &[18, 19], &[ParkKind::Later])
        })],
        then: &[],
    },
    Ask {
        say: "The dentist and the coffee machine are real errands. Keep them parked, but as things for later rather than off topic.",
        parks: 2,
        returns: 0,
        lasting: false,
        still: false,
        holds: &[("the errands are parked for later", |s| {
            held(s, &[7, 17], &[ParkKind::Later])
        })],
        then: &[],
    },
    Ask {
        say: "The match and the board deck have nothing to do with this. Keep them parked for good.",
        parks: 2,
        returns: 0,
        lasting: false,
        still: false,
        holds: &[(
            "the match and the board deck stay parked as off topic",
            |s| held(s, &[23, 31], &[ParkKind::OffTopic]),
        )],
        then: &[],
    },
    Ask {
        say: "Make a new main branch called Quick wins and put the in-product checklist and the three-field signup experiment in it.",
        parks: 0,
        returns: 0,
        lasting: false,
        still: false,
        holds: &[],
        then: &[("Quick wins is a main branch holding both thoughts", quick)],
    },
    Ask {
        say: "From now on, keep every branch name to one or two words.",
        parks: 0,
        returns: 0,
        lasting: true,
        still: false,
        holds: &[("every branch name is one or two words", |s| {
            s.categories.iter().all(|c| words(&c.name) <= 2)
        })],
        then: &[],
    },
    Ask {
        say: "Put the first main branch exactly at pixel x=170, y=400, set it in bold red, and animate a spinning 3D cube beside it.",
        parks: 0,
        returns: 0,
        lasting: false,
        still: true,
        holds: &[],
        then: &[],
    },
    Ask {
        say: "Call the map Onboarding fix. And get rid of the thought about the help articles, I don't care about that one.",
        parks: 1,
        returns: 0,
        lasting: false,
        still: false,
        holds: &[
            ("the map is called Onboarding fix", |s| {
                s.title.trim().eq_ignore_ascii_case("onboarding fix")
            }),
            ("the help-articles thought is off the map for good", |s| {
                held(s, &[11], &[ParkKind::OffTopic, ParkKind::Aside])
            }),
        ],
        then: &[],
    },
    Ask {
        say: "Reorganise the whole map around the customer's journey, from signing up to building a first report.",
        parks: 0,
        returns: 0,
        lasting: false,
        still: false,
        holds: &[],
        then: &[
            ("three to eight main branches", |s| {
                let roots = s.categories.iter().filter(|c| c.parent.is_none());
                (3..=8).contains(&roots.count())
            }),
            ("nothing is left without a branch", |s| {
                s.thoughts.iter().all(|t| t.category.is_some())
            }),
            ("signing up reads before connecting data", |s| {
                seat(s, 3).zip(seat(s, 5)).is_none_or(|(a, b)| a <= b)
            }),
            ("no branch carries more than seven items", |s| {
                health::shape(s).crowded == 0
            }),
        ],
    },
];
/// The asks in turn, then a steward's look at what they left. Returns what fell short.
async fn direct(
    cloud: Cloud<'_>,
    session: &mut Session,
    tally: &mut Tally,
) -> Result<Vec<String>, String> {
    let lot = |s: &Session| -> BTreeSet<u64> { s.parked.iter().map(|p| p.thought.id).collect() };
    let pinned = |s: &Session| -> Vec<(u64, Option<String>)> {
        let pinned = s.thoughts.iter().filter(|t| t.pinned);
        pinned.map(|t| (t.id, t.category.clone())).collect()
    };
    let said = session.thoughts.len() + session.parked.len() + session.retired.len();
    let (mut short, mut standing): (Vec<String>, Vec<(usize, Check)>) = (vec![], vec![]);
    let tell = |found: &[(bool, String)]| {
        for (ok, what) in found {
            println!("{:11}{} {what}", "", if *ok { "ok  " } else { "FAIL" });
        }
    };
    for (index, ask) in ASKS.iter().enumerate() {
        let number = index + 1;
        println!("\nask {number}      {}", ask.say);
        let (before, was) = (lot(session), session.clone());
        let label = format!("{number} direct");
        let turn = step(cloud, Tier::Direct, session, tally, &label, ask.say).await?;
        println!("{:11}{}", "", main_branches(session));
        let after = lot(session);
        let parks = after.difference(&before).count();
        let returns = before.difference(&after).count();
        let checks = ask.holds.iter().chain(ask.then);
        let mut found: Vec<(bool, String)> = checks
            .map(|(what, holds)| (holds(session), (*what).to_owned()))
            .collect();
        found.push((
            parks <= ask.parks && returns <= ask.returns,
            format!("the rest of the parking lot left alone ({parks} parked, {returns} back)"),
        ));
        found.push((turn.unmet == 0, "no edit refused at the end".into()));
        if ask.lasting {
            found.push((turn.remember, "taken as a standing preference".into()));
        }
        if ask.still {
            found.push((*session == was, "nothing changed or remembered".into()));
            found.push((!turn.note.trim().is_empty(), "said why".into()));
        }
        tell(&found);
        let failed = found.into_iter().filter(|(ok, _)| !ok);
        short.extend(failed.map(|(_, what)| format!("ask {number}: {what}")));
        for (asked, (what, holds)) in &standing {
            if !holds(session) {
                println!("{:11}UNDID ask {asked}: {what}", "");
                short.push(format!("ask {number} undid ask {asked}: {what}"));
            }
        }
        // Only what landed can be undone later.
        let kept = ask.holds.iter().filter(|(_, holds)| holds(session));
        standing.extend(kept.map(|check| (number, *check)));
    }
    // The steward comes after, and must leave standing what the user decided.
    println!();
    // The arrangement is the user's, and so is a name they gave. A name the directed turn
    // chose for them is still the steward's to correct.
    let seats = |s: &Session| -> Vec<String> {
        let roots = s.categories.iter().filter(|c| c.parent.is_none());
        roots.map(|c| c.id.clone()).collect()
    };
    let given = |s: &Session| -> Vec<(String, String)> {
        let said = |name: &str| ASKS.iter().any(|ask| ask.say.contains(name));
        let named = s.categories.iter().filter(|c| said(&c.name));
        named.map(|c| (c.id.clone(), c.name.clone())).collect()
    };
    let (was, arranged, named) = (pinned(session), seats(session), given(session));
    let turn = step(cloud, Tier::Steward, session, tally, "steward", "").await?;
    println!("{:11}{}", "", main_branches(session));
    let now = session.thoughts.len() + session.parked.len() + session.retired.len();
    let (loose, kept) = (vague(session), given(session));
    let found = [
        (
            seats(session) == arranged,
            "the main branches kept, in the order the user arranged them".to_owned(),
        ),
        (
            named.iter().all(|name| kept.contains(name)),
            format!(
                "the {} branch name(s) the user gave left alone",
                named.len()
            ),
        ),
        (
            loose.is_empty(),
            format!("no catch-all branch ({})", loose.join(", ")),
        ),
        (
            pinned(session) == was,
            format!(
                "the {} pinned thought(s) left where the user put them",
                was.len()
            ),
        ),
        (now == said, "nothing deleted".to_owned()),
    ];
    tell(&found);
    let failed = found.into_iter().filter(|(ok, _)| !ok);
    short.extend(failed.map(|(_, what)| format!("steward: {what}")));
    for (asked, (what, holds)) in &standing {
        if !holds(session) {
            println!("{:11}UNDID ask {asked}: {what}", "");
            short.push(format!("the steward undid ask {asked}: {what}"));
        }
    }
    println!(
        "{:11}the map refused the steward {} edit(s)",
        "",
        turn.refused.len()
    );
    Ok(short)
}

/// One instruction of the caller's own, then the steward's look at what it left, and a second
/// look with nothing new said. Whether the map is what was asked for is read by eye; only what
/// any instruction must hold is checked.
async fn asked(
    cloud: Cloud<'_>,
    session: &mut Session,
    tally: &mut Tally,
    ask: &str,
) -> Result<Vec<String>, String> {
    let seats = |s: &Session| -> Vec<String> {
        let roots = s.categories.iter().filter(|c| c.parent.is_none());
        roots.map(|c| c.id.clone()).collect()
    };
    let said = session.thoughts.len() + session.parked.len() + session.retired.len();
    println!("\nask        {ask}");
    let turn = step(cloud, Tier::Direct, session, tally, "direct", ask).await?;
    println!("{:11}{}", "", main_branches(session));
    // A branch the instruction made or named is the user's, whatever else the steward mends.
    let named = |s: &Session, id: &String| -> Option<(String, String)> {
        let branch = s.categories.iter().find(|c| c.id == *id);
        branch.map(|c| (c.id.clone(), c.name.clone()))
    };
    let made = |op: &Op| match op {
        Op::Branch { id, .. } | Op::Rename { branch: id, .. } => named(session, id),
        _ => None,
    };
    let made: Vec<(String, String)> = turn.ops.iter().filter_map(made).collect();
    // What hangs directly from each branch the instruction arranged: the steward's to leave.
    let arranged: Vec<String> = tally.stands.asked.keys().cloned().collect();
    let hangs = |s: &Session| -> Vec<(BTreeSet<u64>, BTreeSet<String>)> {
        let at = |id: &String| {
            let on = s
                .thoughts
                .iter()
                .filter(|t| t.category.as_ref() == Some(id));
            let under = s
                .categories
                .iter()
                .filter(|c| c.parent.as_ref() == Some(id));
            let under = under.map(|c| c.id.clone());
            (on.map(|t| t.id).collect(), under.collect())
        };
        arranged.iter().map(at).collect()
    };
    let left = hangs(session);
    step(cloud, Tier::Steward, session, tally, "steward", "").await?;
    println!("{:11}{}", "", main_branches(session));
    let settled = seats(session);
    let again = step(cloud, Tier::Steward, session, tally, "again", "").await?;
    println!("{:11}{}", "", main_branches(session));
    let now = session.thoughts.len() + session.parked.len() + session.retired.len();
    let kept = |(id, name): &(String, String)| named(session, id).is_some_and(|c| c.1 == *name);
    let found = [
        (turn.unmet == 0, "no edit refused at the end"),
        (
            made.iter().all(kept),
            "the steward kept the branches the instruction made",
        ),
        (
            hangs(session) == left,
            "the steward left what the instruction arranged as it was",
        ),
        (
            seats(session) == settled,
            "a second look left the main branches alone",
        ),
        (
            again.usage.calls == 1,
            "a second look took one call: nothing it had left was put to it again",
        ),
        (now == said, "nothing deleted"),
    ];
    for (ok, what) in found {
        println!("{:11}{} {what}", "", if ok { "ok  " } else { "FAIL" });
    }
    let failed = found.into_iter().filter(|(ok, _)| !ok);
    Ok(failed.map(|(_, what)| what.to_owned()).collect())
}

type Cloud<'a> = (&'a openrouter_rs::OpenRouterClient, &'a CloudConfig);
fn main_branches(session: &Session) -> String {
    let roots: Vec<String> = session
        .categories
        .iter()
        .filter(|c| c.parent.is_none())
        .map(|c| format!("{} ({})", c.name, health::weight(session, &c.id)))
        .collect();
    roots.join(" · ")
}
async fn talk(cloud: Cloud<'_>, args: &Args) -> Result<bool, String> {
    let none = BTreeSet::new();
    let (mut session, mut truth) = (Session::default(), BTreeMap::new());
    let mut tally = Tally {
        trace: args.trace,
        ..Tally::default()
    };
    // Whether the thought the speaker parked was parked when it arrived.
    let mut returned = None;
    let steward = args.mode != "place";
    if let Some(path) = &args.load {
        let saved = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let saved: serde_json::Value = serde_json::from_str(&saved).map_err(|e| e.to_string())?;
        session = serde_json::from_value(saved["session"].clone()).map_err(|e| e.to_string())?;
        for (id, name) in saved["truth"].as_object().ok_or("Saved run has no truth")? {
            let want = [On, Off, Aside, Defer, Back]
                .into_iter()
                .find(|w| Some(format!("{w:?}").as_str()) == name.as_str());
            truth.insert(
                id.parse::<u64>().map_err(|e| e.to_string())?,
                want.ok_or("Bad truth")?,
            );
        }
        returned = saved["returned"].as_bool();
    }
    let tally = &mut tally;
    let mut short = vec![];
    if args.mode == "steward" {
        step(cloud, Tier::Steward, &mut session, tally, "steward", "").await?;
    } else if let ("direct", Some(ask)) = (args.mode.as_str(), &args.ask) {
        short = asked(cloud, &mut session, tally, ask).await?;
    } else if args.mode == "direct" {
        if truth.len() != 36 || truth.get(&15) != Some(&Defer) {
            return Err("mode=direct needs a saved onboarding talk, or ask=INSTRUCTION".into());
        }
        short = direct(cloud, &mut session, tally).await?;
    } else {
        let (mut since, mut back) = (0, None);
        for (index, slice) in args.talk.iter().enumerate() {
            for (want, line) in *slice {
                let id = session.add(line).ok_or("An empty line in the talk")?;
                truth.insert(id, *want);
                if *want == Back {
                    back = Some(id);
                }
            }
            let label = format!("{} place", index + 1);
            let placed = step(cloud, Tier::Place, &mut session, tally, &label, "").await?;
            let mut call = placed.escalate;
            // The live loop goes again for whatever a turn left waiting.
            if !brief::waiting(&session, &none).is_empty() {
                let again = step(cloud, Tier::Place, &mut session, tally, "  again", "").await?;
                call = call.or(again.escalate);
            }
            if let (None, Some(id)) = (returned, back) {
                returned = Some(matches!(at(&session, id), At::Parked(_)));
            }
            let mut frame = 0;
            let mut keep = |session: &Session| -> Result<(), String> {
                if let Some(dir) = &args.frames {
                    let path = format!("{dir}/{:02}-{frame}.json", index + 1);
                    let saved = serde_json::to_string(session).map_err(|e| e.to_string())?;
                    std::fs::write(&path, saved).map_err(|e| format!("{path}: {e}"))?;
                    frame += 1;
                }
                Ok(())
            };
            keep(&session)?;
            since += 1;
            let last = index + 1 == args.talk.len();
            let due = last || since >= args.every || call.is_some() || health::strained(&session);
            if steward && due {
                let label = format!("{} steward", index + 1);
                step(cloud, Tier::Steward, &mut session, tally, &label, "").await?;
                println!("{:11}{}", "", main_branches(&session));
                keep(&session)?;
                since = 0;
            }
        }
    }
    if !args.quiet {
        println!();
        outline(&session);
    }
    // The instructions overrule the facilitator's truth, so a directed run is judged by them.
    let sound = if args.mode == "direct" {
        println!("\nshape      {:?}", health::shape(&session));
        for note in health::notes(&session) {
            println!("health     {note}");
        }
        match short.as_slice() {
            [] => println!("verdict    sound"),
            short => println!("verdict    SHORT: {}", short.join("; ")),
        }
        short.is_empty()
    } else {
        score(&session, &truth, returned, steward)
    };
    println!(
        "placer     {}\nsteward    {}\ndirector   {}\ntotals     {} call(s), {}→{} tokens, ${:.4}, {} regrouped by the steward, {} lost, {} asked again",
        seconds(&tally.place),
        seconds(&tally.steward),
        seconds(&tally.direct),
        tally.calls,
        tally.prompt,
        tally.completion,
        tally.cost,
        tally.churn,
        tally.lost,
        tally.garbled
    );
    if let Some(path) = &args.save {
        let names: BTreeMap<String, String> = truth
            .iter()
            .map(|(id, want)| (id.to_string(), format!("{want:?}")))
            .collect();
        let saved = serde_json::json!({"session": session, "truth": names, "returned": returned});
        std::fs::write(path, saved.to_string()).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(sound && tally.lost == 0)
}

fn main() -> Result<(), String> {
    let mut config = CloudConfig::from_env()?.ok_or("OpenRouter is not configured")?;
    let args = args(&mut config)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Could not start the runtime")?;
    let client = config.client()?;
    println!(
        "placer  {} (effort {:?})\nsteward {} (effort {:?}){}",
        config.place_model,
        config.place_effort,
        config.steward_model,
        config.steward_effort,
        if args.mode == "place" {
            ", not used"
        } else {
            ""
        }
    );
    if args.mode == "direct" {
        println!(
            "director {} (effort {:?})",
            config.direct_model, config.direct_effort
        );
    }
    let mut short = 0;
    for run in 1..=args.runs {
        println!("\n=== run {run} of {} ===", args.runs);
        short += usize::from(!runtime.block_on(talk((&client, &config), &args))?);
    }
    if short > 0 {
        return Err(format!("{short} of {} run(s) fell short", args.runs));
    }
    Ok(())
}
