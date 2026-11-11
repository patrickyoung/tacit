//! Measures the listener: how a model cuts a finished recording into thoughts, and how fast.
//! The recordings are invented, so nothing private is sent. Uses your OpenRouter credits:
//! `cargo run --release --example split_eval -- model=openai/gpt-6-luna effort=off trace=1`.
use std::time::Instant;
use tacit::agent::{self, Op, Usage};
use tacit::model::{Role, Session};
use tacit::{organizer::CloudConfig, recording};

/// A recording as the thoughts a careful listener would hear in it, and whether a map is under way.
const CASES: &[(&str, bool, &[&str])] = &[
    (
        "one point",
        true,
        &[
            "So the thing I keep coming back to is that people sign up and then never connect their data, and I think that is because the connect button is hidden on the second screen, I mean I watched three people yesterday scroll straight past it.",
        ],
    ),
    (
        "claim with example, then a new topic",
        true,
        &[
            "Our support queue is getting slower every week. For example last Tuesday someone waited nine hours for a password reset.",
            "Separately, the sample dashboard confuses people because the numbers in it are obviously fake.",
        ],
    ),
    (
        "two proposals in one sentence, then a trade-off",
        true,
        &[
            "We could send the weekly update in writing beforehand,",
            "and another option is to cut the meeting down to fifteen minutes.",
            "Writing it up is cheaper, but only if people actually read it, which so far they have not.",
        ],
    ),
    (
        "an aside in the middle, then a return",
        true,
        &[
            "The trial is fourteen days and most people only log in twice, so they never reach the point where the product pays off.",
            "Oh, I need to move my car before six, remind me.",
            "Anyway, back to the trial, maybe the clock should only start once they have connected their data.",
        ],
    ),
    (
        "a ramble",
        false,
        &[
            "Okay so, let me think about the kitchen first. The big problem is the dishwasher sits right next to the stove so two people cannot work at the same time, they just bump into each other.",
            "And the counter space is tiny, well, not tiny, it is long enough but it is too shallow to put a chopping board down properly.",
            "What I do not know is whether we are allowed to move the plumbing, has anyone asked the landlord?",
            "One idea is to swap the dishwasher and the bin cupboard, which would not need any new pipes if the hose reaches.",
            "We should also talk about the lighting at some point, but not today.",
            "The other thing about the dishwasher is that it is really loud, you can hear it from the bedroom at night.",
        ],
    ),
    (
        "no punctuation",
        false,
        &[
            "so the first thing is the signup form asks for way too much and people just drop off right there on the phone number field",
            "and the second thing which is completely different is that our welcome emails are landing in spam for anyone on outlook",
        ],
    ),
    (
        "a question and its reason",
        true,
        &[
            "Do we even know how many trials come from the partner page, because if it is most of them then the generic welcome tour is aimed at the wrong people.",
        ],
    ),
];

fn map() -> Session {
    let mut session = Session::default();
    for text in [
        "People sign up but do not finish setting up.",
        "The welcome tour is too long.",
    ] {
        session.add(text);
    }
    let ops = [
        Op::Title {
            title: "Trial conversion barriers".into(),
        },
        Op::Branch {
            id: "setup".into(),
            name: "Setup".into(),
            parent: None,
        },
        Op::Branch {
            id: "guidance".into(),
            name: "Guidance".into(),
            parent: None,
        },
        Op::Place {
            thought: 1,
            branch: "setup".into(),
            concept: Some("Unfinished setup".into()),
            role: Some(Role::Observation),
        },
        Op::Place {
            thought: 2,
            branch: "guidance".into(),
            concept: Some("Tour too long".into()),
            role: Some(Role::Observation),
        },
    ];
    agent::replay(&session, &ops).expect("the fixture map is sound")
}
/// Word numbers where the thoughts end, the last one aside.
fn inner(chunks: &[String]) -> Vec<usize> {
    let mut at = 0;
    let ends = chunks.iter().map(|chunk| {
        at += chunk.split_whitespace().count();
        at
    });
    let mut ends: Vec<usize> = ends.collect();
    ends.pop();
    ends
}

fn main() -> Result<(), String> {
    let mut config = CloudConfig::from_env()?.ok_or("Set OPENROUTER_API_KEY or the key file")?;
    let mut trace = false;
    for arg in std::env::args().skip(1) {
        match arg.split_once('=') {
            Some(("model", value)) => config.assist_model = value.into(),
            Some(("effort", value)) => config.assist_effort = value.into(),
            Some(("trace", value)) => trace = value != "0",
            _ => return Err(format!("Unknown argument {arg}")),
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let runtime = runtime.map_err(|_| "Could not start a runtime")?;
    let client = config.client()?;
    println!(
        "listener {} (effort {:?})",
        config.assist_model, config.assist_effort
    );
    let (mut usage, mut times) = (Usage::default(), vec![]);
    let (mut hit, mut said, mut wanted, mut exact) = (0, 0, 0, 0);
    for (name, mapped, gold) in CASES {
        let text = gold.join(" ");
        let gold: Vec<String> = gold.iter().map(|unit| unit.to_string()).collect();
        let context = if *mapped { map() } else { Session::default() };
        let started = Instant::now();
        let raw = runtime.block_on(recording::hear(
            &client, &config, &text, &context, &mut usage,
        ));
        let seconds = started.elapsed().as_secs_f64();
        times.push(seconds);
        let chunks = raw.clone().and_then(|raw| recording::parse(&raw, &text));
        let (want, got) = (
            inner(&gold),
            chunks.as_deref().map(inner).unwrap_or_default(),
        );
        let same = got.iter().filter(|end| want.contains(end)).count();
        hit += same;
        said += got.len();
        wanted += want.len();
        exact += usize::from(chunks.is_ok() && got == want);
        let verdict = match &chunks {
            Err(error) => format!("FAILED {error}"),
            Ok(_) if got == want => "exact".into(),
            Ok(_) => format!("cuts {got:?}, wanted {want:?}"),
        };
        println!("{seconds:5.2}s  {name}: {verdict}");
        if trace {
            for chunk in chunks.iter().flatten() {
                println!("         | {}", chunk.trim());
            }
            println!("         raw {}", raw.unwrap_or_else(|error| error));
        }
    }
    times.sort_by(f64::total_cmp);
    println!(
        "totals: {exact}/{} exact · cuts {hit} right of {said} made, {wanted} wanted · median {:.2}s · slowest {:.2}s · ${:.4}",
        CASES.len(),
        times[times.len() / 2],
        times[times.len() - 1],
        usage.cost
    );
    Ok(())
}
