use rune::{
    Color, Cx, El, Key, KeyEvent, LineCap, Path, ScrollSpec, Shape, Spring, TextEdit, TextStyle,
    Tx, canvas, col, el, row, text, text_input,
};
use std::path::PathBuf;
use tacit::{
    model::{Role, Session, classify},
    storage,
};

const PAPER: Color = Color {
    r: 0.911408,
    g: 0.890005,
    b: 0.807346,
    a: 1.0,
};
const INK: Color = Color {
    r: 0.033105,
    g: 0.043234,
    b: 0.039682,
    a: 1.0,
};
const MUTED: Color = Color {
    r: 0.154872,
    g: 0.162647,
    b: 0.132868,
    a: 1.0,
};
const LINE: Color = Color {
    r: 0.673860,
    g: 0.673860,
    b: 0.570482,
    a: 1.0,
};
const WHITE: Color = Color {
    r: 0.977402,
    g: 0.955105,
    b: 0.890005,
    a: 1.0,
};
const GREEN: Color = Color {
    r: 0.063724,
    g: 0.162647,
    b: 0.106539,
    a: 1.0,
};
const GOLD: Color = Color {
    r: 0.380056,
    g: 0.162647,
    b: 0.033105,
    a: 1.0,
};
const BLUE: Color = Color {
    r: 0.083535,
    g: 0.162647,
    b: 0.273838,
    a: 1.0,
};
const RED: Color = Color {
    r: 0.380056,
    g: 0.106539,
    b: 0.078288,
    a: 1.0,
};
const SPRING: Spring = Spring::new(0.45, 0.15);

pub struct State {
    pub session: Session,
    pub draft: TextEdit,
    selected: Option<u64>,
    history: Vec<Session>,
    path: Option<PathBuf>,
    _lease: Option<std::fs::File>,
    status: String,
    read_only: bool,
    story: bool,
    worker: Option<tacit::organizer::Worker>,
    revision: u64,
    requested: Option<u64>,
    observed: u64,
    due: f64,
    organizer_status: String,
    previewed_draft: String,
    revealed_selection: Option<u64>,
}
impl State {
    pub fn load() -> Self {
        if rune::extra_args().iter().any(|a| a == "--demo") {
            return Self::demo();
        }
        if rune::is_headless() {
            return Self::empty();
        }
        let path = storage::default_path();
        let lease = match storage::lock(&path) {
            Ok(lease) => lease,
            Err(e) => {
                return Self {
                    status: e.to_string(),
                    read_only: true,
                    ..Self::empty()
                };
            }
        };
        match storage::load(&path) {
            Ok(session) => Self {
                session,
                path: Some(path),
                _lease: Some(lease),
                status: "Saved on this device".into(),
                ..Self::empty()
            },
            Err(e) => Self {
                status: format!(
                    "Could not open {}: {e}. File left untouched.",
                    path.display()
                ),
                read_only: true,
                ..Self::empty()
            },
        }
    }
    fn empty() -> Self {
        Self {
            session: Session::default(),
            draft: TextEdit::new(""),
            selected: None,
            history: vec![],
            path: None,
            _lease: None,
            status: "Demo · changes are not saved".into(),
            read_only: false,
            story: false,
            worker: None,
            revision: 0,
            requested: None,
            observed: 0,
            due: 0.0,
            organizer_status: "Offline · local rules".into(),
            previewed_draft: String::new(),
            revealed_selection: None,
        }
    }
    pub fn demo() -> Self {
        Self {
            session: Session::demo(),
            ..Self::empty()
        }
    }
    fn checkpoint(&mut self) {
        self.revision += 1;
        self.history.push(self.session.clone());
        if self.history.len() > 100 {
            self.history.remove(0);
        }
    }
    fn save(&mut self) {
        if let Some(path) = &self.path {
            self.status = match storage::save(path, &self.session) {
                Ok(()) => "Saved on this device".into(),
                Err(e) => format!("Not saved: {e}. Use Retry save."),
            };
        }
    }
    fn capture(&mut self) {
        if self.read_only || self.draft.text.trim().is_empty() {
            return;
        }
        self.checkpoint();
        self.selected = self.session.add(&self.draft.text);
        self.draft = TextEdit::new("");
        self.save();
    }
    fn move_selected(&mut self) {
        if self.read_only {
            return;
        }
        if let Some(id) = self.selected {
            self.checkpoint();
            self.session.move_thought(id);
            self.save();
        }
    }
    fn remove_selected(&mut self) {
        if self.read_only {
            return;
        }
        if let Some(id) = self.selected.take() {
            self.checkpoint();
            self.session.thoughts.retain(|t| t.id != id);
            self.session.interpretation = None;
            self.save();
        }
    }
    fn undo(&mut self) {
        if let Some(previous) = self.history.pop() {
            self.revision += 1;
            self.session = previous;
            self.selected = None;
            self.save();
        }
    }
}
fn label(content: impl Into<String>, size: f32, color: Color) -> El {
    text(content)
        .font(TextStyle::new(size, 400.0))
        .color(color)
        .wrap()
}
fn button(content: &str, key: &str, action: impl Fn(&mut State) + 'static) -> El {
    label(content, 12.0, INK)
        .key(key)
        .role("button")
        .focusable(true)
        .px(12.0)
        .py(8.0)
        .radius(7.0)
        .hover(LINE)
        .on_press(action)
}
fn color(role: Role) -> Color {
    match role {
        Role::Observation => GREEN,
        Role::Question => BLUE,
        Role::Tension => RED,
        Role::Possibility => GOLD,
    }
}
fn name(role: Role) -> &'static str {
    match role {
        Role::Observation => "THE GROUND",
        Role::Question => "THE OPEN QUESTION",
        Role::Tension => "THE TENSION",
        Role::Possibility => "A WAY FORWARD",
    }
}

