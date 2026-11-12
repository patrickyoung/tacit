mod capture;
mod ui;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if let Some(index) = args.iter().position(|a| a == "--export-video") {
        let result = (|| -> std::io::Result<()> {
            let path = args
                .get(index + 1)
                .ok_or_else(|| std::io::Error::other("--export-video needs SESSION OUTPUT.mkv"))?;
            let output = args
                .get(index + 2)
                .ok_or_else(|| std::io::Error::other("--export-video needs SESSION OUTPUT.mkv"))?;
            let journal = tacit::timeline::Journal::read(std::path::Path::new(path))?;
            tacit::video_archive::export(&journal, std::path::Path::new(output))?;
            println!("Original lossless video exported to {output}");
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(index) = args.iter().position(|a| a == "--check-audio") {
        let result = (|| -> Result<(), String> {
            let path = std::path::Path::new(
                args.get(index + 1)
                    .ok_or("--check-audio needs a 16 kHz mono WAV path")?,
            );
            let (text, cues) = tacit::voice::transcribe_file(path)?;
            println!(
                "Validated {} timed segments from saved audio ({} characters)",
                cues.len(),
                text.chars().count()
            );
            Ok(())
        })();
        if let Err(e) = result {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|a| a == "--check-recording") {
        let result = (|| -> Result<(), String> {
            let config = tacit::organizer::CloudConfig::from_env()?
                .ok_or("OpenRouter key is not configured")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            let client = config.client()?;
            let mut session = tacit::model::Session::demo();
            let transcript = "We spend too long in meetings. We could collect updates beforehand so that time is used for decisions. On another topic, our hosting costs have doubled. We should audit unused servers. Coming back to meetings, having one owner for each decision would help too.";
            let usage = &mut tacit::agent::Usage::default();
            let heard = tacit::recording::sift(&client, &config, transcript, &session, usage);
            let chunks = runtime.block_on(heard)?;
            if chunks.concat() != transcript {
                return Err("Transcript preservation failed".into());
            }
            println!(
                "Recording split into {} thoughts; original words validated",
                chunks.len()
            );
            let said = session.next_id;
            for chunk in &chunks {
                session.add(chunk);
            }
            settle(&runtime, &client, &config, &mut session)?;
            for thought in session.thoughts.iter().filter(|t| t.id >= said) {
                println!("{}: {}", session.placement_path(thought), thought.text);
            }
            for parked in session.parked.iter().filter(|p| p.thought.id >= said) {
                let (kind, text) = (parked.kind.label(), &parked.thought.text);
                println!("Parked, {kind} ({}): {text}", parked.topic);
            }
            // Said before: the words stay with the thought they were combined into.
            for gone in session.retired.iter().filter(|r| r.thought.id >= said) {
                let into = |id: &u64| session.thoughts.iter().find(|t| t.id == *id);
                let label = gone.replaced_by.iter().find_map(into);
                let label = label.map_or("another thought", |t| t.map_label());
                println!("Combined into {label}: {}", gone.thought.text);
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|a| a == "--check-image") {
        let result = tacit::organizer::CloudConfig::from_env()
            .and_then(|c| c.ok_or("OpenRouter key is not configured".into()))
            .and_then(|c| {
                tacit::generated_art::check_demo(&c, &ui::State::themed_demo().projection.session)
            });
        match result {
            Ok(path) => println!(
                "Generated demo art validated and cached: {}",
                path.display()
            ),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|a| a == "--check-openrouter") {
        let result = (|| -> Result<(), String> {
            let config = tacit::organizer::CloudConfig::from_env()?
                .ok_or("OpenRouter key is not configured")?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            let client = config.client()?;
            let mut session = tacit::model::Session::demo();
            settle(&runtime, &client, &config, &mut session)?;
            // What is said next is filed at once, and what the user asks for is done as asked.
            session.add("We could protect an hour each morning for focused work.");
            let place = tacit::agent::Tier::Place;
            turn(&runtime, &client, &config, place, &mut session, "")?;
            let asked = "Call this map Room for focused work.";
            let direct = tacit::agent::Tier::Direct;
            turn(&runtime, &client, &config, direct, &mut session, asked)?;
            filed(&session)
        })();
        if let Err(e) = result {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|a| a == "--check-hex") {
        match tacit::voice::check_installed() {
            Ok(()) => println!("Hex desktop capture is ready. Microphone was not opened."),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    rune::set_app_id("app.tacit.Desktop");
    rune::run_with(
        rune::AppSpec {
            min_size: Some((820.0, 640.0)),
            scenes: &[
                ("demo", |s| *s = ui::State::demo()),
                ("themes", |s| *s = ui::State::themed_demo()),
                ("hierarchy", |s| *s = ui::State::hierarchy_demo()),
                ("mapped", |s| *s = ui::State::mapped_demo()),
                ("parked", |s| *s = ui::State::parked_demo()),
                ("file", |s| *s = ui::State::file_demo()),
                ("art-draw", |s| *s = ui::State::art_demo()),
                ("generated", |s| *s = ui::State::generated_demo()),
            ],
            on_key: Some(ui::on_key),
            setup: Some(ui::setup),
            poll: Some(ui::poll),
            ..rune::AppSpec::new(
                "Tacit — a thought taking shape",
                (1280.0, 860.0),
                ui::State::load,
                ui::view,
            )
        },
        capture::Capture::default(),
    );
}

/// One turn of the map agent, asked of its tier's model. Its edits go onto the map the way
/// the journal replays them, so a turn the journal could not keep fails the check.
fn turn(
    runtime: &tokio::runtime::Runtime,
    client: &openrouter_rs::OpenRouterClient,
    config: &tacit::organizer::CloudConfig,
    tier: tacit::agent::Tier,
    session: &mut tacit::model::Session,
    instruction: &str,
) -> Result<(), String> {
    use tacit::agent::{guided, health, replay, turn};
    let holds = std::collections::BTreeSet::new();
    let stands = &mut Default::default();
    let asked = turn::run(client, config, tier, session, &holds, instruction, stands);
    let done = runtime.block_on(asked)?;
    *session = if tier.user() {
        guided(session, instruction, &done.ops)?
    } else {
        replay(session, &done.ops)?
    };
    let shape = health::shape(session);
    println!(
        "{}: {:.2}s over {} call(s), {} edit(s); {} thoughts on {} branches, {} waiting, {} parked",
        tier.name(),
        done.usage.elapsed.as_secs_f32(),
        done.usage.calls,
        done.ops.len(),
        shape.filed,
        shape.branches,
        shape.unfiled,
        shape.parked
    );
    Ok(())
}
/// File what waits, then step back and read the whole map, as the app does when the talk
/// pauses.
fn settle(
    runtime: &tokio::runtime::Runtime,
    client: &openrouter_rs::OpenRouterClient,
    config: &tacit::organizer::CloudConfig,
    session: &mut tacit::model::Session,
) -> Result<(), String> {
    use tacit::agent::Tier;
    turn(runtime, client, config, Tier::Place, session, "")?;
    turn(runtime, client, config, Tier::Steward, session, "")?;
    filed(session)
}
/// Every thought ends on a branch or in the parking lot.
fn filed(session: &tacit::model::Session) -> Result<(), String> {
    match tacit::agent::health::shape(session).unfiled {
        0 => Ok(()),
        left => Err(format!(
            "The map agent left {left} thought(s) without a branch"
        )),
    }
}
