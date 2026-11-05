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
            let mut session = tacit::model::Session::demo();
            let transcript = "We spend too long in meetings. We could collect updates beforehand so that time is used for decisions. On another topic, our hosting costs have doubled. We should audit unused servers. Coming back to meetings, having one owner for each decision would help too.";
            let chunks =
                runtime.block_on(tacit::recording::split(&config, transcript, &session))?;
            if chunks.concat() != transcript {
                return Err("Transcript preservation failed".into());
            }
            println!(
                "Recording split into {} thoughts; original words validated",
                chunks.len()
            );
            for chunk in &chunks {
                session.add(chunk);
            }
            runtime
                .block_on(config.organize(&session, false))?
                .apply(&mut session)?;
            for thought in session.thoughts.iter().skip(6) {
                println!("{}: {}", session.placement(thought), thought.text);
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
            let mut session = tacit::model::Session::demo();
            for review in [false, false, true] {
                if !review && !session.categories.is_empty() {
                    session.add("We could protect an hour each morning for focused work.");
                }
                let started = std::time::Instant::now();
                let result = runtime.block_on(config.organize(&session, review))?;
                result.apply(&mut session)?;
                println!(
                    "{}: {:.2}s, {} themes, {} thoughts validated",
                    if review { "Review" } else { "Fast" },
                    started.elapsed().as_secs_f32(),
                    session.categories.len(),
                    session.thoughts.len()
                );
            }
            Ok(())
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
