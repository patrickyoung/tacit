use rune::{
    Color, Cx, El, Key, KeyEvent, LineCap, Path, ScrollSpec, Shape, Spring, TextEdit, TextStyle,
    Tx, canvas, col, el, row, text, text_input,
};
use std::path::PathBuf;
use tacit::{
    model::{Role, Session, classify},
    storage,
    voice::{self, Command as VoiceCommand, Event as VoiceEvent},
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
    r: 0.024,
    g: 0.31,
    b: 0.14,
    a: 1.0,
};
const GOLD: Color = Color {
    r: 0.72,
    g: 0.25,
    b: 0.015,
    a: 1.0,
};
const BLUE: Color = Color {
    r: 0.035,
    g: 0.2,
    b: 0.62,
    a: 1.0,
};
const RED: Color = Color {
    r: 0.65,
    g: 0.035,
    b: 0.1,
    a: 1.0,
};
const SPRING: Spring = Spring::new(0.45, 0.15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceState {
    Checking,
    Ready,
    Starting,
    Recording,
    Finishing,
    Cancelling,
    Unavailable,
}

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
    voice_worker: Option<voice::Worker>,
    voice_state: VoiceState,
    voice_status: String,
    voice_started: f64,
    voice_seconds: u64,
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
            voice_worker: None,
            voice_state: VoiceState::Unavailable,
            voice_status: "Hex is not connected".into(),
            voice_started: 0.0,
            voice_seconds: 0,
        }
    }
    pub fn demo() -> Self {
        Self {
            session: Session::demo(),
            ..Self::empty()
        }
    }
    fn voice_command(&mut self, command: VoiceCommand) {
        if self.read_only {
            return;
        }
        let allowed = match command {
            VoiceCommand::Check => matches!(
                self.voice_state,
                VoiceState::Ready | VoiceState::Unavailable
            ),
            VoiceCommand::Start => self.voice_state == VoiceState::Ready,
            VoiceCommand::Finish | VoiceCommand::Cancel => {
                self.voice_state == VoiceState::Recording
            }
        };
        if !allowed {
            return;
        }
        if self
            .voice_worker
            .as_ref()
            .is_none_or(|w| w.send.send(command).is_err())
        {
            self.voice_state = VoiceState::Unavailable;
            self.voice_status = "Hex connection is unavailable; restart Tacit.".into();
            return;
        }
        (self.voice_state, self.voice_status) = match command {
            VoiceCommand::Check => (VoiceState::Checking, "Connecting to Hex…".into()),
            VoiceCommand::Start => (VoiceState::Starting, "Opening the microphone…".into()),
            VoiceCommand::Finish => (
                VoiceState::Finishing,
                "Microphone stopping · transcribing with Hex…".into(),
            ),
            VoiceCommand::Cancel => (VoiceState::Cancelling, "Discarding recording…".into()),
        };
    }
    fn receive_voice(&mut self, event: VoiceEvent, now: f64) {
        match event {
            VoiceEvent::Ready => {
                self.voice_state = VoiceState::Ready;
                self.voice_status = "Hex ready · speech stays local".into();
            }
            VoiceEvent::Recording => {
                self.voice_state = VoiceState::Recording;
                self.voice_started = now;
                self.voice_seconds = 0;
                self.voice_status = "Listening with Hex".into();
            }
            VoiceEvent::Transcript(words) => {
                self.voice_state = VoiceState::Ready;
                if words.trim().is_empty() {
                    self.voice_status = "No speech heard. Try again.".into();
                } else {
                    let mut text = self.draft.text.clone();
                    if !text.is_empty() && !text.ends_with(char::is_whitespace) {
                        text.push('\n');
                    }
                    text.push_str(words.trim());
                    self.draft = TextEdit::new(text);
                    self.voice_status =
                        "Speech added to your draft · review, then Enter to keep".into();
                }
            }
            VoiceEvent::Cancelled => {
                self.voice_state = VoiceState::Ready;
                self.voice_status = "Recording discarded".into();
            }
            VoiceEvent::Error(message) => {
                self.voice_state = VoiceState::Unavailable;
                self.voice_status = message;
            }
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
fn branch_name(role: Role) -> &'static str {
    match role {
        Role::Observation => "Noticing",
        Role::Question => "Wondering",
        Role::Tension => "Tensions",
        Role::Possibility => "Possibilities",
    }
}
fn short_label(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 58 {
        flat
    } else {
        format!("{}…", flat.chars().take(55).collect::<String>())
    }
}
fn curve(mut shape: Shape, start: [f32; 2], end: [f32; 2], ink: Color, thick: f32) -> Shape {
    let dx = end[0] - start[0];
    let c1 = [start[0] + dx * 0.48, start[1]];
    let c2 = [end[0] - dx * 0.30, end[1]];
    let at = |t: f32| {
        let u = 1.0 - t;
        [
            u * u * u * start[0]
                + 3.0 * u * u * t * c1[0]
                + 3.0 * u * t * t * c2[0]
                + t * t * t * end[0],
            u * u * u * start[1]
                + 3.0 * u * u * t * c1[1]
                + 3.0 * u * t * t * c2[1]
                + t * t * t * end[1],
        ]
    };
    for i in 0..36 {
        let t = i as f32 / 36.0;
        shape = shape.line(
            at(t),
            at((i + 1) as f32 / 36.0),
            thick * (1.0 - t * 0.65),
            LineCap::Round,
            ink,
        );
    }
    shape
}
fn tree(s: &State, cx: &Cx<State>) -> El {
    let width = cx
        .layout_size("map")
        .map_or(920.0, |size| size[0])
        .max(700.0);
    let count = |role| {
        s.session.thoughts.iter().filter(|t| t.role == role).count()
            + usize::from(!s.draft.text.trim().is_empty() && classify(&s.draft.text) == role)
    };
    let upper = (count(Role::Observation).max(count(Role::Question)).max(2) as f32 * 72.0 + 70.0)
        .max(210.0);
    let lower =
        (count(Role::Tension).max(count(Role::Possibility)).max(2) as f32 * 72.0 + 70.0).max(210.0);
    let height = upper + lower + 60.0;
    let center = [width / 2.0, upper + 25.0];
    let leaf_w = (width * 0.27).min(255.0);
    let mut drawing = Shape::new();
    let mut labels = Vec::new();
    for (role, left, top) in [
        (Role::Observation, true, true),
        (Role::Question, false, true),
        (Role::Tension, true, false),
        (Role::Possibility, false, false),
    ] {
        let ink = color(role);
        let first_y = if top { 55.0 } else { upper + 105.0 };
        let n = count(role).max(1);
        let fork_y = first_y + ((n - 1) as f32 * 72.0) / 2.0 + 54.0;
        let fork_x = if left {
            leaf_w + 55.0
        } else {
            width - leaf_w - 55.0
        };
        let start = [
            center[0] + if left { -64.0 } else { 64.0 },
            center[1] + if top { -24.0 } else { 24.0 },
        ];
        drawing = curve(drawing, start, [fork_x, fork_y], ink, 9.0);
        let leaf_x = if left { 12.0 } else { width - leaf_w - 12.0 };
        let heading_x = if left { leaf_x + 10.0 } else { leaf_x };
        labels.push(
            row()
                .absolute(heading_x, first_y - 45.0)
                .gap(8.0)
                .center_y()
                .child(canvas(illustration(role)).size(38.0, 34.0))
                .child(label(branch_name(role), 22.0, ink).font(TextStyle::new(22.0, 600.0))),
        );
        let mut entries: Vec<(Option<u64>, String)> = s
            .session
            .thoughts
            .iter()
            .filter(|t| t.role == role)
            .map(|t| (Some(t.id), t.text.clone()))
            .collect();
        if !s.draft.text.trim().is_empty() && classify(&s.draft.text) == role {
            entries.push((None, s.draft.text.clone()));
        }
        if entries.is_empty() {
            labels.push(
                label("Let a thought take root…", 13.0, MUTED)
                    .w(leaf_w)
                    .absolute(leaf_x, first_y + 12.0),
            );
            drawing = curve(
                drawing,
                [fork_x, fork_y],
                [
                    if left {
                        leaf_x + 15.0
                    } else {
                        leaf_x + leaf_w - 15.0
                    },
                    first_y + 54.0,
                ],
                ink.alpha(0.4),
                2.5,
            );
        }
        for (index, (id, words)) in entries.into_iter().enumerate() {
            let y = first_y + index as f32 * 72.0;
            let end = [
                if left {
                    leaf_x + leaf_w - 5.0
                } else {
                    leaf_x + 5.0
                },
                y + 54.0,
            ];
            drawing = curve(drawing, [fork_x, fork_y], end, ink, 3.5);
            drawing = drawing.line(
                [leaf_x + 8.0, y + 54.0],
                [leaf_x + leaf_w - 8.0, y + 54.0],
                1.6,
                LineCap::Round,
                ink,
            );
            let selected = id.is_some() && id == s.selected;
            let mut node = col()
                .key(id.map_or_else(|| "draft-preview".into(), |id| format!("thought-{id}")))
                .absolute(leaf_x, y)
                .w(leaf_w)
                .h(53.0)
                .px(8.0)
                .py(3.0)
                .gap(2.0)
                .radius(8.0)
                .fill(if selected {
                    ink.alpha(0.10)
                } else {
                    Color::TRANSPARENT
                })
                .hover(ink.alpha(0.08))
                .motion(SPRING)
                .role("button")
                .focusable(id.is_some())
                .child(
                    label(
                        short_label(&words),
                        14.0,
                        if id.is_some() { INK } else { MUTED },
                    )
                    .wrap_anywhere(),
                )
                .child(label(
                    id.map_or_else(
                        || "Growing · Enter to keep".into(),
                        |id| {
                            format!(
                                "{:02}{}",
                                id,
                                if s.session.thoughts.iter().any(|t| t.id == id && t.pinned) {
                                    " · pinned"
                                } else {
                                    ""
                                }
                            )
                        },
                    ),
                    10.0,
                    ink,
                ));
            if let Some(id) = id {
                node = node.on_press(move |s: &mut State| s.selected = Some(id));
            }
            labels.push(node);
        }
    }
    let halo = Path::new()
        .move_to([center[0] - 88.0, center[1] - 42.0])
        .cubic_to(
            [center[0] - 105.0, center[1] - 100.0],
            [center[0] + 95.0, center[1] - 100.0],
            [center[0] + 90.0, center[1] - 20.0],
        )
        .cubic_to(
            [center[0] + 111.0, center[1] + 84.0],
            [center[0] - 106.0, center[1] + 95.0],
            [center[0] - 88.0, center[1] - 42.0],
        )
        .close();
    drawing = drawing
        .fill_path(&halo, WHITE)
        .stroke_path(&halo, 2.5, LineCap::Round, GOLD);
    let mut sheet = el()
        .key("tree-sheet")
        .w(width)
        .h(height)
        .child(canvas(drawing).size(width, height));
    for node in labels {
        sheet = sheet.child(node);
    }
    sheet = sheet.child(
        col()
            .key("central-idea")
            .absolute(center[0] - 78.0, center[1] - 65.0)
            .w(156.0)
            .h(136.0)
            .center()
            .gap(7.0)
            .child(canvas(illustration(Role::Observation)).size(40.0, 34.0))
            .child(
                label(short_label(&s.session.title), 18.0, INK).font(TextStyle::new(18.0, 600.0)),
            )
            .child(label("THE CENTRAL IDEA", 9.0, GOLD)),
    );
    col()
        .key("map")
        .w(0.0)
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .h_full()
        .child(sheet)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
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
        .child(label("Let your thoughts branch out.", 20.0, INK))
        .child(label(
            format!(
                "{} thoughts  /  one living tree  /  follow a branch",
                s.session.thoughts.len()
            ),
            12.0,
            MUTED,
        ));
    let main = if s.story {
        col().key("story").grow(1.0).min_h(0.0).gap(20.0).padding(26.0)
            .child(label("THE THREAD SO FAR", 11.0, GREEN))
            .child(label(s.session.narrative(), 23.0, INK).selectable(true))
            .child(label(if s.session.interpretation.is_some() { "A model interpretation. Bracketed numbers refer to your original thoughts; relationships remain hypotheses." } else { "A reading order assembled from your words, not an inferred causal explanation." }, 12.0, MUTED))
            .scroll(ScrollSpec { stick_bottom: false })
    } else {
        tree(s, cx)
    };
    let mut workspace = row().h(0.0).grow(1.0).min_h(0.0).gap(28.0).child(main);
    if wide {
        workspace = workspace
            .child(el().w(1.0).h_full().fill(LINE))
            .child(inspector(s));
    }
    let draft_role = classify(&s.draft.text);
    let mut voice_controls = row().gap(8.0).center_y().child(
        label(
            if s.voice_state == VoiceState::Recording {
                format!(
                    "● Listening  {:02}:{:02}",
                    s.voice_seconds / 60,
                    s.voice_seconds % 60
                )
            } else {
                s.voice_status.clone()
            },
            12.0,
            if s.voice_state == VoiceState::Recording {
                RED
            } else {
                MUTED
            },
        )
        .grow(1.0)
        .min_w(0.0),
    );
    voice_controls = match s.voice_state {
        VoiceState::Ready => voice_controls.child(
            button("Listen", "voice-start", |s| {
                s.voice_command(VoiceCommand::Start)
            })
            .fill(PAPER),
        ),
        VoiceState::Recording => voice_controls
            .child(button("Cancel", "voice-cancel", |s| {
                s.voice_command(VoiceCommand::Cancel)
            }))
            .child(
                button("Stop & transcribe", "voice-stop", |s| {
                    s.voice_command(VoiceCommand::Finish)
                })
                .fill(PAPER),
            ),
        VoiceState::Unavailable => {
            voice_controls.child(button("Reconnect Hex", "voice-check", |s| {
                s.voice_command(VoiceCommand::Check)
            }))
        }
        _ => voice_controls,
    };
    let composer = col()
        .key("composer")
        .child(voice_controls)
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
    panel
        .child(el().grow(1.0))
        .child(label("An open reading", 15.0, GREEN))
        .child(label(
            "Each color is a lens. Follow a curved branch from the central idea to your own words.",
            12.0,
            MUTED,
        ))
        .child(label(
            "Local rules suggest a lens. You have the final say.",
            12.0,
            MUTED,
        ))
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
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
            "Select a branch to inspect its full thought or change its home.",
            11.0,
            MUTED,
        ));
    }
    if let Some(t) = s.session.thoughts.iter().find(|t| Some(t.id) == s.selected) {
        col().gap(6.0).child(r).child(
            label(t.text.clone(), 13.0, INK)
                .wrap_anywhere()
                .selectable(true)
                .max_h(64.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                }),
        )
    } else {
        r
    }
}
pub fn on_key(s: &mut State, event: &KeyEvent, _: &mut Tx) -> bool {
    match event.key {
        Key::Command('s') => {
            s.save();
            true
        }
        Key::Escape => {
            if s.voice_state == VoiceState::Recording {
                s.voice_command(VoiceCommand::Cancel);
            }
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
    let voice_wake = wake.clone();
    s.voice_worker = Some(voice::Worker::start(move || voice_wake()));
    s.voice_command(VoiceCommand::Check);
    if let Ok(model) = std::env::var("TACIT_MODEL")
        && !model.trim().is_empty()
    {
        s.organizer_status = format!("Local model: {model}");
        s.worker = Some(tacit::organizer::Worker::start(model, move || wake()));
    }
}
pub fn poll(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    while let Some(event) = s
        .voice_worker
        .as_ref()
        .and_then(|w| w.receive.try_recv().ok())
    {
        s.receive_voice(event, now);
        changed = true;
    }
    if s.voice_state == VoiceState::Recording {
        let elapsed = (now - s.voice_started).max(0.0) as u64;
        if elapsed != s.voice_seconds {
            s.voice_seconds = elapsed;
            changed = true;
        }
        tx.wake_after(std::time::Duration::from_millis(500));
    }
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
        // Let newly inserted controls finish their entry animation before hit testing.
        rt.frame(1.0);
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
    #[test]
    fn voice_result_preserves_a_typed_draft_and_waits_for_review() {
        let mut state = State::demo();
        state.draft = TextEdit::new("My typed thought.");
        let before = state.session.clone();
        state.receive_voice(VoiceEvent::Transcript("My spoken thought.".into()), 0.0);
        assert_eq!(state.draft.text, "My typed thought.\nMy spoken thought.");
        assert_eq!(state.session, before);
        assert_eq!(state.voice_state, VoiceState::Ready);
        state.capture();
        assert_eq!(state.session.thoughts.len(), before.thoughts.len() + 1);
    }
    #[test]
    fn voice_controls_record_stop_and_cancel_without_duplicate_commands() {
        use std::sync::mpsc;
        let mut rt = runtime((1280.0, 860.0));
        let (send, commands) = mpsc::channel();
        let (events, receive) = mpsc::channel();
        rt.state.voice_worker = Some(voice::Worker { send, receive });
        events.send(VoiceEvent::Ready).unwrap();
        rt.frame(0.0);
        click(&mut rt, "voice-start");
        assert_eq!(commands.try_recv().unwrap(), VoiceCommand::Start);
        rt.state.voice_command(VoiceCommand::Start);
        assert!(commands.try_recv().is_err());
        events.send(VoiceEvent::Recording).unwrap();
        rt.frame(0.0);
        click(&mut rt, "voice-stop");
        assert_eq!(commands.try_recv().unwrap(), VoiceCommand::Finish);
        events
            .send(VoiceEvent::Transcript("A thought".into()))
            .unwrap();
        rt.frame(0.0);
        click(&mut rt, "voice-start");
        let _ = commands.try_recv();
        events.send(VoiceEvent::Recording).unwrap();
        rt.frame(0.0);
        click(&mut rt, "voice-cancel");
        assert_eq!(commands.try_recv().unwrap(), VoiceCommand::Cancel);
        events.send(VoiceEvent::Cancelled).unwrap();
        assert_eq!(rt.state.draft.text, "A thought");
    }
}
