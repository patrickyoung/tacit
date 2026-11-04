mod ui;
fn main() {
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
        scenes: &[("demo", |s| *s = ui::State::demo())],
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