// Small, meaningful line drawings: seeds, a branching path, opposing forces, and a bridge.
fn illustration(role: Role) -> Shape {
    let c = color(role);
    let mut shape = Shape::new();
    match role {
        Role::Observation => {
            let stem = Path::new().move_to([34.0, 55.0]).cubic_to(
                [32.0, 36.0],
                [36.0, 26.0],
                [42.0, 11.0],
            );
            let leaf =
                Path::new()
                    .move_to([35.0, 37.0])
                    .cubic_to([8.0, 38.0], [9.0, 16.0], [35.0, 37.0]);
            let leaf2 =
                Path::new()
                    .move_to([36.0, 29.0])
                    .cubic_to([60.0, 28.0], [64.0, 9.0], [36.0, 29.0]);
            shape = shape
                .stroke_path(&stem, 1.7, LineCap::Round, c)
                .stroke_path(&leaf, 1.4, LineCap::Round, c)
                .stroke_path(&leaf2, 1.4, LineCap::Round, c)
                .line([14.0, 56.0], [55.0, 54.0], 1.4, LineCap::Round, c);
        }
        Role::Question => {
            let path = Path::new().move_to([32.0, 57.0]).cubic_to(
                [31.0, 39.0],
                [13.0, 32.0],
                [13.0, 12.0],
            );
            let branch =
                Path::new()
                    .move_to([29.0, 42.0])
                    .cubic_to([31.0, 24.0], [56.0, 30.0], [56.0, 9.0]);
            shape = shape
                .stroke_path(&path, 1.7, LineCap::Round, c)
                .stroke_path(&branch, 1.7, LineCap::Round, c)
                .stroke(vec![[7.0, 18.0], [13.0, 10.0], [20.0, 18.0]], 1.7, c)
                .stroke(vec![[49.0, 16.0], [56.0, 8.0], [62.0, 16.0]], 1.7, c);
        }
        Role::Tension => {
            shape = shape
                .stroke(
                    vec![[6.0, 17.0], [6.0, 48.0], [25.0, 48.0], [25.0, 17.0]],
                    1.6,
                    c,
                )
                .stroke(
                    vec![[45.0, 17.0], [45.0, 48.0], [64.0, 48.0], [64.0, 17.0]],
                    1.6,
                    c,
                )
                .line([17.0, 31.0], [54.0, 31.0], 1.4, LineCap::Round, c)
                .stroke(vec![[23.0, 26.0], [17.0, 31.0], [23.0, 36.0]], 1.4, c)
                .stroke(vec![[48.0, 26.0], [54.0, 31.0], [48.0, 36.0]], 1.4, c);
        }
        Role::Possibility => {
            let arch =
                Path::new()
                    .move_to([6.0, 44.0])
                    .cubic_to([20.0, 5.0], [51.0, 5.0], [65.0, 44.0]);
            shape = shape.stroke_path(&arch, 1.7, LineCap::Round, c).line(
                [7.0, 44.0],
                [64.0, 44.0],
                1.6,
                LineCap::Round,
                c,
            );
            for x in [19.0, 30.0, 41.0, 52.0] {
                shape = shape.line([x, 24.0], [x, 44.0], 1.1, LineCap::Round, c);
            }
            shape = shape.stroke(
                vec![
                    [4.0, 53.0],
                    [20.0, 51.0],
                    [36.0, 54.0],
                    [53.0, 51.0],
                    [66.0, 53.0],
                ],
                1.0,
                c,
            );
        }
    }
    shape.view_box(72.0, 64.0)
}
fn island(s: &State, role: Role) -> El {
    let accent = color(role);
    let mut notes = col().w_full().gap(8.0);
    for thought in s.session.thoughts.iter().filter(|t| t.role == role) {
        let id = thought.id;
        let selected = s.selected == Some(id);
        notes = notes.child(
            col()
                .key(format!("thought-{id}"))
                .w_full()
                .gap(4.0)
                .padding(10.0)
                .radius(6.0)
                .fill(if selected { WHITE } else { PAPER })
                .border(if selected { 1.0 } else { 0.0 }, accent)
                .hover(WHITE)
                .role("button")
                .focusable(true)
                .motion(SPRING)
                .on_press(move |s: &mut State| s.selected = Some(id))
                .child(label(thought.text.clone(), 15.0, INK).wrap_anywhere())
                .child(label(
                    format!(
                        "{:02}  ·  {}",
                        id,
                        if thought.pinned {
                            "placed by you"
                        } else if s.session.interpretation.is_some() {
                            "suggested by local model"
                        } else {
                            "suggested by local rules"
                        }
                    ),
                    10.0,
                    MUTED,
                )),
        );
        if let Some(interpretation) = &s.session.interpretation {
            for link in interpretation.relationships.iter().filter(|r| r.from == id) {
                let target = link.to;
                notes = notes.child(button(
                    &format!("↳ {} → {:02} ?", link.label, target),
                    &format!("link-{id}-{target}"),
                    move |s| s.selected = Some(target),
                ));
            }
        }
    }
    if !s.draft.text.trim().is_empty() && classify(&s.draft.text) == role {
        notes = notes.child(
            col()
                .key("draft-preview")
                .padding(10.0)
                .radius(6.0)
                .border(1.0, LINE)
                .gap(4.0)
                .child(label(s.draft.text.clone(), 15.0, MUTED).wrap_anywhere())
                .child(label("Taking shape · Enter to keep", 10.0, accent)),
        );
    }
    if s.session.thoughts.iter().all(|t| t.role != role)
        && (s.draft.text.trim().is_empty() || classify(&s.draft.text) != role)
    {
        let hint = match role {
            Role::Observation => "What keeps happening?",
            Role::Question => "What don't you understand yet?",
            Role::Tension => "Which needs pull in different directions?",
            Role::Possibility => "What might be worth trying?",
        };
        notes = notes.child(label(hint, 14.0, MUTED).padding(10.0));
    }
    col()
        .key(format!("island-{role:?}"))
        .w(0.0)
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .gap(10.0)
        .child(
            row()
                .gap(12.0)
                .center_y()
                .child(canvas(illustration(role)).size(58.0, 52.0))
                .child(
                    col()
                        .gap(6.0)
                        .grow(1.0)
                        .min_w(0.0)
                        .child(label(name(role), 10.0, accent))
                        .child(label(role.label(), 21.0, INK)),
                ),
        )
        .child(el().w_full().h(1.0).fill(LINE))
        .child(
            notes
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .grow(1.0)
                .min_h(0.0),
        )
}
fn connector() -> El {
    let p = Path::new()
        .move_to([3.0, 12.0])
        .cubic_to([20.0, 0.0], [38.0, 25.0], [59.0, 12.0]);
    canvas(
        Shape::new()
            .stroke_path(&p, 1.1, LineCap::Round, MUTED)
            .stroke(vec![[52.0, 9.0], [59.0, 12.0], [55.0, 18.0]], 1.1, MUTED)
            .view_box(64.0, 26.0),
    )
    .size(42.0, 24.0)
}
pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let wide = cx.layout_size("root").is_none_or(|size| size[0] >= 1080.0);
    let header = row()
        .w_full()
        .center_y()
        .gap(16.0)
        .child(label("tacit", 32.0, INK).font(TextStyle::new(32.0, 500.0).tracking(-1.5)))
        .child(label("A FIELD GUIDE TO YOUR THINKING", 10.0, MUTED).grow(1.0))
        .child(button(
            if s.story {
                "Show map"
            } else {
                "Read the thread"
            },
            "toggle-story",
            |s| s.story = !s.story,
        ))
        .child(button("Undo", "undo", State::undo));
    let title = col()
        .gap(8.0)
        .child(label(s.session.title.clone(), 29.0, INK))
        .child(label(
            format!(
                "{} thoughts  /  four lenses  /  no need for a tidy conclusion",
                s.session.thoughts.len()
            ),
            12.0,
            MUTED,
        ));
    let map = col()
        .key("map")
        .grow(1.0)
        .min_h(0.0)
        .gap(24.0)
        .child(
            row()
                .h(0.0)
                .grow(1.0)
                .min_h(0.0)
                .gap(22.0)
                .min_h(220.0)
                .child(island(s, Role::Observation))
                .child(col().h_full().center().child(connector()))
                .child(island(s, Role::Question)),
        )
        .child(
            row()
                .h(0.0)
                .grow(1.0)
                .min_h(0.0)
                .gap(22.0)
                .min_h(220.0)
                .child(island(s, Role::Tension))
                .child(col().h_full().center().child(connector()))
                .child(island(s, Role::Possibility)),
        );
    let main = if s.story {
        col().key("story").grow(1.0).min_h(0.0).gap(20.0).padding(26.0)
            .child(label("THE THREAD SO FAR", 11.0, GREEN))
            .child(label(s.session.narrative(), 23.0, INK).selectable(true))
            .child(label(if s.session.interpretation.is_some() { "A model interpretation. Bracketed numbers refer to your original thoughts; relationships remain hypotheses." } else { "A reading order assembled from your words, not an inferred causal explanation." }, 12.0, MUTED))
            .scroll(ScrollSpec { stick_bottom: false })
    } else {
        map.scroll(ScrollSpec {
            stick_bottom: false,
        })
    };
    let mut workspace = row().h(0.0).grow(1.0).min_h(0.0).gap(28.0).child(main);
    if wide {
        workspace = workspace
            .child(el().w(1.0).h_full().fill(LINE))
            .child(inspector(s));
    }
    let draft_role = classify(&s.draft.text);
    let composer = col()
        .key("composer")
        .gap(8.0)
        .padding(14.0)
        .fill(WHITE)
        .border(1.0, LINE)
        .radius(12.0)
        .child(
            row()
                .gap(12.0)
                .center_y()
                .child(
                    text_input(&s.draft, TextStyle::new(16.0, 400.0))
                        .key("thought-input")
                        .grow(1.0)
                        .min_w(0.0)
                        .h(44.0)
                        .color(INK)
                        .placeholder("A thought, a question, a loose thread…", MUTED)
                        .on_edit(|s: &mut State| &mut s.draft)
                        .on_submit(|s: &mut State, _| s.capture()),
                )
                .child(button("Keep thought ↗", "capture", State::capture).fill(PAPER)),
        )
        .child(label(
            if s.draft.text.trim().is_empty() {
                "Enter to keep  ·  Shift+Enter for a new line  ·  your words stay yours".into()
            } else {
                format!(
                    "Finding a home: {}  ·  suggestion from local rules",
                    draft_role.label()
                )
            },
            11.0,
            MUTED,
        ));
    let mut root = col()
        .key("root")
        .w_full()
        .h_full()
        .padding(28.0)
        .gap(22.0)
        .fill(PAPER)
        .child(header)
        .child(title)
        .child(workspace);
    if !wide {
        root = root.child(compact_inspector(s));
    }
    root.child(composer).child(
        row()
            .w_full()
            .gap(10.0)
            .center_y()
            .child(
                label(
                    format!("{}  ·  {}", s.status, s.organizer_status),
                    11.0,
                    MUTED,
                )
                .grow(1.0),
            )
            .child(button("Retry save", "save", State::save)),
    )
}
fn inspector(s: &State) -> El {
    let mut panel = col()
        .w(230.0)
        .h_full()
        .gap(16.0)
        .child(label("IN THE MARGIN", 10.0, MUTED))
        .child(label("Follow the thought.", 22.0, INK));
    if let Some(t) = s.session.thoughts.iter().find(|t| Some(t.id) == s.selected) {
        panel = panel
            .child(label(
                format!("ORIGINAL WORDS · {:02}", t.id),
                10.0,
                color(t.role),
            ))
            .child(
                label(t.text.clone(), 18.0, INK)
                    .wrap_anywhere()
                    .selectable(true),
            )
            .child(label(t.role.label(), 12.0, MUTED))
            .child(button("Move to next lens", "move", State::move_selected))
            .child(button("Remove thought", "remove", State::remove_selected));
    } else {
        panel = panel.child(label(
            "Select a thought to inspect its original words or change where it belongs.",
            14.0,
            MUTED,
        ));
    }
    if let Some(i) = &s.session.interpretation {
        panel = panel.child(label("POSSIBLE CONNECTIONS", 10.0, MUTED));
        for relation in &i.relationships {
            let from = relation.from;
            let to = relation.to;
            panel = panel
                .child(label(
                    format!("{from:02} → {to:02}  {}", relation.label),
                    12.0,
                    GREEN,
                ))
                .child(
                    row()
                        .gap(2.0)
                        .child(button("Source", &format!("source-{from}-{to}"), move |s| {
                            s.selected = Some(from)
                        }))
                        .child(button("Target", &format!("target-{from}-{to}"), move |s| {
                            s.selected = Some(to)
                        })),
                );
        }
    }
    panel.child(el().grow(1.0)).child(label("An open reading", 15.0, GREEN))
        .child(label("The arrows suggest a reading order. They do not claim that one thought causes another.", 12.0, MUTED))
        .child(label("Local rules suggest a lens. You have the final say.", 12.0, MUTED))
        .scroll(ScrollSpec { stick_bottom: false })
}
fn compact_inspector(s: &State) -> El {
    let mut r = row().gap(10.0).center_y();
    if let Some(t) = s.session.thoughts.iter().find(|t| Some(t.id) == s.selected) {
        r = r
            .child(
                label(
                    format!("Selected {:02} · {}", t.id, t.role.label()),
                    11.0,
                    MUTED,
                )
                .grow(1.0),
            )
            .child(button("Move lens", "move", State::move_selected))
            .child(button("Remove", "remove", State::remove_selected));
    } else {
        r = r.child(label(
            "Select a thought to move it. Arrows show reading order, not causation.",
            11.0,
            MUTED,
        ));
    }
    r
}
pub fn on_key(s: &mut State, event: &KeyEvent, _: &mut Tx) -> bool {
    match event.key {
        Key::Command('s') => {
            s.save();
            true
        }
        Key::Escape => {
            s.selected = None;
            true
        }
        _ => false,
    }
}

