use rune::{
    Color, Cx, El, Key, KeyEvent, LineCap, Path, ScrollSpec, Shape, Spring, TextEdit, TextStyle,
    Tx, canvas, col, el, row, text, text_input,
};
use std::path::PathBuf;
use tacit::{
    art::{self, Garden, Style},
    generated_art::{self, Gallery},
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
    art: Garden,
    gallery: Gallery,
    art_style: Style,
    art_active: bool,
    pub draft: TextEdit,
    selected: Option<u64>,
    history: Vec<Session>,
    path: Option<PathBuf>,
    _lease: Option<std::fs::File>,
    status: String,
    read_only: bool,
    story: bool,
    worker: Option<tacit::organizer::Worker>,
    reviewer: Option<tacit::organizer::Worker>,
    review_requested: Option<u64>,
    review_applied: Option<u64>,
    fast_finished: Option<u64>,
    review_due: f64,
    settling: f32,
    settled_at: f64,
    busy: bool,
    pulse: f32,
    revision: u64,
    requested: Option<u64>,
    observed: u64,
    due: f64,
    organizer_status: String,
    previewed_draft: String,
    revealed_selection: Option<u64>,
    recording_worker: Option<tacit::recording::Worker>,
    pending_recordings: std::collections::HashSet<u64>,
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
            art: Garden::default(),
            gallery: Gallery::default(),
            art_style: Style::default(),
            art_active: false,
            draft: TextEdit::new(""),
            selected: None,
            history: vec![],
            path: None,
            _lease: None,
            status: "Demo · changes are not saved".into(),
            read_only: false,
            story: false,
            worker: None,
            reviewer: None,
            review_requested: None,
            review_applied: None,
            fast_finished: None,
            review_due: 3.0,
            settling: 1.0,
            settled_at: -10.0,
            busy: false,
            pulse: 0.0,
            revision: 0,
            requested: None,
            observed: 0,
            due: 0.0,
            organizer_status: "Offline · local rules".into(),
            previewed_draft: String::new(),
            revealed_selection: None,
            recording_worker: None,
            pending_recordings: Default::default(),
            voice_worker: None,
            voice_state: VoiceState::Unavailable,
            voice_status: "Hex is not connected".into(),
            voice_started: 0.0,
            voice_seconds: 0,
        }
    }
    pub fn themed_demo() -> Self {
        let mut s = Self::demo();
        s.session.categories = [
            ("time", "Time to think"),
            ("decisions", "Clear ownership"),
            ("belonging", "Being included"),
        ]
        .into_iter()
        .map(|(id, name)| tacit::model::Category {
            motif: None,
            illustration: None,
            id: id.into(),
            name: name.into(),
        })
        .collect();
        for (thought, category) in s.session.thoughts.iter_mut().zip([
            "time",
            "decisions",
            "belonging",
            "decisions",
            "belonging",
            "time",
        ]) {
            thought.category = Some(category.into());
        }
        s.organizer_status = "✦ Whole map refined · your words preserved".into();
        s.art = Garden::settled(&s.session);
        s
    }
    pub fn generated_demo() -> Self {
        let mut s = Self::themed_demo();
        s.gallery.load_demo(&s.session);
        s
    }
    pub fn art_demo() -> Self {
        let mut s = Self::themed_demo();
        s.art = Garden::default();
        s
    }
    pub fn demo() -> Self {
        Self {
            art: Garden::settled(&Session::demo()),
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
                self.voice_status = "Hex ready · transcription stays local".into();
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
                    if self.read_only {
                        return;
                    }
                    self.checkpoint();
                    let id = self.session.add(&words).unwrap();
                    self.selected = Some(id);
                    self.save();
                    let request = tacit::recording::Request {
                        id,
                        text: words.trim().to_owned(),
                        context: self.session.clone(),
                    };
                    if self
                        .recording_worker
                        .as_ref()
                        .is_some_and(|w| w.send.send(request).is_ok())
                    {
                        self.pending_recordings.insert(id);
                        self.voice_status = "Recording saved · separating ideas in context…".into();
                    } else {
                        self.voice_status =
                            "Full recording kept · automatic splitting needs OpenRouter".into();
                    }
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
    fn receive_recording(&mut self, response: tacit::recording::Response) {
        if !self.pending_recordings.remove(&response.id) || self.read_only {
            return;
        }
        let Some(index) = self
            .session
            .thoughts
            .iter()
            .position(|t| t.id == response.id && t.text == response.text && !t.pinned)
        else {
            return;
        };
        match response.result {
            Ok(chunks)
                if !chunks.is_empty()
                    && chunks.iter().all(|c| !c.trim().is_empty())
                    && chunks.concat() == response.text =>
            {
                self.checkpoint();
                let count = chunks.len();
                self.session.thoughts[index].text = chunks[0].trim().into();
                self.session.thoughts[index].role = classify(&chunks[0]);
                self.session.thoughts[index].category = None;
                for (offset, chunk) in chunks.iter().enumerate().skip(1) {
                    self.session.add(chunk);
                    let thought = self.session.thoughts.pop().unwrap();
                    self.session.thoughts.insert(index + offset, thought);
                }
                self.session.interpretation = None;
                self.voice_status = format!("Recording → {count} thoughts · organizing in context");
                self.save();
            }
            Err(error) => self.voice_status = format!("{error} · full recording kept"),
            _ => self.voice_status = "Invalid split · full recording kept".into(),
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
        self.pending_recordings.clear();
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
    // Themes carry stable IDs; the original narrative lenses remain the offline fallback.
    let mut groups: Vec<(Option<String>, String, Role)> = if s.session.categories.is_empty() {
        Role::ALL
            .into_iter()
            .map(|r| (None, branch_name(r).into(), r))
            .collect()
    } else {
        s.session
            .categories
            .iter()
            .enumerate()
            .map(|(i, c)| (Some(c.id.clone()), c.name.clone(), Role::ALL[i % 4]))
            .collect()
    };
    if !s.session.categories.is_empty()
        && (s.session.thoughts.iter().any(|t| t.category.is_none())
            || !s.draft.text.trim().is_empty())
    {
        groups.push((None, "Taking root".into(), Role::Observation));
    }
    let matches = |t: &tacit::model::Thought, group: usize| {
        if s.session.categories.is_empty() {
            t.role == groups[group].2
        } else {
            t.category == groups[group].0
        }
    };
    let draft_group = if s.session.categories.is_empty() {
        Role::ALL
            .iter()
            .position(|r| *r == classify(&s.draft.text))
            .unwrap_or(0)
    } else {
        groups.len() - 1
    };
    let counts: Vec<usize> = (0..groups.len())
        .map(|i| {
            s.session.thoughts.iter().filter(|t| matches(t, i)).count()
                + usize::from(!s.draft.text.trim().is_empty() && i == draft_group)
        })
        .collect();
    let mut offsets = Vec::new();
    let mut height = 0.0f32;
    for pair in counts.chunks(2) {
        offsets.push(height + 55.0);
        height += (pair.iter().copied().max().unwrap_or(1).max(2) as f32 * 72.0 + 100.0).max(230.0);
    }
    height = height.max(460.0);
    let center = [width / 2.0, height / 2.0];
    let leaf_w = (width * 0.27).min(255.0);
    let mut drawing = Shape::new();
    let mut labels = Vec::new();
    for (group, (_, name, role)) in groups.iter().enumerate() {
        let role = *role;
        let left = group % 2 == 0;
        let first_y = offsets[group / 2];
        let top = first_y < center[1];
        let ink = color(role);
        let n = counts[group].max(1);
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
                .key(format!(
                    "theme-heading-{}",
                    groups[group]
                        .0
                        .clone()
                        .unwrap_or_else(|| format!("lens-{group}"))
                ))
                .absolute(heading_x, first_y - 45.0)
                .gap(8.0)
                .center_y()
                .child(canvas(illustration(role)).size(38.0, 34.0))
                .child(
                    label(name, 20.0, ink)
                        .font(TextStyle::new(20.0, 600.0))
                        .max_w(leaf_w - 44.0),
                ),
        );
        let mut entries: Vec<(Option<u64>, String)> = s
            .session
            .thoughts
            .iter()
            .filter(|t| matches(t, group))
            .map(|t| (Some(t.id), t.text.clone()))
            .collect();
        if !s.draft.text.trim().is_empty() && group == draft_group {
            entries.push((None, s.draft.text.clone()));
        }
        if entries.is_empty() {
            labels.push(
                label("Let a thought take root…", 13.0, MUTED)
                    .key(format!("empty-branch-{group}"))
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
                    ink.alpha((1.0 - s.settling) * 0.13)
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
    // Rounded, overlapping cloud lobes leave a calm interior for the title.
    let p = |x: f32, y: f32| [center[0] + x, center[1] + y];
    let cloud = Path::new()
        .move_to(p(-82.0, -43.0))
        .cubic_to(p(-88.0, -73.0), p(-54.0, -88.0), p(-32.0, -70.0))
        .cubic_to(p(-18.0, -103.0), p(33.0, -100.0), p(45.0, -69.0))
        .cubic_to(p(77.0, -83.0), p(105.0, -61.0), p(96.0, -33.0))
        .cubic_to(p(127.0, -17.0), p(122.0, 25.0), p(96.0, 36.0))
        .cubic_to(p(103.0, 69.0), p(66.0, 88.0), p(43.0, 73.0))
        .cubic_to(p(25.0, 96.0), p(-18.0, 96.0), p(-37.0, 75.0))
        .cubic_to(p(-66.0, 91.0), p(-99.0, 70.0), p(-94.0, 42.0))
        .cubic_to(p(-126.0, 30.0), p(-126.0, -13.0), p(-101.0, -27.0))
        .cubic_to(p(-100.0, -38.0), p(-92.0, -43.0), p(-82.0, -43.0))
        .close();
    drawing = drawing
        .fill_path(&cloud, WHITE)
        .stroke_path(&cloud, 2.5, LineCap::Round, GOLD);
    let mut sheet = el().key("tree-sheet").w(width).h(height);
    let mut art_layer = el()
        .key("background-art")
        .absolute(0.0, 0.0)
        .size(width, height);
    if s.art_style != Style::Off {
        let size = (width / 2.0 - leaf_w - 94.0).clamp(56.0, 124.0);
        for layer in &s.art.layers {
            if cx.is_reduced_motion() && layer.retiring() {
                continue;
            }
            let slot = layer.concept.slot;
            let left = slot % 2 == 0;
            let x = if left {
                leaf_w + 25.0
            } else {
                width - leaf_w - 25.0 - size
            };
            let y = offsets.get(slot / 2).copied().unwrap_or(height - 180.0)
                - if slot < 2 { 20.0 } else { 70.0 };
            let pigment = color(Role::ALL[slot % 4]);
            let accent = color(Role::ALL[(slot + 1) % 4]);
            let progress = if cx.is_reduced_motion() {
                1.0
            } else {
                layer.reveal(s.art.now)
            };
            let opacity = if cx.is_reduced_motion() {
                1.0
            } else {
                layer.opacity(s.art.now)
            };
            art_layer = art_layer.child(
                canvas(art::draw(
                    layer.concept.motif,
                    s.art_style,
                    pigment,
                    accent,
                    size,
                    progress,
                ))
                .key(format!("theme-art-{}", layer.serial))
                .absolute(x, y)
                .size(size, size)
                .opacity(opacity * (1.0 - s.gallery.coverage(&layer.concept.key)))
                .motion(Spring::new(1.2, 0.05)),
            );
        }
    }
    if s.art_style != Style::Off {
        let size = (width / 2.0 - leaf_w - 94.0).clamp(56.0, 140.0);
        for layer in &s.gallery.layers {
            if cx.is_reduced_motion() && layer.retired.is_some() {
                continue;
            }
            let slot = layer.slot;
            let x = if slot % 2 == 0 {
                leaf_w + 25.0
            } else {
                width - leaf_w - 25.0 - size
            };
            let y = offsets.get(slot / 2).copied().unwrap_or(height - 180.0)
                - if slot < 2 { 20.0 } else { 70.0 };
            let progress = if cx.is_reduced_motion() {
                1.0
            } else {
                layer.progress(s.gallery.now)
            };
            let opacity = if cx.is_reduced_motion() {
                1.0
            } else {
                layer.opacity(s.gallery.now)
            };
            let art = el()
                .key(format!("generated-{}-{}", layer.key, layer.fingerprint))
                .absolute(x, y)
                .size(size, size)
                .motion(Spring::new(1.2, 0.05))
                .opacity(opacity * progress.min(0.4) / 0.4)
                .child(
                    el().absolute(0.0, 0.0)
                        .size(size, size * progress)
                        .clip_fade(0.0, 0.0, 0.0, if progress < 1.0 { 16.0 } else { 0.0 })
                        .child(
                            el().pixels(layer.image.pixels())
                                .absolute(0.0, 0.0)
                                .size(size, size),
                        ),
                );
            art_layer = art_layer.child(art);
        }
    }
    sheet = sheet.child(art_layer).child(
        canvas(drawing)
            .key("tree-branches")
            .size(width, height)
            .opacity(if cx.is_reduced_motion() {
                1.0
            } else {
                0.35 + 0.65 * s.settling
            }),
    );
    for node in labels {
        sheet = sheet.child(node);
    }
    sheet = sheet.child(
        col()
            .key("central-idea")
            .absolute(center[0] - 78.0, center[1] - 62.0)
            .w(156.0)
            .h(124.0)
            .center()
            .gap(5.0)
            .child(canvas(illustration(Role::Observation)).size(32.0, 26.0))
            .child(
                label(short_label(&s.session.title), 16.0, INK).font(TextStyle::new(16.0, 600.0)),
            )
            .child(
                label(
                    if s.busy {
                        "✦  FINDING CONNECTIONS"
                    } else {
                        "THE CENTRAL IDEA"
                    },
                    9.0,
                    GOLD,
                )
                .opacity(if s.busy && !cx.is_reduced_motion() {
                    0.55 + 0.45 * s.pulse
                } else {
                    1.0
                }),
            ),
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
        .child(button(s.art_style.label(), "art-style", |s| {
            s.art_style = s.art_style.next()
        }))
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
            } else if !s.session.categories.is_empty() {
                "Taking root · keep this thought to find its theme".into()
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
                    format!(
                        "{}  ·  {}{}",
                        s.status,
                        s.organizer_status,
                        if s.gallery.status.is_empty() {
                            String::new()
                        } else {
                            format!("  ·  {}", s.gallery.status)
                        }
                    ),
                    11.0,
                    MUTED,
                )
                .grow(1.0),
            )
            .child(if s.gallery.worker.is_some() {
                button("Retry art", "retry-art", |s| s.gallery.retry())
            } else {
                el()
            })
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
            .child(label(s.session.placement(t), 12.0, MUTED))
            .child(button("Move to next branch", "move", State::move_selected))
            .child(button("Remove thought", "remove", State::remove_selected));
    } else {
        panel = panel.child(label(
            "Select a thought to inspect its original words or change where it belongs.",
            14.0,
            MUTED,
        ));
    }
    let selected_theme = s
        .session
        .thoughts
        .iter()
        .find(|t| Some(t.id) == s.selected)
        .and_then(|t| t.category.as_ref())
        .map(|id| format!("theme-{id}"));
    if s.art_style != Style::Off
        && let Some(art) = s.gallery.layers.iter().rev().find(|l| {
            l.retired.is_none() && selected_theme.as_ref().is_none_or(|key| &l.key == key)
        })
    {
        panel = panel.child(label("VISUAL THREAD", 10.0, MUTED)).child(
            el().key("art-detail")
                .pixels(art.image.pixels())
                .size(230.0, 230.0),
        );
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
            "Follow a colored branch from the central idea to your own words.",
            12.0,
            MUTED,
        ))
        .child(label(
            if s.session.categories.is_empty() {
                "Local rules suggest a starting point. You have the final say."
            } else {
                "Themes are suggestions. Move a thought to pin its place."
            },
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
                    format!("Selected {:02} · {}", t.id, s.session.placement(t)),
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
    let backend = std::env::var("TACIT_ORGANIZER").unwrap_or_default();
    if backend == "offline" {
        return;
    }
    let cloud = if backend == "ollama" {
        Ok(None)
    } else {
        tacit::organizer::CloudConfig::from_env()
    };
    match cloud {
        Ok(Some(config)) => {
            s.organizer_status = "OpenRouter · quick themes + thoughtful review".into();
            if std::env::var("TACIT_IMAGE_ART").as_deref() != Ok("off") {
                s.gallery.worker = Some(generated_art::Worker::start(
                    config.clone(),
                    s.gallery.model.clone(),
                    wake.clone(),
                ));
                s.gallery.status = "Art · ready to illustrate themes".into();
            }
            let recording_wake = wake.clone();
            s.recording_worker = Some(tacit::recording::Worker::start(config.clone(), move || {
                recording_wake()
            }));
            let fast_wake = wake.clone();
            s.worker = Some(config.start(false, move || fast_wake()));
            s.reviewer = Some(config.start(true, move || wake()));
            return;
        }
        Err(error) => {
            s.organizer_status = error;
            return;
        }
        Ok(None) => {}
    }
    if let Ok(model) = std::env::var("TACIT_MODEL")
        && !model.trim().is_empty()
    {
        s.organizer_status = format!("Local model: {model}");
        s.worker = Some(tacit::organizer::Worker::start(model, move || wake()));
    }
}
pub fn poll(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    let art_deadline = s.art.update(&s.session, now);
    let image_changed = s.gallery.update(&s.session, s.art_style, s.busy, now, tx);
    let mut changed = image_changed || s.art_active || art_deadline.is_some();
    s.art_active = art_deadline.is_some();
    if s.art_style != Style::Off
        && !s.story
        && let Some(delay) = art_deadline
    {
        tx.wake_after(std::time::Duration::from_secs_f64(delay));
    }
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
    while let Some(response) = s
        .recording_worker
        .as_ref()
        .and_then(|w| w.receive.try_recv().ok())
    {
        s.receive_recording(response);
        changed = true;
    }
    if !s.pending_recordings.is_empty() {
        return changed;
    }
    if s.worker.is_none() {
        return changed;
    }
    let progress = ((now - s.settled_at) / 0.85).clamp(0.0, 1.0) as f32;
    if s.settling != progress {
        s.settling = progress;
        changed = true;
    }
    if s.settling < 1.0 || s.busy {
        s.pulse = ((now * 3.0).sin() as f32 + 1.0) / 2.0;
        tx.wake_after(std::time::Duration::from_millis(33));
        changed = true;
    }
    while let Some(response) = s.reviewer.as_ref().and_then(|w| w.receive.try_recv().ok()) {
        if response.revision == s.revision {
            s.busy = false;
            match response.result.and_then(|i| i.apply(&mut s.session)) {
                Ok(()) => {
                    s.review_applied = Some(s.revision);
                    s.organizer_status = "✦ Whole map refined · your words preserved".into();
                    s.settled_at = now;
                    s.settling = 0.0;
                    s.save();
                }
                Err(e) => s.organizer_status = format!("Review: {e} · keeping quick themes"),
            }
            changed = true;
        }
    }
    while let Some(response) = s.worker.as_ref().and_then(|w| w.receive.try_recv().ok()) {
        if response.revision == s.revision && s.review_applied != Some(s.revision) {
            s.fast_finished = Some(s.revision);
            s.busy = false;
            match response.result.and_then(|i| i.apply(&mut s.session)) {
                Ok(()) => {
                    s.organizer_status = if s.reviewer.is_some() {
                        "Quick themes ready · review follows after a pause"
                    } else {
                        "Organized by local model · suggestions"
                    }
                    .into();
                    s.settled_at = now;
                    s.settling = 0.0;
                    s.save();
                }
                Err(e) => s.organizer_status = format!("{e} · keeping local organization"),
            }
            changed = true;
        }
    }
    if s.observed != s.revision {
        s.observed = s.revision;
        s.due = now + 0.18;
        s.review_due = now + 3.0;
        s.busy = false;
    }
    if s.requested != Some(s.revision) && !s.session.thoughts.is_empty() {
        if now >= s.due {
            let request = tacit::organizer::Request {
                revision: s.revision,
                session: s.session.clone(),
            };
            if s.worker.as_ref().unwrap().send.send(request).is_ok() {
                s.organizer_status = "Finding a home for your thoughts…".into();
                s.busy = true;
            } else {
                s.organizer_status = "Organizer stopped · local rules remain available".into();
            }
            s.requested = Some(s.revision);
            changed = true;
        } else {
            tx.wake_after(std::time::Duration::from_secs_f64((s.due - now).max(0.01)));
        }
    }
    if let Some(reviewer) = &s.reviewer
        && s.review_requested != Some(s.revision)
        && s.fast_finished == Some(s.revision)
        && !s.session.thoughts.is_empty()
    {
        if now >= s.review_due {
            let request = tacit::organizer::Request {
                revision: s.revision,
                session: s.session.clone(),
            };
            if reviewer.send.send(request).is_ok() {
                s.organizer_status = "✦ Stepping back · refining the whole map…".into();
                s.busy = true;
            }
            s.review_requested = Some(s.revision);
            changed = true;
        } else {
            tx.wake_after(std::time::Duration::from_secs_f64(
                (s.review_due - now).max(0.01),
            ));
        }
    }
    if s.busy || s.settling < 1.0 {
        tx.wake_after(std::time::Duration::from_millis(33));
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
            categories: vec![],
            title: "Outdated title".into(),
            narrative: "Outdated interpretation".into(),
            assignments: before
                .thoughts
                .iter()
                .map(|t| tacit::model::Assignment {
                    id: t.id,
                    category: None,
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
    fn review_waits_for_fast_pass_and_quiet_period_and_rejects_old_edits() {
        use std::sync::mpsc;
        let mut s = State::demo();
        let (fast_send, fast_requests) = mpsc::channel();
        let (fast_response, fast_receive) = mpsc::channel();
        let (review_send, review_requests) = mpsc::channel();
        let (review_response, review_receive) = mpsc::channel();
        s.worker = Some(tacit::organizer::Worker {
            send: fast_send,
            receive: fast_receive,
        });
        s.reviewer = Some(tacit::organizer::Worker {
            send: review_send,
            receive: review_receive,
        });
        s.revision = 1;
        let mut tx = Tx::default();
        poll(&mut s, 0.0, &mut tx);
        assert!(fast_requests.try_recv().is_err());
        poll(&mut s, 0.2, &mut tx);
        assert_eq!(fast_requests.try_recv().unwrap().revision, 1);
        assert!(review_requests.try_recv().is_err());
        fast_response
            .send(tacit::organizer::Response {
                revision: 1,
                result: Err("fixture".into()),
            })
            .unwrap();
        poll(&mut s, 1.0, &mut tx);
        assert!(review_requests.try_recv().is_err());
        poll(&mut s, 3.1, &mut tx);
        assert_eq!(review_requests.try_recv().unwrap().revision, 1);
        s.revision = 2;
        let before = s.session.clone();
        review_response
            .send(tacit::organizer::Response {
                revision: 1,
                result: Err("stale review".into()),
            })
            .unwrap();
        poll(&mut s, 3.2, &mut tx);
        assert_eq!(s.session, before);
        assert!(!s.organizer_status.contains("stale review"));
        poll(&mut s, 3.4, &mut tx);
        assert_eq!(fast_requests.try_recv().unwrap().revision, 2);
    }
    #[test]
    fn background_art_style_cycles_without_changing_the_map() {
        let mut rt = runtime((820.0, 640.0));
        let before = rt.state.session.clone();
        let ids = rt
            .state
            .art
            .layers
            .iter()
            .map(|l| l.serial)
            .collect::<Vec<_>>();
        for expected in [Style::Chalk, Style::Marker, Style::Off, Style::Pastel] {
            click(&mut rt, "art-style");
            assert!(rt.state.art_style == expected);
            assert_eq!(rt.state.session, before);
        }
        rt.state.draft = TextEdit::new("Draft text does not trigger background redraws");
        rt.frame(10.0);
        assert_eq!(
            ids,
            rt.state
                .art
                .layers
                .iter()
                .map(|l| l.serial)
                .collect::<Vec<_>>()
        );
        assert!(!rt.state.art_active);
    }
    #[test]
    fn recording_split_keeps_order_is_undoable_and_ignores_cancelled_results() {
        let mut state = State::empty();
        state.receive_voice(
            VoiceEvent::Transcript("Meetings help. Budgets hurt.".into()),
            0.0,
        );
        let id = state.selected.unwrap();
        state.pending_recordings.insert(id);
        state.session.add("A later typed thought");
        state.receive_recording(tacit::recording::Response {
            id,
            text: "Meetings help. Budgets hurt.".into(),
            result: Ok(vec!["Meetings help. ".into(), "Budgets hurt.".into()]),
        });
        assert_eq!(
            state
                .session
                .thoughts
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            vec!["Meetings help.", "Budgets hurt.", "A later typed thought"]
        );
        state.session.validate().unwrap();
        state.undo();
        assert_eq!(
            state.session.thoughts[0].text,
            "Meetings help. Budgets hurt."
        );
        state.pending_recordings.insert(id);
        state.undo();
        let before = state.session.clone();
        state.receive_recording(tacit::recording::Response {
            id,
            text: "Meetings help. Budgets hurt.".into(),
            result: Ok(vec!["Meetings help. ".into(), "Budgets hurt.".into()]),
        });
        assert_eq!(state.session, before);
    }
    #[test]
    fn invalid_split_or_removed_recording_never_loses_words() {
        let mut state = State::empty();
        state.receive_voice(VoiceEvent::Transcript("Full original words".into()), 0.0);
        let id = state.selected.unwrap();
        state.pending_recordings.insert(id);
        state.receive_recording(tacit::recording::Response {
            id,
            text: "Full original words".into(),
            result: Ok(vec!["Made up text".into()]),
        });
        assert_eq!(state.session.thoughts[0].text, "Full original words");
        state.pending_recordings.insert(id);
        state.remove_selected();
        state.receive_recording(tacit::recording::Response {
            id,
            text: "Full original words".into(),
            result: Ok(vec!["Full original words".into()]),
        });
        assert!(state.session.thoughts.is_empty());
    }
    #[test]
    fn voice_result_preserves_typed_draft_and_saves_recording() {
        let mut state = State::demo();
        state.draft = TextEdit::new("My typed thought.");
        let before = state.session.clone();
        state.receive_voice(VoiceEvent::Transcript("My spoken thought.".into()), 0.0);
        assert_eq!(state.draft.text, "My typed thought.");
        assert_eq!(state.session.thoughts.len(), before.thoughts.len() + 1);
        assert_eq!(state.voice_state, VoiceState::Ready);
        state.capture();
        assert_eq!(state.session.thoughts.len(), before.thoughts.len() + 2);
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
        assert_eq!(rt.state.draft.text, "");
        assert!(
            rt.state
                .session
                .thoughts
                .iter()
                .any(|t| t.text == "A thought")
        );
    }
}
