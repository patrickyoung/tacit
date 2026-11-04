mod ui;
fn main() {
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