pub fn setup(s: &mut State, wake: rune::Waker) {
    if s.read_only || rune::is_headless() {
        return;
    }
    if let Ok(model) = std::env::var("TACIT_MODEL")
        && !model.trim().is_empty()
    {
        s.organizer_status = format!("Local model: {model}");
        s.worker = Some(tacit::organizer::Worker::start(model, move || wake()));
    }
}
pub fn poll(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    if s.previewed_draft != s.draft.text {
        s.previewed_draft.clone_from(&s.draft.text);
        if !s.draft.text.trim().is_empty() {
            tx.scroll_into_view("draft-preview");
        }
        changed = true;
    }
    if s.revealed_selection != s.selected {
        s.revealed_selection = s.selected;
        if let Some(id) = s.selected {
            tx.scroll_into_view(format!("thought-{id}"));
        }
        changed = true;
    }
    if s.worker.is_none() {
        return changed;
    }
    while let Some(response) = s.worker.as_ref().and_then(|w| w.receive.try_recv().ok()) {
        if response.revision == s.revision {
            match response.result.and_then(|i| i.apply(&mut s.session)) {
                Ok(()) => {
                    s.organizer_status = "Organized by local model · suggestions".into();
                    s.save();
                }
                Err(e) => s.organizer_status = format!("{e} · keeping local organization"),
            }
            changed = true;
        }
    }
    if s.observed != s.revision {
        s.observed = s.revision;
        s.due = now + 0.8;
    }
    if s.requested != Some(s.revision) && !s.session.thoughts.is_empty() {
        if now >= s.due {
            let request = tacit::organizer::Request {
                revision: s.revision,
                session: s.session.clone(),
            };
            if s.worker.as_ref().unwrap().send.send(request).is_ok() {
                s.organizer_status = "Finding the thread… local model is thinking".into();
            } else {
                s.organizer_status = "Organizer stopped · local rules remain available".into();
            }
            s.requested = Some(s.revision);
            changed = true;
        } else {
            tx.wake_after(std::time::Duration::from_secs_f64((s.due - now).max(0.01)));
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Mods, Runtime};
    fn runtime(size: (f32, f32)) -> Runtime<State> {
        let mut rt = Runtime::new(
            State::demo(),
            view,
            size,
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        rt.poll = Some(poll);
        rt.frame(0.0);
        rt
    }
    fn click(rt: &mut Runtime<State>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0);
    }
    #[test]
    fn typing_previews_then_enter_captures_without_losing_original_words() {
        let mut rt = runtime((1280.0, 860.0));
        click(&mut rt, "thought-input");
        rt.key(KeyEvent {
            key: Key::Text("What if we listened first?".into()),
            mods: Mods::default(),
        });
        rt.frame(0.0);
        assert_eq!(rt.state.session.thoughts.len(), 6);
        assert!(rt.find("draft-preview").is_some());
        rt.key(KeyEvent {
            key: Key::Enter,
            mods: Mods::default(),
        });
        rt.frame(1.0);
        assert_eq!(rt.state.session.thoughts.len(), 7);
        assert_eq!(rt.state.session.thoughts[6].role, Role::Possibility);
        assert!(rt.state.draft.text.is_empty());
        click(&mut rt, "undo");
        assert_eq!(rt.state.session.thoughts.len(), 6);
    }
    #[test]
    fn selection_correction_delete_and_undo_work_at_minimum_size() {
        let mut rt = runtime((820.0, 640.0));
        click(&mut rt, "thought-1");
        click(&mut rt, "move");
        assert_eq!(rt.state.session.thoughts[0].role, Role::Question);
        assert!(rt.state.session.thoughts[0].pinned);
        click(&mut rt, "remove");
        assert_eq!(rt.state.session.thoughts.len(), 5);
        click(&mut rt, "undo");
        assert_eq!(rt.state.session.thoughts.len(), 6);
        click(&mut rt, "toggle-story");
        assert!(rt.find("story").is_some());
    }
    #[test]
    fn stale_model_results_are_discarded_and_failures_preserve_the_session() {
        use std::sync::mpsc;
        let mut state = State::demo();
        let (request_tx, _requests) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        state.worker = Some(tacit::organizer::Worker {
            send: request_tx,
            receive: response_rx,
        });
        state.revision = 2;
        state.observed = 2;
        state.requested = Some(2);
        let before = state.session.clone();
        let proposal = tacit::model::Interpretation {
            title: "Outdated title".into(),
            narrative: "Outdated interpretation".into(),
            assignments: before
                .thoughts
                .iter()
                .map(|t| tacit::model::Assignment {
                    id: t.id,
                    role: Role::Question,
                })
                .collect(),
            relationships: vec![],
        };
        response_tx
            .send(tacit::organizer::Response {
                revision: 1,
                result: Ok(proposal),
            })
            .unwrap();
        assert!(!poll(&mut state, 1.0, &mut Tx::default()));
        assert_eq!(state.session, before);
        response_tx
            .send(tacit::organizer::Response {
                revision: 2,
                result: Err("test timeout".into()),
            })
            .unwrap();
        assert!(poll(&mut state, 2.0, &mut Tx::default()));
        assert_eq!(state.session, before);
        assert!(state.organizer_status.contains("test timeout"));
    }
}
