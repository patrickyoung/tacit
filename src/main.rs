mod ui;
fn main() {
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
    rune::run(rune::AppSpec {
        min_size: Some((820.0, 640.0)),
        scenes: &[
            ("demo", |s| *s = ui::State::demo()),
            ("themes", |s| *s = ui::State::themed_demo()),
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
    });
}
