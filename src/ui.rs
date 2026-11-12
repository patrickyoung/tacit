mod kit;
mod workspace;
use rune::{
    Color, Cx, El, Key, KeyEvent, LineCap, Path, ScrollSpec, Shape, TextEdit, TextStyle, Tx,
    canvas, col, el, row, text, text_input,
};
use std::path::PathBuf;
use tacit::{
    art::{self, Garden, Style},
    generated_art::{self, Gallery},
    model::{Role, Session, classify},
    session_event::SessionEvent,
    storage,
    voice::{self, Command as VoiceCommand, Event as VoiceEvent},
};

// Linear-light values. sRGB: paper #F4F1EA, ink #17211C, muted #5A645E, line #DDD7CA,
// surface #FFFDF8, green #17794A, gold #B45F0E, blue #2F5FD6, red #D0323E. Every text
// colour keeps at least 4.5:1 against the surface.
const PAPER: Color = Color {
    r: 0.904661,
    g: 0.879622,
    b: 0.822786,
    a: 1.0,
};
const INK: Color = Color {
    r: 0.008568,
    g: 0.015209,
    b: 0.011612,
    a: 1.0,
};
const MUTED: Color = Color {
    r: 0.102242,
    g: 0.127438,
    b: 0.111932,
    a: 1.0,
};
const LINE: Color = Color {
    r: 0.723055,
    g: 0.679542,
    b: 0.590619,
    a: 1.0,
};
const WHITE: Color = Color {
    r: 1.0,
    g: 0.982251,
    b: 0.938686,
    a: 1.0,
};
const GREEN: Color = Color {
    r: 0.008568,
    g: 0.191202,
    b: 0.068478,
    a: 1.0,
};
const GOLD: Color = Color {
    r: 0.456411,
    g: 0.114435,
    b: 0.004391,
    a: 1.0,
};
const BLUE: Color = Color {
    r: 0.028426,
    g: 0.114435,
    b: 0.672443,
    a: 1.0,
};
const RED: Color = Color {
    r: 0.630757,
    g: 0.031896,
    b: 0.048172,
    a: 1.0,
};

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

struct Replay {
    at_ms: u64,
    playing: bool,
    last_tick: Option<(f64, u64)>,
    last_projected: Option<u64>,
}
pub struct State {
    pub projection: tacit::timeline::Projection,
    library_base: Option<PathBuf>,
    session_entries: Vec<tacit::sessions::Entry>,
    session_name: String,
    session_name_edit: TextEdit,
    sessions_open: bool,
    wake: Option<rune::Waker>,
    journal: Option<tacit::timeline::Journal>,
    archive_only: bool,
    replay: Option<Replay>,
    pixel_replay: bool,
    pixel_image: Option<(String, rune::Image)>,
    last_pixels: Option<([u8; 32], tacit::timeline::PixelFrame)>,
    video_export: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
    video_destination_pending: bool,
    saved_video: Option<PathBuf>,
    replay_audio: Option<(String, rune::Player)>,
    replay_images: std::collections::HashMap<String, rune::Image>,
    image_cache: rune::ImageCache,
    art_assets: std::collections::HashMap<rune::ImageId, String>,
    modern_ui: bool,
    camera_zoom: rune::Live<f32>,
    camera_pan: rune::Live<[f32; 2]>,
    zoom_readout: rune::LiveText,
    map_size: std::cell::Cell<[f32; 2]>,
    framing: std::cell::Cell<workspace::Framing>,
    /// What the following camera last framed: the map, the canvas and the window.
    framed: std::cell::Cell<Option<[[f32; 2]; 3]>>,
    /// The camera is bound to a spring, to follow the map as it grows.
    gliding: std::cell::Cell<bool>,
    /// Which way each stroke of the map ran, and how often it has turned over.
    bearings: std::cell::RefCell<std::collections::HashMap<String, (bool, u32)>>,
    /// The maps a `file` scene has yet to show, and the second each is due.
    steps: std::collections::VecDeque<(f64, Session)>,
    drag_pan: [f32; 2],
    transcript_expanded: bool,
    transcript_following: bool,
    transcript_scroll: rune::Live<[f32; 2]>,
    map_scroll: rune::Live<[f32; 2]>,
    story_scroll: rune::Live<[f32; 2]>,
    inspector_scroll: rune::Live<[f32; 2]>,
    viewport: [f32; 2],
    map_viewport: [f32; 2],
    reduced_motion: bool,
    active_recording: Option<String>,
    live_text: String,
    art: Garden,
    gallery: Gallery,
    art_style: Style,
    art_active: bool,
    pub draft: TextEdit,
    direction_mode: bool,
    direction_draft: TextEdit,
    queued_direction: Option<String>,
    direction_status: String,
    selected: Option<u64>,
    history: Vec<Session>,
    path: Option<PathBuf>,
    _lease: Option<std::fs::File>,
    status: String,
    read_only: bool,
    story: bool,
    /// The map agent, when OpenRouter is configured; `worker` is the local model otherwise.
    agent: Option<tacit::agent::Worker>,
    pace: tacit::agent::pace::Pace,
    worker: Option<tacit::organizer::Worker>,
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
    pending_recordings: std::collections::BTreeSet<u64>,
    voice_worker: Option<voice::Worker>,
    voice_state: VoiceState,
    voice_status: String,
    voice_started: f64,
    voice_seconds: u64,
}
impl State {
    pub fn load() -> Self {
        if let Some(index) = rune::extra_args()
            .iter()
            .position(|a| a == "--capture-demo")
        {
            let Some(path) = rune::extra_args().get(index + 1) else {
                return Self {
                    read_only: true,
                    status: "--capture-demo needs a new session path".into(),
                    ..Self::empty()
                };
            };
            let path = std::path::PathBuf::from(path);
            let mut state = Self::demo();
            if tacit::timeline::root_for(&path).exists() {
                state.read_only = true;
                state.status = "Capture fixture path already exists".into();
                return state;
            }
            match tacit::timeline::Journal::open(&path, state.projection.session.clone()) {
                Ok(journal) => {
                    state.projection = journal.projection.clone();
                    state.journal = Some(journal);
                }
                Err(error) => {
                    state.read_only = true;
                    state.status = error.to_string();
                }
            }
            return state;
        }
        if rune::extra_args().iter().any(|a| a == "--demo") {
            return Self::demo();
        }
        let args = rune::extra_args();
        if let Some(index) = args.iter().position(|a| a == "--replay") {
            let loaded = args
                .get(index + 1)
                .ok_or_else(|| std::io::Error::other("--replay needs a session path"))
                .and_then(|path| tacit::timeline::Journal::read(std::path::Path::new(path)));
            return match loaded {
                Ok(journal) => {
                    if let Err(error) = journal.validate_replay() {
                        return Self {
                            read_only: true,
                            status: format!("Cannot replay exactly: {error}"),
                            ..Self::empty()
                        };
                    }
                    let at_ms = args
                        .iter()
                        .position(|a| a == "--replay-at")
                        .and_then(|i| args.get(i + 1))
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0)
                        .min(journal.duration())
                        .max(journal.first_frame_ms().unwrap());
                    let scene = journal
                        .replay_scene_at(at_ms)
                        .expect("validated presentation");
                    let mut s = Self {
                        journal: Some(journal),
                        read_only: true,
                        archive_only: true,
                        pixel_replay: args.iter().any(|a| a == "--replay-pixels"),
                        replay: Some(Replay {
                            at_ms,
                            playing: false,
                            last_tick: None,
                            last_projected: None,
                        }),
                        status: "Saved session · read-only replay".into(),
                        ..Self::empty()
                    };
                    s.restore_scene(scene, at_ms as f64 / 1000.0);
                    s
                }
                Err(e) => Self {
                    read_only: true,
                    status: format!("Cannot replay: {e}"),
                    ..Self::empty()
                },
            };
        }
        if rune::is_headless() {
            return Self::empty();
        }
        let path = storage::default_path();
        match Self::open_session(path.clone()) {
            Ok(mut state) => {
                state.library_base = Some(path);
                state.refresh_sessions();
                state
            }
            Err(e) => Self {
                library_base: Some(path),
                status: format!("Could not open session: {e}. Archive retained."),
                read_only: true,
                ..Self::empty()
            },
        }
    }
    fn open_session(path: PathBuf) -> std::io::Result<Self> {
        let lease = storage::lock(&path)?;
        let legacy = if tacit::timeline::root_for(&path)
            .join("events.jsonl")
            .exists()
        {
            Session::default()
        } else {
            storage::load(&path)?
        };
        let journal = tacit::timeline::Journal::open(&path, legacy)?;
        let scene = journal.projection.scene();
        let now = journal.now_ms() as f64 / 1000.0;
        let mut state = Self {
            journal: Some(journal),
            path: Some(path),
            _lease: Some(lease),
            status: "Saved on this device".into(),
            ..Self::empty()
        };
        state.restore_scene(scene, now);
        state
            .gallery
            .set_session_identity(state.path.as_ref().unwrap());
        Ok(state)
    }
    fn refresh_sessions(&mut self) {
        let Some(base) = &self.library_base else {
            return;
        };
        match tacit::sessions::list(base) {
            Ok(entries) => {
                if let Some(entry) = entries.iter().find(|e| Some(&e.path) == self.path.as_ref()) {
                    self.session_name = entry.name.clone();
                }
                self.session_entries = entries;
            }
            Err(e) => self.status = format!("Cannot list sessions: {e}"),
        }
    }
    fn can_switch_session(&mut self) -> bool {
        if matches!(
            self.voice_state,
            VoiceState::Starting
                | VoiceState::Recording
                | VoiceState::Finishing
                | VoiceState::Cancelling
        ) || !self.pending_recordings.is_empty()
            || self.video_busy()
            || self.directing()
            || self.queued_direction.is_some()
        {
            self.status =
                "Finish recording, transcription, map guidance, or video export before switching sessions".into();
            return false;
        }
        true
    }
    fn switch_session(&mut self, path: PathBuf) {
        if self.path.as_ref() == Some(&path) {
            self.sessions_open = false;
            return;
        }
        if !self.can_switch_session() {
            return;
        }
        // Open and lock the destination before releasing the current session.
        let mut next = match Self::open_session(path) {
            Ok(next) => next,
            Err(e) => {
                self.status = format!("Cannot open session: {e}");
                return;
            }
        };
        if self.replay.is_none() && !self.read_only {
            self.sessions_open = false;
            self.record_presentation("switch session");
            if self.read_only {
                return;
            }
        }
        if let Some(journal) = &mut self.journal
            && let Err(e) = journal.end_pixels()
        {
            self.status = format!("Cannot close capture: {e}");
            return;
        }
        next.library_base = self.library_base.clone();
        next.refresh_sessions();
        if let Some(wake) = self.wake.clone() {
            setup(&mut next, wake);
        }
        // Dropping the old workers also isolates late AI results from the new session.
        *self = next;
    }
    fn create_session(&mut self) {
        if !self.can_switch_session() {
            return;
        }
        let Some(base) = &self.library_base else {
            return;
        };
        let created = if self.session_name_edit.text.trim().is_empty() {
            tacit::sessions::create_automatic(base)
        } else {
            tacit::sessions::create(base, &self.session_name_edit.text)
        };
        match created {
            Ok(entry) => self.switch_session(entry.path),
            Err(e) => self.status = format!("Cannot create session: {e}"),
        }
    }
    fn rename_session(&mut self) {
        let (Some(base), Some(path)) = (&self.library_base, &self.path) else {
            return;
        };
        if self.read_only {
            return;
        }
        match tacit::sessions::rename(base, path, &self.session_name_edit.text) {
            Ok(()) => {
                self.refresh_sessions();
                self.status = "Session renamed".into();
            }
            Err(e) => self.status = format!("Cannot rename session: {e}"),
        }
    }
    fn empty() -> Self {
        Self {
            projection: Default::default(),
            library_base: None,
            session_entries: vec![],
            session_name: String::new(),
            session_name_edit: TextEdit::new(""),
            sessions_open: false,
            wake: None,
            journal: None,
            archive_only: false,
            replay: None,
            pixel_replay: false,
            pixel_image: None,
            video_export: None,
            video_destination_pending: false,
            saved_video: None,
            last_pixels: None,
            replay_audio: None,
            replay_images: Default::default(),
            art_assets: Default::default(),
            image_cache: rune::ImageCache::new(std::sync::Arc::new(|| {})),
            modern_ui: true,
            camera_zoom: rune::Live::new(1.0),
            camera_pan: rune::Live::new([0.0; 2]),
            zoom_readout: rune::LiveText::new("100%"),
            map_size: std::cell::Cell::new([1100.0, 600.0]),
            framing: std::cell::Cell::new(workspace::Framing::Follow),
            framed: Default::default(),
            gliding: Default::default(),
            bearings: Default::default(),
            steps: Default::default(),
            drag_pan: [0.0; 2],
            transcript_expanded: false,
            transcript_following: true,
            transcript_scroll: rune::Live::new([0.0; 2]),
            map_scroll: rune::Live::new([0.0; 2]),
            story_scroll: rune::Live::new([0.0; 2]),
            inspector_scroll: rune::Live::new([0.0; 2]),
            viewport: [1280.0, 860.0],
            map_viewport: [0.0; 2],
            reduced_motion: false,
            active_recording: None,
            live_text: String::new(),
            art: Garden::default(),
            gallery: Gallery::default(),
            art_style: Style::default(),
            art_active: false,
            draft: TextEdit::new(""),
            direction_mode: false,
            direction_draft: TextEdit::new(""),
            queued_direction: None,
            direction_status: "Guide the categories, structure, and placement of your ideas".into(),
            selected: None,
            history: vec![],
            path: None,
            _lease: None,
            status: "Demo · changes are not saved".into(),
            read_only: false,
            story: false,
            agent: None,
            pace: Default::default(),
            worker: None,
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
        s.projection.session.categories = [
            ("time", "Time to think"),
            ("decisions", "Clear ownership"),
            ("belonging", "Being included"),
        ]
        .into_iter()
        .map(|(id, name)| tacit::model::Category {
            parent: None,
            motif: None,
            illustration: None,
            id: id.into(),
            name: name.into(),
        })
        .collect();
        for (thought, category) in s.projection.session.thoughts.iter_mut().zip([
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
        s.art = Garden::settled(&s.projection.session);
        s
    }
    pub fn hierarchy_demo() -> Self {
        let mut s = Self::themed_demo();
        for (id, name, parent) in [
            ("meetings", "Meeting design", "time"),
            ("focus", "Protected focus", "meetings"),
            ("owners", "Decision owners", "decisions"),
        ] {
            s.projection
                .session
                .categories
                .push(tacit::model::Category {
                    id: id.into(),
                    name: name.into(),
                    parent: Some(parent.into()),
                    motif: None,
                    illustration: None,
                });
        }
        s.projection.session.thoughts[0].category = Some("focus".into());
        s.projection.session.thoughts[5].category = Some("meetings".into());
        s.projection.session.thoughts[3].category = Some("owners".into());
        s.art = Garden::settled(&s.projection.session);
        s
    }
    /// A map as the agent leaves it: keyword labels, a repeat combined with the thought it
    /// repeats, cross-links, a thread to read, and a parking lot of each kind.
    /// A map saved as JSON by `agent_eval frames=DIR`, named by `TACIT_SCENE_SESSION`, shown
    /// as a snapshot. Without one it is the plain demo. `TACIT_SCENE_STORY` names a directory
    /// written by `story_video` instead, and its maps are shown in turn, each at its time on
    /// the timeline.
    pub fn file_demo() -> Self {
        let mut s = Self::demo();
        let read = |path: PathBuf| std::fs::read_to_string(path).ok();
        if let Some(dir) = std::env::var_os("TACIT_SCENE_STORY").map(PathBuf::from) {
            let timeline = read(dir.join("timeline.json"))
                .and_then(|text| serde_json::from_str::<Vec<serde_json::Value>>(&text).ok());
            for step in timeline.unwrap_or_default() {
                let map = step["file"].as_str().and_then(|file| read(dir.join(file)));
                if let Some(session) = map.and_then(|text| serde_json::from_str(&text).ok()) {
                    s.steps
                        .push_back((step["at"].as_f64().unwrap_or(0.0), session));
                }
            }
            s.projection.session = s.steps.pop_front().map(|step| step.1).unwrap_or_default();
            return s;
        }
        let saved = std::env::var_os("TACIT_SCENE_SESSION")
            .and_then(|path| read(path.into()))
            .and_then(|text| serde_json::from_str(&text).ok());
        if let Some(session) = saved {
            s.projection.session = session;
        }
        s
    }
    pub fn mapped_demo() -> Self {
        let mut s = Self::demo();
        let session = &mut s.projection.session;
        for text in [
            "We spend too long in meetings.",
            "We could collect updates beforehand so that time is used for decisions.",
            "On another topic, our hosting costs have doubled.",
            "We should audit unused servers.",
            "Coming back to meetings, having one owner for each decision would help too.",
            "Hybrid days might change all of this.",
            "Hang on, is this still recording?",
        ] {
            session.add(text);
        }
        let place = |thought: u64, branch: &str, concept: &str, role: &str| {
            serde_json::json!({"op": "place", "thought": thought, "branch": branch,
                "concept": concept, "role": role})
        };
        let park = |thought: u64, kind: &str, topic: &str, concept: &str, reason: &str| {
            serde_json::json!({"op": "park", "thought": thought, "kind": kind, "topic": topic,
                "concept": concept, "reason": reason})
        };
        let ops = serde_json::json!([
            {"op": "focus", "focus": "Making room for meaningful work",
                "principle": "What crowds the week, then what would give the time back"},
            {"op": "branch", "id": "load", "name": "Meeting load"},
            {"op": "branch", "id": "inclusion", "name": "Inclusion"},
            {"op": "branch", "id": "ownership", "name": "Decision ownership"},
            {"op": "branch", "id": "preparation", "name": "Preparation"},
            place(1, "load", "Calendars fill up", "Observation"),
            place(7, "load", "Meetings run long", "Observation"),
            place(5, "inclusion", "Everyone invited", "Observation"),
            place(3, "inclusion", "Need to feel included", "Tension"),
            place(2, "ownership", "Who decides?", "Question"),
            place(4, "ownership", "One owner per decision", "Possibility"),
            place(6, "preparation", "Input before meetings", "Possibility"),
            place(8, "preparation", "Updates beforehand", "Possibility"),
            {"op": "combine", "into": 4, "from": [11], "concept": "One owner per decision"},
            park(9, "off_topic", "Hosting costs", "Hosting costs doubled",
                "A different subject from how the team meets and decides."),
            park(10, "off_topic", "Hosting costs", "Audit unused servers",
                "A different subject from how the team meets and decides."),
            park(12, "later", "Hybrid days", "Hybrid days",
                "No branch is about where people work yet."),
            park(13, "aside", "Recording check", "Recording check",
                "Housekeeping, with nothing to map."),
            {"op": "links", "links": [
                {"from": 5, "to": 3, "label": "driven by"},
                {"from": 2, "to": 4, "label": "answered by"}]},
            {"op": "narrative", "narrative": "Meetings take more of the week than they give \
                back [1][7], partly because everyone is invited just in case [5] so that nobody \
                feels left out [3]. Two things would give the time back: settling who decides \
                [2][4], and doing the preparation beforehand [6][8]."},
        ]);
        let ops: Vec<tacit::agent::Op> = serde_json::from_value(ops).expect("scene edits");
        *session = tacit::agent::replay(session, &ops).expect("scene map");
        s.organizer_status = "✦ Combined a repeated call for one owner per decision".into();
        s.art = Garden::settled(&s.projection.session);
        s
    }
    /// The same map with the parking lot open on what was parked last.
    pub fn parked_demo() -> Self {
        let mut s = Self::mapped_demo();
        s.selected = s.projection.session.parked.last().map(|p| p.thought.id);
        s
    }
    pub fn generated_demo() -> Self {
        let mut s = Self::themed_demo();
        s.gallery.load_demo(&s.projection.session);
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
            projection: tacit::timeline::Projection {
                session: Session::demo(),
                ..Default::default()
            },
            ..Self::empty()
        }
    }
    fn voice_command(&mut self, command: VoiceCommand) {
        let command = if command == VoiceCommand::Cancel && !self.direction_mode {
            VoiceCommand::Finish
        } else {
            command
        };
        if self.read_only && !matches!(command, VoiceCommand::Finish | VoiceCommand::Cancel) {
            return;
        }
        let allowed = match command {
            VoiceCommand::Check => matches!(
                self.voice_state,
                VoiceState::Ready | VoiceState::Unavailable
            ),
            VoiceCommand::Start => {
                self.voice_state == VoiceState::Ready
                    && !self.directing()
                    && self.queued_direction.is_none()
            }
            VoiceCommand::Retry(_) => matches!(
                self.voice_state,
                VoiceState::Ready | VoiceState::Unavailable
            ),
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
            .is_none_or(|w| w.send.send(command.clone()).is_err())
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
            VoiceCommand::Retry(_) => (VoiceState::Finishing, "Transcribing saved audio…".into()),
        };
    }
    fn receive_voice(&mut self, event: VoiceEvent, now: f64) {
        match event {
            VoiceEvent::Partial { id, text } => {
                if self.active_recording.as_ref() == Some(&id)
                    && !self
                        .projection
                        .transcripts
                        .iter()
                        .any(|(key, _, _)| key == &id)
                {
                    self.live_text = text;
                }
            }
            VoiceEvent::Segment { id, segment, cue } => {
                if self.active_recording.as_ref() != Some(&id)
                    || self.read_only
                    || self
                        .projection
                        .transcripts
                        .iter()
                        .any(|(key, _, _)| key == &id)
                {
                    return;
                }
                if self.projection.direction_recordings.contains(&id) {
                    self.live_text = cue.text;
                } else {
                    self.receive_speech_segment(&id, segment, cue);
                }
            }
            VoiceEvent::LiveStatus { id, message } => {
                if self.active_recording.as_ref() == Some(&id) {
                    self.live_text = message.clone();
                    self.voice_status = message;
                }
            }
            VoiceEvent::Started { id, started } => {
                if self.modern_ui {
                    self.selected = None;
                }
                self.live_text.clear();
                self.active_recording = Some(id.clone());
                let start_ms = self.journal.as_ref().map_or(0, |j| {
                    j.now_ms()
                        .saturating_sub(started.elapsed().as_millis() as u64)
                });
                let mut events = vec![tacit::timeline::Event::RecordingStarted {
                    id: id.clone(),
                    start_ms,
                }];
                if self.direction_mode {
                    events.push(tacit::timeline::Event::RecordingDirected { id });
                }
                self.timeline_event(tacit::timeline::Event::Transaction { events });
            }
            VoiceEvent::AudioSaved(recording) => {
                let result = (|| -> std::io::Result<()> {
                    let Some(journal) = &mut self.journal else {
                        return Ok(());
                    };
                    let asset = journal.import(&recording.path, "wav")?;
                    let audio = tacit::timeline::Audio {
                        id: recording.id.clone(),
                        asset,
                        start_ms: journal
                            .entries
                            .iter()
                            .flat_map(|e| e.event.leaves())
                            .find_map(|e| match e {
                                tacit::timeline::Event::RecordingStarted { id, start_ms }
                                    if id == &recording.id =>
                                {
                                    Some(*start_ms)
                                }
                                _ => None,
                            })
                            .unwrap_or_else(|| {
                                journal
                                    .now_ms()
                                    .saturating_sub(recording.started.elapsed().as_millis() as u64)
                            }),
                        duration_ms: recording.duration_ms(),
                        sample_rate: recording.sample_rate,
                        samples: recording.samples,
                    };
                    journal.append(
                        journal.now_ms(),
                        tacit::timeline::Event::AudioSaved { audio },
                    )?;
                    journal.export_vtt()?;
                    self.projection = journal.projection.clone();
                    Ok(())
                })();
                if let Err(error) = result {
                    self.read_only = true;
                    self.status = format!("Audio archive failed: {error}");
                }
            }
            VoiceEvent::TimedTranscript { id, text, cues } => {
                if self.read_only {
                    return;
                }
                if self
                    .projection
                    .transcripts
                    .iter()
                    .any(|(key, _, _)| key == &id)
                {
                    self.voice_state = VoiceState::Ready;
                    self.active_recording = None;
                    self.live_text.clear();
                    return;
                }
                if self.projection.speech_segments.contains_key(&id)
                    && !self.projection.direction_recordings.contains(&id)
                {
                    let existing = self.projection.speech_segments.get(&id).unwrap().clone();
                    let consistent = existing.values().all(|segment| cues.contains(&segment.cue));
                    if consistent {
                        for cue in &cues {
                            if !self
                                .projection
                                .speech_segments
                                .get(&id)
                                .is_some_and(|s| s.values().any(|old| &old.cue == cue))
                            {
                                let next = self
                                    .projection
                                    .speech_segments
                                    .get(&id)
                                    .and_then(|s| s.keys().max())
                                    .copied()
                                    .unwrap_or(0)
                                    + 1;
                                self.receive_speech_segment(&id, next, cue.clone());
                            }
                        }
                    }
                    if self
                        .dispatch(tacit::timeline::Event::Transcript {
                            recording: id.clone(),
                            cues,
                            text,
                        })
                        .is_err()
                    {
                        return;
                    }
                    if let Some(j) = &self.journal
                        && let Err(error) = j.export_vtt()
                    {
                        self.status = format!("WebVTT export failed: {error}");
                    }
                    self.active_recording = None;
                    self.voice_state = VoiceState::Ready;
                    self.live_text.clear();
                    // The complete transcript is new editorial context even when every
                    // live window was already mapped. Invalidate an in-flight review.
                    self.revision += 1;
                    self.voice_status = if consistent {
                        "Recording saved · live concepts retained".into()
                    } else {
                        "Final transcription differs from live windows. Full transcript and audio saved; live concepts retained without duplication.".into()
                    };
                    self.save();
                    return;
                }
                self.active_recording = None;
                self.live_text.clear();
                let mut events = vec![tacit::timeline::Event::Transcript {
                    recording: id.clone(),
                    cues,
                    text: text.clone(),
                }];
                if self.projection.direction_recordings.contains(&id) {
                    if self
                        .dispatch(tacit::timeline::Event::Transaction { events })
                        .is_err()
                    {
                        return;
                    }
                    if let Some(j) = &self.journal
                        && let Err(error) = j.export_vtt()
                    {
                        self.status = format!("WebVTT export failed: {error}");
                    }
                    self.voice_state = VoiceState::Ready;
                    self.direction_mode = true;
                    self.direction_draft = TextEdit::new(&text);
                    self.submit_direction();
                    return;
                }
                let thought_id = self.projection.session.next_id;
                let has_words = !text.trim().is_empty();
                if has_words {
                    events.push(tacit::timeline::Event::SessionChanged {
                        change: SessionEvent::capture(&self.projection.session, &text),
                    });
                    events.push(tacit::timeline::Event::TranscriptLinked {
                        recording: id,
                        thought_id,
                    });
                    self.checkpoint();
                }
                if self
                    .dispatch(tacit::timeline::Event::Transaction { events })
                    .is_err()
                {
                    return;
                }
                if let Some(j) = &self.journal
                    && let Err(error) = j.export_vtt()
                {
                    self.status = format!("WebVTT export failed: {error}");
                }
                self.voice_state = VoiceState::Ready;
                if has_words {
                    self.selected = Some(thought_id);
                    self.save();
                    self.schedule_recording_split(thought_id, &text);
                } else {
                    self.voice_status = "No speech heard. Try again.".into();
                }
            }
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
                if self.direction_mode && !self.read_only {
                    self.voice_state = VoiceState::Ready;
                    self.direction_draft = TextEdit::new(&words);
                    self.submit_direction();
                    return;
                }
                self.voice_state = VoiceState::Ready;
                if words.trim().is_empty() {
                    self.voice_status = "No speech heard. Try again.".into();
                } else {
                    if self.read_only {
                        return;
                    }
                    self.checkpoint();
                    let id = self.projection.session.next_id;
                    if self
                        .dispatch_session(SessionEvent::capture(&self.projection.session, &words))
                        .is_err()
                    {
                        return;
                    }
                    self.selected = Some(id);
                    self.save();
                    self.schedule_recording_split(id, &words);
                }
            }
            VoiceEvent::Cancelled => {
                self.live_text.clear();
                if let Some(id) = self.active_recording.take() {
                    self.timeline_event(tacit::timeline::Event::RecordingCancelled { id });
                }
                self.voice_state = VoiceState::Ready;
                self.voice_status = "Recording discarded".into();
            }
            VoiceEvent::Error(message) => {
                self.timeline_event(tacit::timeline::Event::Failure {
                    operation: "recording/transcription".into(),
                    message: message.clone(),
                });
                self.voice_state = VoiceState::Unavailable;
                self.voice_status = message;
            }
        }
    }
    fn receive_speech_segment(&mut self, recording: &str, segment: u64, cue: tacit::timeline::Cue) {
        if self.read_only || self.replay.is_some() {
            return;
        }
        if let Some(existing) = self
            .projection
            .speech_segments
            .get(recording)
            .and_then(|s| s.get(&segment))
        {
            if existing.cue != cue {
                self.voice_status =
                    "Conflicting live segment ignored; final transcript will be saved".into();
            }
            return;
        }
        let thought_id = self.projection.session.next_id;
        let text = cue.text.clone();
        let events = vec![
            tacit::timeline::Event::SessionChanged {
                change: SessionEvent::capture(&self.projection.session, &text),
            },
            tacit::timeline::Event::SpeechSegment {
                recording: recording.into(),
                segment,
                cue,
                thought_id,
            },
        ];
        // Reject malformed/replayed windows without disabling a healthy journal.
        if self
            .projection
            .clone()
            .apply(&tacit::timeline::Event::Transaction {
                events: events.clone(),
            })
            .is_err()
        {
            self.voice_status = "Invalid live segment ignored; audio remains recorded".into();
            return;
        }
        self.checkpoint();
        if self
            .dispatch(tacit::timeline::Event::Transaction { events })
            .is_err()
        {
            return;
        }
        self.live_text.clear();
        if !self.modern_ui {
            self.selected = Some(thought_id);
        }
        self.save();
        self.schedule_recording_split(thought_id, &text);
    }
    fn toggle_direction(&mut self) {
        if self.replay.is_some() || self.read_only {
            return;
        }
        if matches!(
            self.voice_state,
            VoiceState::Starting
                | VoiceState::Recording
                | VoiceState::Finishing
                | VoiceState::Cancelling
        ) {
            self.direction_status = "Finish this recording before changing input mode".into();
            self.voice_status = self.direction_status.clone();
            return;
        }
        self.direction_mode = !self.direction_mode;
    }
    /// The map agent is carrying out something the user asked of the map.
    fn directing(&self) -> bool {
        self.agent.is_some() && self.pace.flying(tacit::agent::Tier::Direct)
    }
    /// Queue what the user asked of the map. The next frame hands it to the agent, once
    /// nothing spoken is still on its way to becoming a thought.
    fn submit_direction(&mut self) {
        if self.read_only || self.replay.is_some() || self.directing() {
            return;
        }
        let instruction = self.direction_draft.text.trim().to_owned();
        if instruction.is_empty() {
            return;
        }
        if self.agent.is_none() {
            self.direction_status =
                "Map direction needs OpenRouter; your instruction is kept here".into();
            return;
        }
        self.queued_direction = Some(instruction);
        self.direction_status = if self.pending_recordings.is_empty() {
            "Applying your guidance to the map…"
        } else {
            "Finishing captured thoughts, then applying your guidance…"
        }
        .into();
    }
    /// Ask the agent for a turn of `tier` on the map as it stands. False: the agent is gone.
    fn ask(&mut self, tier: tacit::agent::Tier, instruction: Option<String>, now: f64) -> bool {
        // A directed turn is awaited under the epoch its own asking opens.
        self.pace.sent(tier, &self.projection.session, now);
        let request = tacit::agent::worker::Request {
            epoch: self.pace.epoch(),
            session: self.projection.session.clone(),
            holds: self.pending_recordings.clone(),
            instruction,
        };
        if self.agent.as_ref().is_some_and(|a| a.ask(tier, request)) {
            return true;
        }
        self.pace.dropped(tier);
        self.agent = None;
        self.organizer_status = "Map agent stopped · editing by hand remains available".into();
        false
    }
    /// Hand the queued instruction to the agent. True when it left the queue.
    fn start_queued_direction(&mut self, now: f64) -> bool {
        if self.read_only
            || self.replay.is_some()
            || self.directing()
            || !self.pending_recordings.is_empty()
        {
            return false;
        }
        let Some(instruction) = self.queued_direction.take() else {
            return false;
        };
        if self.ask(tacit::agent::Tier::Direct, Some(instruction), now) {
            self.direction_status = "Applying your guidance to the map…".into();
            self.organizer_status = "✦ Following your guidance…".into();
        } else {
            self.direction_status = "The map agent stopped; your instruction is kept here".into();
        }
        true
    }
    /// The agent's answer to an instruction. Whatever the user did by hand meanwhile stands:
    /// the answer's edits are tried on the map as it is now.
    fn receive_direction(&mut self, response: tacit::agent::worker::Response, now: f64) {
        use tacit::agent::{self, Tier};
        // Cancelled by an undo, or not the instruction now in flight: nobody waits for it.
        if !self.pace.awaited(response.epoch) {
            return;
        }
        let turn = match response.result {
            Ok(turn) => turn,
            Err(error) => {
                self.pace.failed(Tier::Direct, now);
                self.direction_status = error;
                self.organizer_status = AGENT_IDLE.into();
                return;
            }
        };
        let instruction = response.instruction.unwrap_or_default();
        let outcome = agent::apply(
            &self.projection.session,
            &turn.ops,
            Tier::Direct,
            &self.pending_recordings,
        );
        let unmet = turn.unmet + outcome.rejected.len();
        let changed = !outcome.ops.is_empty();
        // Guidance that changes nothing today is still journaled when it says how the map is
        // to be kept from now on.
        if changed || turn.remember {
            self.checkpoint();
            let change = SessionEvent::Guided {
                instruction: instruction.clone(),
                ops: outcome.ops,
                note: turn.note.clone(),
            };
            if let Err(error) = self.dispatch_session(change) {
                self.history.pop();
                self.pace.failed(Tier::Direct, now);
                self.direction_status = error;
                self.organizer_status = AGENT_IDLE.into();
                return;
            }
            self.settled_at = now;
            if self.direction_draft.text.trim() == instruction {
                self.direction_draft = TextEdit::new("");
            }
            self.save();
        }
        // What the user asked for stands until new brainstorming content arrives.
        self.pace
            .landed(Tier::Direct, &self.projection.session, false, now);
        let mut said = match (turn.note.trim(), changed, turn.remember) {
            ("", true, _) => "Guidance applied to the map".to_owned(),
            ("", false, true) => "Noted for how the map is kept from now on".to_owned(),
            ("", false, false) => "Nothing on the map needed to change".to_owned(),
            (note, ..) => note.to_owned(),
        };
        if unmet > 0 {
            let edits = if unmet == 1 { "edit" } else { "edits" };
            said += &format!(" · {unmet} {edits} did not fit the map");
        }
        self.direction_status = said;
        self.organizer_status = if changed {
            "✦ Map follows your guidance"
        } else {
            AGENT_IDLE
        }
        .into();
    }
    fn schedule_recording_split(&mut self, id: u64, words: &str) {
        let mut context = self.projection.session.clone();
        context.thoughts.retain(|t| t.id != id);
        context.interpretation = None;
        let request = tacit::recording::Request {
            id,
            text: words.trim().to_owned(),
            context,
        };
        if self
            .recording_worker
            .as_ref()
            .is_some_and(|w| w.send.send(request).is_ok())
        {
            self.pending_recordings.insert(id);
            self.voice_status = "Recording saved · separating ideas in context…".into();
        } else {
            self.voice_status = "Full recording kept · automatic splitting needs OpenRouter".into();
        }
    }
    fn receive_recording(&mut self, response: tacit::recording::Response) {
        if !self.pending_recordings.remove(&response.id) || self.read_only {
            return;
        }
        let Some(_index) = self
            .projection
            .session
            .thoughts
            .iter()
            .position(|t| t.id == response.id && t.text == response.text && !t.pinned)
        else {
            return;
        };
        match response.result {
            // Already one thought: nothing to split, and nothing for undo to step over.
            Ok(chunks) if chunks.len() == 1 && chunks[0] == response.text => {
                self.voice_status = "Recording saved as one thought".into();
            }
            Ok(chunks)
                if !chunks.is_empty()
                    && chunks.iter().all(|c| !c.trim().is_empty())
                    && chunks.concat() == response.text =>
            {
                self.checkpoint();
                let count = chunks.len();
                if self
                    .dispatch_session(SessionEvent::Split {
                        source_id: response.id,
                        original: response.text.clone(),
                        chunks: chunks.clone(),
                    })
                    .is_err()
                {
                    return;
                }
                self.voice_status = format!("Recording → {count} thoughts · organizing in context");
                self.save();
            }
            Err(error) => self.voice_status = format!("{error} · full recording kept"),
            _ => self.voice_status = "Invalid split · full recording kept".into(),
        }
    }
    /// Persist first, then publish the reducer's projection to the renderer.
    fn dispatch(&mut self, event: tacit::timeline::Event) -> Result<(), String> {
        if self.replay.is_some() || self.read_only {
            return Err("Session is read-only".into());
        }
        let result = if let Some(journal) = &mut self.journal {
            journal
                .append(journal.now_ms(), event)
                .map(|()| journal.projection.clone())
        } else {
            let mut next = self.projection.clone();
            next.apply(&event).map(|()| next)
        };
        match result {
            Ok(next) => {
                self.projection = next;
                Ok(())
            }
            Err(error) => {
                let message =
                    format!("Timeline write failed: {error}. Editing stopped to protect replay.");
                self.read_only = true;
                self.status = message.clone();
                Err(message)
            }
        }
    }
    fn dispatch_session(&mut self, change: SessionEvent) -> Result<(), String> {
        // Invalid/stale model output is rejected without disabling a healthy journal.
        change.reduce(&self.projection.session)?;
        self.dispatch(tacit::timeline::Event::SessionChanged { change })
    }
    fn timeline_event(&mut self, event: tacit::timeline::Event) {
        let _ = self.dispatch(event);
    }

    fn record_presentation(&mut self, _reason: &str) {
        let _ = self.commit_presentation(None);
    }
    fn commit_presentation(
        &mut self,
        pixels: Option<tacit::timeline::PixelFrame>,
    ) -> Result<(), String> {
        if self.sessions_open || self.replay.is_some() || self.read_only || self.journal.is_none() {
            return Ok(());
        }
        let result = (|| -> std::io::Result<()> {
            let journal = self.journal.as_mut().unwrap();
            let mut artwork = Vec::new();
            for layer in &self.gallery.layers {
                let asset = if let Some(asset) = self.art_assets.get(&layer.image.id()) {
                    asset.clone()
                } else {
                    let bytes = layer.encoded.as_ref().ok_or_else(|| {
                        std::io::Error::other("Illustration source bytes unavailable")
                    })?;
                    let asset = journal.import_bytes(bytes, "png")?;
                    self.art_assets.insert(layer.image.id(), asset.clone());
                    asset
                };
                artwork.push(tacit::timeline::Artwork {
                    mural: layer.mural.clone(),
                    key: layer.key.clone(),
                    fingerprint: layer.fingerprint.clone(),
                    slot: layer.slot,
                    asset,
                    born: layer.born,
                    retired: layer.retired,
                });
            }
            let mut garden = self.art.clone();
            garden.now = 0.0;
            let scene = tacit::timeline::Scene {
                mural_layout: self.gallery.mural_layout,
                workspace: self.modern_ui.then(|| tacit::presentation::Workspace {
                    camera: tacit::camera::Camera {
                        zoom: self.camera_zoom.get(),
                        pan: self.camera_pan.get(),
                    },
                    expanded: self.transcript_expanded,
                    following: self.transcript_following,
                    transcript_scroll: self.transcript_scroll.get(),
                }),
                session: self.projection.session.clone(),
                render_time: Some(self.art.now),
                session_title: self.session_name.clone(),
                direction_mode: self.direction_mode,
                direction_draft: self.direction_draft.text.clone(),
                direction_status: self.direction_status.clone(),
                live_text: self.live_text.clone(),
                draft: self.draft.text.clone(),
                selected: self.selected,
                style: self.art_style,
                story: self.story,
                garden,
                artwork,
                busy: self.busy,
                settled_at: self.settled_at,
                organizer_status: self.organizer_status.clone(),
                voice_status: self.voice_status.clone(),
                art_status: self.gallery.status.clone(),
                map_scroll: self.map_scroll.get(),
                story_scroll: self.story_scroll.get(),
                inspector_scroll: self.inspector_scroll.get(),
                viewport: self.viewport,
                map_viewport: self.map_viewport,
                reduced_motion: self.reduced_motion,
            };
            if let Some(frame) = pixels {
                journal.present_pixels(journal.now_ms(), scene, frame)?;
            } else {
                journal.present_scene(journal.now_ms(), scene)?;
            }
            // Presentation commits cannot change session content.
            self.projection = journal.projection.clone();
            Ok(())
        })();
        if let Err(error) = result {
            self.read_only = true;
            self.status =
                format!("Timeline write failed: {error}. Editing stopped to protect replay.");
            if let Some(journal) = &self.journal {
                self.projection = journal.projection.clone();
            }
            return Err(self.status.clone());
        }
        Ok(())
    }
    fn restore_scene(&mut self, scene: tacit::timeline::Scene, now: f64) {
        let projection = self.journal.as_ref().map(|journal| {
            if self.replay.is_some() {
                journal
                    .at((now * 1000.0).round() as u64)
                    .expect("validated timeline")
            } else {
                journal.projection.clone()
            }
        });
        self.restore_projected_scene(scene, now, projection);
    }
    fn restore_projected_scene(
        &mut self,
        scene: tacit::timeline::Scene,
        now: f64,
        projection: Option<tacit::timeline::Projection>,
    ) {
        self.modern_ui = self.replay.is_none() || scene.workspace.is_some();
        if let Some(w) = &scene.workspace {
            self.camera_zoom.set(w.camera.zoom);
            self.camera_pan.set(w.camera.pan);
            self.framing.set(workspace::Framing::Restored);
            self.transcript_expanded = w.expanded;
            self.transcript_following = w.following;
            self.transcript_scroll.set(w.transcript_scroll);
        }
        self.session_name = scene.session_title.clone();
        self.live_text = scene.live_text.clone();
        self.direction_mode = scene.direction_mode;
        self.direction_draft = TextEdit::new(&scene.direction_draft);
        self.direction_status = scene.direction_status.clone();
        let render_now = if self.replay.is_some() {
            scene.render_time.unwrap_or(now)
        } else {
            now
        };
        if let Some(projection) = projection {
            self.projection = projection;
        } else {
            self.projection.session = scene.session.clone();
        }
        if self.replay.is_some() {
            self.projection.session = scene.session;
        }
        self.viewport = scene.viewport;
        self.map_viewport = scene.map_viewport;
        let pixel_gap = self.projection.pixel_capture_active == Some(false)
            && self
                .journal
                .as_ref()
                .is_some_and(|j| (now * 1000.0).round() < j.duration() as f64);
        if self.replay.is_some() && self.pixel_replay && !pixel_gap {
            if let Some(frame) = &self.projection.pixel_frame {
                if self
                    .pixel_image
                    .as_ref()
                    .is_none_or(|(asset, _)| asset != &frame.asset)
                {
                    let loaded = (|| -> std::io::Result<rune::Image> {
                        let path = self.journal.as_ref().unwrap().asset(&frame.asset)?;
                        let image = self.image_cache.load(std::fs::read(path)?);
                        image
                            .wait()
                            .map_err(|e| std::io::Error::other(e.to_string()))?;
                        Ok(image)
                    })();
                    match loaded {
                        Ok(image) => self.pixel_image = Some((frame.asset.clone(), image)),
                        Err(error) => {
                            self.pixel_image = None;
                            self.status = format!("Original frame unavailable: {error}");
                            if let Some(r) = &mut self.replay {
                                r.playing = false;
                            }
                        }
                    }
                }
            } else {
                self.pixel_image = None;
            }
        }
        if self.pixel_replay && pixel_gap {
            self.pixel_image = None;
        }
        self.draft = TextEdit::new(scene.draft);
        self.selected = scene.selected;
        self.art_style = if self.replay.is_some() {
            scene.style
        } else {
            scene.style.current()
        };
        self.story = scene.story;
        self.art = scene.garden;
        self.art.now = render_now;
        self.gallery.now = render_now;
        self.gallery.mural_layout = self.replay.is_none() || scene.mural_layout;
        self.reduced_motion = scene.reduced_motion;
        self.busy = scene.busy;
        self.settled_at = scene.settled_at;
        self.pulse = ((render_now * 3.0).sin() as f32 + 1.0) / 2.0;
        self.organizer_status = scene.organizer_status;
        self.voice_status = scene.voice_status;
        self.gallery.status = scene.art_status;
        self.gallery.layers = scene
            .artwork
            .into_iter()
            .filter_map(|a| {
                // Live editing resumes from the archived pixels, even if the PNG cache
                // was pruned. Replay reuses decoded handles without reading bytes per frame.
                let encoded = if self.replay.is_none() && a.mural.is_some() && a.retired.is_none() {
                    self.journal
                        .as_ref()
                        .and_then(|j| j.asset(&a.asset).ok())
                        .and_then(|p| std::fs::read(p).ok())
                        .map(std::sync::Arc::new)
                } else {
                    None
                };
                let image = if let Some(image) = self.replay_images.get(&a.asset) {
                    image.clone()
                } else {
                    let path = self.journal.as_ref()?.asset(&a.asset).ok()?;
                    let bytes = std::fs::read(path).ok()?;
                    let image = self.image_cache.load(bytes);
                    if rune::is_headless() {
                        let _ = image.wait();
                    }
                    self.replay_images.insert(a.asset.clone(), image.clone());
                    image
                };
                self.art_assets.insert(image.id(), a.asset.clone());
                Some(generated_art::Layer {
                    encoded,
                    mural: a.mural,
                    key: a.key,
                    fingerprint: a.fingerprint,
                    slot: a.slot,
                    image,
                    born: a.born,
                    retired: a.retired,
                })
            })
            .collect();
    }
    fn video_busy(&self) -> bool {
        self.video_destination_pending || self.video_export.is_some()
    }
    fn first_video_frame_ms(&self) -> Option<u64> {
        self.journal.as_ref()?.entries.iter().find_map(|entry| {
            entry
                .event
                .leaves()
                .any(|event| matches!(event, tacit::timeline::Event::Pixels { .. }))
                .then_some(entry.at_ms)
        })
    }
    fn replay_start_ms(&self) -> u64 {
        self.pixel_replay
            .then(|| self.first_video_frame_ms())
            .flatten()
            .or_else(|| self.journal.as_ref()?.first_frame_ms())
            .unwrap_or(0)
    }
    fn can_play_session(&self) -> bool {
        self.journal.is_some()
            && (self.replay.is_some() || !self.read_only)
            && !self.voice_in_progress()
            && self.pending_recordings.is_empty()
            && !self.directing()
            && self.queued_direction.is_none()
            && !self.video_busy()
    }
    fn play_session(&mut self) {
        if !self.can_play_session() {
            return;
        }
        if self.replay.is_none() {
            self.toggle_replay();
        }
        if let Some(replay) = &mut self.replay {
            replay.playing = true;
            replay.last_tick = None;
        }
    }
    fn choose_video_file(&mut self, tx: &mut Tx) {
        if !self.can_play_session() || self.first_video_frame_ms().is_none() {
            return;
        }
        let title: String = self
            .session_name
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
            .take(64)
            .collect();
        let title = if title.trim().is_empty() {
            "Tacit session"
        } else {
            title.trim()
        };
        let options = rune::FileDialogOptions {
            title: "Save session video".into(),
            file_name: Some(format!("{title}-{}.mkv", tacit::timeline::unix_ms())),
            ..Default::default()
        }
        .filter("Lossless video", &["mkv"]);
        self.video_destination_pending = true;
        tx.save_file(options, |s: &mut State, result, _| {
            s.video_destination_pending = false;
            match result {
                Ok(Some(path)) => s.export_video_to(path),
                Ok(None) => {}
                Err(error) => s.status = format!("Cannot choose a video destination: {error}"),
            }
        });
    }
    fn export_video_to(&mut self, mut output: PathBuf) {
        if !self.can_play_session() || self.first_video_frame_ms().is_none() {
            return;
        }
        if output.extension().is_none() {
            output.set_extension("mkv");
        }
        if output.extension().and_then(|s| s.to_str()) != Some("mkv") {
            self.status = "Choose an .mkv filename for the lossless video".into();
            return;
        }
        if output.exists() {
            self.status = "That file already exists · choose a new video filename".into();
            return;
        }
        // Freeze the journal before the worker reads it. Live capture stays paused
        // until export completes, so the audio and pixels share a stable endpoint.
        if self.replay.is_none() {
            self.toggle_replay();
        }
        if self.replay.is_none() {
            return;
        }
        let Some(journal) = &self.journal else {
            return;
        };
        let source = journal.root.clone();
        let (send, receive) = std::sync::mpsc::channel();
        self.video_export = Some(receive);
        self.status = "Saving video with original audio…".into();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let result = tacit::timeline::Journal::read(&source)
                .and_then(|journal| tacit::video_archive::export(&journal, &output))
                .map(|()| output)
                .map_err(|e| e.to_string());
            let _ = send.send(result);
            if let Some(wake) = wake {
                wake();
            }
        });
    }
    fn show_saved_video(&mut self) {
        let Some(path) = &self.saved_video else {
            return;
        };
        #[cfg(target_os = "macos")]
        let result = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .status();
        #[cfg(target_os = "windows")]
        let result = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .status();
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let result = std::process::Command::new("xdg-open")
            .arg(path.parent().unwrap_or(path))
            .status();
        match result {
            Ok(status) if status.success() => {}
            _ => {
                self.status = format!(
                    "Video saved at {} · could not open its folder",
                    path.display()
                )
            }
        }
    }
    pub fn capturing_pixels(&self) -> bool {
        self.journal.is_some() && !self.read_only && self.replay.is_none() && !self.sessions_open
    }
    pub fn capture_failed(&mut self, error: String) {
        self.timeline_event(tacit::timeline::Event::Failure {
            operation: "original video capture".into(),
            message: error.clone(),
        });
        self.read_only = true;
        self.status = format!("Original video capture failed: {error}. Editing stopped.");
    }
    pub fn record_pixels(
        &mut self,
        size: (u32, u32),
        scale: f32,
        pixels: &[u8],
    ) -> Result<(), String> {
        if !self.capturing_pixels() {
            return Err("Original pixel capture is not active".into());
        }
        self.journal
            .as_mut()
            .ok_or("No archive for original pixels")?
            .begin_pixels()
            .map_err(|e| e.to_string())?;
        let raw_hash = *blake3::hash(pixels).as_bytes();
        let asset = if let Some((previous, frame)) = &self.last_pixels
            && previous == &raw_hash
            && (frame.width, frame.height) == size
        {
            frame.asset.clone()
        } else {
            let bytes = tacit::video_archive::png_bytes(size.0, size.1, pixels)
                .map_err(|e| e.to_string())?;
            self.journal
                .as_ref()
                .ok_or("No archive for original pixels")?
                .import_bytes(&bytes, "png")
                .map_err(|e| e.to_string())?
        };
        let frame = tacit::timeline::PixelFrame {
            asset,
            width: size.0,
            height: size.1,
            scale,
            content_hash: tacit::timeline::content_hash(&self.projection.session)
                .map_err(|e| e.to_string())?,
        };
        self.commit_presentation(Some(frame.clone()))?;
        self.last_pixels = Some((raw_hash, frame));
        Ok(())
    }
    fn prepare_replay(&mut self) -> std::io::Result<()> {
        let journal = self
            .journal
            .as_ref()
            .ok_or_else(|| std::io::Error::other("No session archive"))?;
        journal.validate_replay()?;
        if journal.entries.iter().flat_map(|e| e.event.leaves()).any(|e| matches!(e, tacit::timeline::Event::Failure { operation, .. } if operation == "original video capture")) {
            self.status = "Original video is incomplete · available frames can be inspected".into();
        }
        for art in &journal.projection.artwork {
            if !self.replay_images.contains_key(&art.asset) {
                let bytes = std::fs::read(journal.asset(&art.asset)?)?;
                let image = self.image_cache.load(bytes);
                image.wait().map_err(|error| {
                    std::io::Error::other(format!(
                        "Cannot decode replay artwork {}: {error}",
                        art.asset
                    ))
                })?;
                self.replay_images.insert(art.asset.clone(), image);
            }
        }
        Ok(())
    }
    fn toggle_replay(&mut self) {
        if self.archive_only || self.video_busy() {
            return;
        }
        if matches!(
            self.voice_state,
            VoiceState::Recording
                | VoiceState::Starting
                | VoiceState::Finishing
                | VoiceState::Cancelling
        ) || !self.pending_recordings.is_empty()
            || self.directing()
            || self.queued_direction.is_some()
        {
            return;
        }
        if self.replay.take().is_some() {
            if let Some((_, player)) = self.replay_audio.take() {
                player.pause();
            }
            let scene = self.journal.as_ref().map(|j| j.projection.scene());
            if let Some(scene) = scene {
                self.restore_scene(
                    scene,
                    self.journal.as_ref().unwrap().now_ms() as f64 / 1000.0,
                );
            }
            self.read_only = false;
            self.pixel_image = None;
            self.voice_state = VoiceState::Unavailable;
            self.voice_status = "Replay ended · reconnect Hex to record".into();
        } else if self.journal.is_some() && !self.read_only {
            if let Err(error) = self.journal.as_mut().unwrap().end_pixels() {
                self.capture_failed(error.to_string());
                return;
            }
            self.record_presentation("replay boundary");
            if self.read_only {
                return;
            }
            if let Err(error) = self.prepare_replay() {
                self.status = format!("Cannot replay: {error}");
                return;
            }
            self.gallery.suspend();
            self.pixel_replay = self.first_video_frame_ms().is_some();
            self.replay = Some(Replay {
                at_ms: self.replay_start_ms(),
                playing: false,
                last_tick: None,
                last_projected: None,
            });
            self.read_only = true;
        }
        if let Some(wake) = &self.wake {
            wake();
        }
    }
    fn seek_replay(&mut self, delta: i64) {
        let end = self.journal.as_ref().map_or(0, |j| j.duration());
        let start = self.replay_start_ms();
        if let Some(replay) = &mut self.replay {
            replay.at_ms = replay
                .at_ms
                .saturating_add_signed(delta)
                .min(end)
                .max(start);
            replay.last_tick = None;
        }
        if let Some((_, player)) = self.replay_audio.take() {
            player.pause();
        }
    }
    fn next_replay_event(&mut self) {
        if let (Some(journal), Some(replay)) = (&self.journal, &mut self.replay) {
            replay.at_ms = journal
                .entries
                .iter()
                .find(|e| e.at_ms > replay.at_ms)
                .map_or(journal.duration(), |e| e.at_ms);
            replay.last_tick = None;
            if let Some((_, player)) = self.replay_audio.take() {
                player.pause();
            }
        }
    }
    fn replay_caption(&self) -> String {
        let (Some(j), Some(r)) = (&self.journal, &self.replay) else {
            return String::new();
        };
        j.projection
            .transcripts
            .iter()
            .find_map(|(id, cues, _)| {
                let audio = j.projection.audio.iter().find(|a| &a.id == id)?;
                let time = r.at_ms.checked_sub(audio.start_ms)?;
                cues.iter()
                    .find(|c| time >= c.start_ms && time < c.end_ms)
                    .map(|c| c.text.clone())
            })
            .unwrap_or_else(|| "Session replay · original audio and saved model results".into())
    }
    fn recover_transcripts(&mut self) {
        let Some(j) = &self.journal else {
            return;
        };
        let pending:Vec<_>=j.entries.iter().enumerate().filter_map(|(index,e)| {
            let tacit::timeline::Event::Transcript{recording,text,..}=&e.event else{return None;};
            if j.projection.direction_recordings.contains(recording) || j.projection.speech_segments.contains_key(recording) || text.trim().is_empty() || j.entries.iter().any(|e|matches!(&e.event,tacit::timeline::Event::TranscriptLinked{recording:id,..} if id==recording)){return None;}
            let mut before = tacit::timeline::Projection::default();
            for entry in &j.entries[..index] { before.apply(&entry.event).ok()?; }
            let expected = before.session.next_id;
            Some((recording.clone(),text.clone(),expected))
        }).collect();
        for (recording, text, expected) in pending {
            let id = if self
                .projection
                .session
                .thoughts
                .iter()
                .any(|t| t.id == expected && t.text == text.trim())
            {
                Some(expected)
            } else {
                let direction_mode = self.direction_mode;
                self.direction_mode = false; // This legacy transcript was captured as brainstorming.
                self.receive_voice(
                    VoiceEvent::Transcript(text),
                    self.journal.as_ref().unwrap().now_ms() as f64 / 1000.0,
                );
                self.direction_mode = direction_mode;
                self.selected
            };
            if let Some(thought_id) = id {
                self.timeline_event(tacit::timeline::Event::TranscriptLinked {
                    recording,
                    thought_id,
                });
            }
        }
        if self.direction_draft.text.trim().is_empty()
            && let Some(j) = &self.journal
            && let Some((_, _, text)) = j.projection.transcripts.iter().rev().find(|(id, _, text)| {
                j.projection.direction_recordings.contains(id) && !j.entries.iter().flat_map(|e| e.event.leaves()).any(|e| {
                    matches!(e, tacit::timeline::Event::SessionChanged { change: SessionEvent::Directed { instruction, .. } | SessionEvent::Guided { instruction, .. } } if instruction.trim() == text.trim())
                })
            }) {
                self.direction_mode = true;
                self.direction_draft = TextEdit::new(text);
                self.direction_status = "Saved spoken direction recovered. Apply it when ready.".into();
        }
    }
    fn retry_transcript(&mut self) {
        let Some(j) = &self.journal else {
            return;
        };
        let Some(audio) = j.projection.audio.iter().rev().find(|a| {
            !j.projection
                .transcripts
                .iter()
                .any(|(id, _, _)| id == &a.id)
        }) else {
            return;
        };
        // Retained originals have recording IDs as filenames, independent of the content-addressed copy.
        let path = j.root.join("recordings").join(format!("{}.wav", audio.id));
        self.voice_command(VoiceCommand::Retry(path));
    }
    fn checkpoint(&mut self) {
        self.revision += 1;
        self.history.push(self.projection.session.clone());
        if self.history.len() > 100 {
            self.history.remove(0);
        }
    }
    fn save(&mut self) {
        self.record_presentation("map edit or model result");
        if self.read_only {
            return;
        }
        if let Some(path) = &self.path {
            self.status = match storage::save(path, &self.projection.session) {
                Ok(()) => {
                    if let Some(base) = &self.library_base {
                        match tacit::sessions::update_automatic_name(
                            base,
                            path,
                            &self.projection.session,
                        ) {
                            Ok(name) => {
                                self.session_name = name.clone();
                                if let Some(entry) =
                                    self.session_entries.iter_mut().find(|e| &e.path == path)
                                {
                                    entry.name = name;
                                }
                            }
                            Err(e) => {
                                self.status = format!(
                                    "Session saved, but its name could not be updated: {e}"
                                );
                                return;
                            }
                        }
                    }
                    "Saved on this device".into()
                }
                Err(e) => format!("Not saved: {e}. Use Retry save."),
            };
        }
    }
    fn capture(&mut self) {
        if self.direction_mode {
            self.submit_direction();
            return;
        }
        if self.read_only || self.draft.text.trim().is_empty() {
            return;
        }
        self.checkpoint();
        let id = self.projection.session.next_id;
        if self
            .dispatch_session(SessionEvent::capture(
                &self.projection.session,
                &self.draft.text,
            ))
            .is_err()
        {
            return;
        }
        self.selected = Some(id);
        self.draft = TextEdit::new("");
        self.save();
    }
    fn move_selected(&mut self) {
        if self.read_only {
            return;
        }
        if let Some(id) = self.selected {
            self.checkpoint();
            let Ok(change) = SessionEvent::correct_placement(&self.projection.session, id) else {
                return;
            };
            if self.dispatch_session(change).is_err() {
                return;
            }
            self.pace.take_over(&self.projection.session);
            self.save();
        }
    }
    /// One edit of the user's own to the selected thought. `own`: it changes the map itself,
    /// so the agent's turns in flight no longer speak for it.
    fn edit_selected(&mut self, change: impl FnOnce(u64) -> SessionEvent, own: bool) {
        if self.read_only {
            return;
        }
        if let Some(id) = self.selected {
            self.checkpoint();
            if self.dispatch_session(change(id)).is_err() {
                return;
            }
            if own {
                self.pace.take_over(&self.projection.session);
            }
            self.save();
        }
    }
    fn unpin_selected(&mut self) {
        self.edit_selected(|id| SessionEvent::Unpinned { id }, false);
    }
    /// Set the selected thought beside the map. It stays selected, so it can come straight back.
    fn park_selected(&mut self) {
        self.edit_selected(|id| SessionEvent::Parked { id }, true);
    }
    /// Return a parked thought to the map, where the agent finds it a branch.
    fn revive_selected(&mut self) {
        self.edit_selected(|id| SessionEvent::Revived { id }, true);
    }
    /// Keep a parked thought off the map until the user says otherwise.
    fn hold_selected(&mut self) {
        self.edit_selected(|id| SessionEvent::Held { id }, false);
    }
    fn remove_selected(&mut self) {
        if self.read_only {
            return;
        }
        if let Some(id) = self.selected.take() {
            self.checkpoint();
            if self.dispatch_session(SessionEvent::Removed { id }).is_err() {
                return;
            }
            self.pace.take_over(&self.projection.session);
            self.save();
        }
    }
    fn undo(&mut self) {
        if self.read_only {
            return;
        }
        // Guidance that has not reached the map is the latest thing to take back. An answer
        // still on its way finds nobody waiting for it.
        if self.queued_direction.take().is_some() || self.directing() {
            self.pace.dropped(tacit::agent::Tier::Direct);
            self.direction_status = "Guidance cancelled; your instruction is kept here".into();
            if self.agent.is_some() {
                self.organizer_status = AGENT_IDLE.into();
            }
            return;
        }
        self.pending_recordings.clear();
        if let Some(previous) = self.history.pop() {
            self.revision += 1;
            if self
                .dispatch_session(SessionEvent::Restored {
                    session: Box::new(previous),
                    reason: "undo".into(),
                })
                .is_err()
            {
                return;
            }
            self.pace.take_over(&self.projection.session);
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
        .radius(16.0)
        .hover(kit::wash(1.6))
        .on_press(action)
}
fn enabled(control: El, available: bool) -> El {
    if available {
        control
    } else {
        control.inert().focusable(false).opacity(0.45)
    }
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
/// A title as one line, or as two of similar length that can each be centered.
fn balanced(title: &str) -> Vec<String> {
    let words: Vec<&str> = title.split_whitespace().collect();
    let total = title.chars().count();
    if total <= 28 || words.len() < 2 {
        return vec![title.into()];
    }
    let mut seen = 0;
    let split = (1..words.len())
        .min_by_key(|&at| {
            seen += words[at - 1].chars().count() + 1;
            (2 * seen).abs_diff(total + 1)
        })
        .unwrap_or(1);
    vec![words[..split].join(" "), words[split..].join(" ")]
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
struct TreeEntry {
    id: Option<u64>,
    words: String,
    theme: Option<String>,
    parent: Option<String>,
    depth: usize,
}
fn theme_entries(session: &Session, theme: &str, depth: usize, entries: &mut Vec<TreeEntry>) {
    for t in session
        .thoughts
        .iter()
        .filter(|t| t.category.as_deref() == Some(theme))
    {
        entries.push(TreeEntry {
            id: Some(t.id),
            words: t.map_label().into(),
            theme: None,
            parent: Some(theme.into()),
            depth,
        });
    }
    if depth + 1 >= tacit::model::MAX_CATEGORY_DEPTH {
        return;
    }
    for c in session
        .categories
        .iter()
        .filter(|c| c.parent.as_deref() == Some(theme))
    {
        entries.push(TreeEntry {
            id: None,
            words: c.name.clone(),
            theme: Some(c.id.clone()),
            parent: Some(theme.into()),
            depth: depth + 1,
        });
        theme_entries(session, &c.id, depth + 1, entries);
    }
}
fn tree(s: &State, cx: &Cx<State>) -> El {
    let depth = s
        .projection
        .session
        .categories
        .iter()
        .map(|c| {
            s.projection
                .session
                .category_path(Some(&c.id))
                .len()
                .saturating_sub(1)
        })
        .max()
        .unwrap_or(0);
    let indent = depth as f32 * 88.0;
    let mural = s.gallery.mural_layout;
    let width = if mural {
        1300.0 + indent * 2.0
    } else if s.modern_ui {
        1100.0 + indent * 2.0
    } else if s.replay.is_some() && s.map_viewport[0] > 0.0 {
        s.map_viewport[0]
    } else {
        cx.layout_size("map").map_or(920.0, |size| size[0])
    }
    .max(if depth > 0 {
        920.0 + indent * 2.0
    } else {
        700.0
    });
    // Themes carry stable IDs; the original narrative lenses remain the offline fallback.
    let mut groups: Vec<(Option<String>, String, Role)> =
        if s.projection.session.categories.is_empty() {
            Role::ALL
                .into_iter()
                .map(|r| (None, branch_name(r).into(), r))
                .collect()
        } else {
            s.projection
                .session
                .categories
                .iter()
                .filter(|c| c.parent.is_none())
                .enumerate()
                .map(|(i, c)| (Some(c.id.clone()), c.name.clone(), Role::ALL[i % 4]))
                .collect()
        };
    if !s.projection.session.categories.is_empty()
        && (s
            .projection
            .session
            .thoughts
            .iter()
            .any(|t| t.category.is_none())
            || !s.draft.text.trim().is_empty())
    {
        groups.push((None, "Taking root".into(), Role::Observation));
    }
    let matches = |t: &tacit::model::Thought, group: usize| {
        if s.projection.session.categories.is_empty() {
            t.role == groups[group].2
        } else {
            t.category == groups[group].0
        }
    };
    let draft_group = if s.projection.session.categories.is_empty() {
        Role::ALL
            .iter()
            .position(|r| *r == classify(&s.draft.text))
            .unwrap_or(0)
    } else {
        groups.len() - 1
    };
    let entries: Vec<Vec<TreeEntry>> = (0..groups.len())
        .map(|i| {
            let mut entries = Vec::new();
            if let Some(theme) = &groups[i].0 {
                theme_entries(&s.projection.session, theme, 0, &mut entries);
            } else {
                entries.extend(
                    s.projection
                        .session
                        .thoughts
                        .iter()
                        .filter(|t| matches(t, i))
                        .map(|t| TreeEntry {
                            id: Some(t.id),
                            words: t.map_label().into(),
                            theme: None,
                            parent: None,
                            depth: 0,
                        }),
                );
            }
            if !s.draft.text.trim().is_empty() && i == draft_group {
                entries.push(TreeEntry {
                    id: None,
                    words: s.draft.text.clone(),
                    theme: None,
                    parent: groups[i].0.clone(),
                    depth: 0,
                });
            }
            entries
        })
        .collect();
    let counts: Vec<usize> = entries.iter().map(Vec::len).collect();
    let mut offsets = Vec::new();
    let mut height = 0.0f32;
    for pair in counts.chunks(2) {
        offsets.push(height + 55.0);
        height += (pair.iter().copied().max().unwrap_or(1).max(2) as f32 * 72.0 + 100.0).max(230.0);
    }
    height = height.max(if mural { 640.0 } else { 460.0 });
    let center = [width / 2.0, height / 2.0];
    let leaf_w = (width * 0.27).min(255.0);
    let mural_w = (width - 2.0 * (leaf_w + indent + 80.0)).clamp(360.0, 640.0);
    let mural_h = mural_w * 2.0 / 3.0;
    let mural_art = mural
        && s.art_style != Style::Off
        && s.gallery.layers.iter().any(|l| {
            l.key == generated_art::CENTRAL_KEY
                && l.mural.is_some()
                && if cx.is_reduced_motion() {
                    l.retired.is_none()
                } else {
                    l.opacity(s.gallery.now) > 0.0
                }
        });
    let reserve_art = mural_art && s.replay.is_none();
    // Where the lowest idea ends: framing follows the ideas, not the paper beneath them.
    let extent = counts
        .iter()
        .enumerate()
        .map(|(group, count)| offsets[group / 2] + (*count).max(1) as f32 * 72.0 - 18.0)
        .fold(0.0, f32::max);
    // Until a mural arrives, the central idea is the hub every theme grows from.
    let hub = (mural && !mural_art).then(|| {
        [
            center[0],
            if counts.is_empty() {
                center[1]
            } else {
                (9.0 + extent) / 2.0
            },
        ]
    });
    let mut strokes = Vec::new();
    let mut labels = Vec::new();
    for (group, (id, name, role)) in groups.iter().enumerate() {
        let role = *role;
        let left = group % 2 == 0;
        // Toward the middle of the map, where every branch comes from.
        let inward = if left { 1.0 } else { -1.0 };
        let first_y = offsets[group / 2];
        let top = first_y < center[1];
        let ink = color(role);
        let n = counts[group].max(1);
        let fork_y = first_y + ((n - 1) as f32 * 72.0) / 2.0 + 27.0;
        let fork_x = if left {
            leaf_w + 55.0 + indent
        } else {
            width - leaf_w - 55.0 - indent
        };
        let start = if let Some(hub) = hub {
            hub
        } else if reserve_art {
            // Transparent faces and objects must not expose branch strokes: each theme buds
            // beside the mural, level with its own ideas.
            [
                center[0]
                    + if left {
                        -mural_w / 2.0 - 12.0
                    } else {
                        mural_w / 2.0 + 12.0
                    },
                fork_y,
            ]
        } else if mural {
            center
        } else {
            [
                center[0] + if left { -64.0 } else { 64.0 },
                center[1] + if top { -24.0 } else { 24.0 },
            ]
        };
        // Places and strokes are keyed by the branch they belong to: a node that changes
        // branch pops out of one and into the other instead of crossing the map.
        let theme = id.clone().unwrap_or_else(|| format!("lens-{group}"));
        strokes.push(kit::span(
            &s.bearings,
            format!("branch-{group}-{theme}"),
            (start, [fork_x, fork_y]),
            |shape, from, to| curve(shape, from, to, ink, 9.0),
        ));
        let leaf_x = if left {
            12.0 + indent
        } else {
            width - leaf_w - 12.0 - indent
        };
        let mut heading = row()
            .key(format!("theme-heading-{theme}"))
            .gap(8.0)
            .center_y();
        if !mural {
            heading = heading.child(canvas(illustration(role)).size(38.0, 34.0));
        }
        labels.push(kit::place(
            format!("place-{group}-theme-{theme}"),
            [leaf_x, first_y - 46.0],
            heading.child(kit::chip(
                name,
                ink,
                leaf_w - if mural { 0.0 } else { 46.0 },
            )),
        ));
        // Each card meets its twig at a bud on the edge that faces the middle of the map.
        let port = |x: f32, y: f32| [if left { x + leaf_w + 4.0 } else { x - 4.0 }, y + 27.0];
        // Twigs bend inside the clear channel beside their parent's cards, then run straight
        // to their own, so no stroke ever crosses a neighbor's paper.
        let twig = |key: String,
                    (from, reach): ([f32; 2], f32),
                    to: [f32; 2],
                    ink: Color,
                    (thick, bud): (f32, f32)| {
            kit::span(
                &s.bearings,
                format!("twig-{group}-{key}"),
                (from, to),
                |shape, from, to| {
                    let knee = [from[0] - inward * reach, to[1]];
                    let mut shape = curve(shape, from, knee, ink, thick);
                    if (knee[0] - to[0]).abs() >= 0.5 {
                        shape = shape.line(knee, to, thick * 0.37, LineCap::Round, ink);
                    }
                    if bud > 0.0 {
                        shape = shape.line(
                            [to[0] - 0.25, to[1]],
                            [to[0] + 0.25, to[1]],
                            bud,
                            LineCap::Round,
                            ink,
                        );
                    }
                    shape
                },
            )
        };
        let fork = ([fork_x, fork_y], 39.0);
        let entries = &entries[group];
        let mut anchors = std::collections::HashMap::new();
        if let Some(id) = &groups[group].0 {
            anchors.insert(id.clone(), fork);
        }
        if entries.is_empty() {
            labels.push(kit::place(
                format!("place-{group}-empty"),
                [leaf_x, first_y],
                row()
                    .key(format!("empty-branch-{group}"))
                    .w(leaf_w)
                    .h(54.0)
                    .px(14.0)
                    .center_y()
                    .radius(14.0)
                    .fill(ink.alpha(0.05))
                    .border(1.0, ink.alpha(0.16))
                    .child(label("Let a thought take root…", 13.0, MUTED)),
            ));
            strokes.push(twig(
                "empty".into(),
                fork,
                port(leaf_x, first_y),
                ink.alpha(0.4),
                (2.5, 0.0),
            ));
        }
        for (index, entry) in entries.iter().enumerate() {
            let id = entry.id;
            let words = &entry.words;
            let leaf_x = leaf_x - inward * entry.depth as f32 * 88.0;
            let y = first_y + index as f32 * 72.0;
            let end = port(leaf_x, y);
            let parent = entry
                .parent
                .as_ref()
                .and_then(|id| anchors.get(id))
                .copied()
                .unwrap_or(fork);
            // A draft already holds the place of the thought it will become.
            let name = match &entry.theme {
                Some(theme) => format!("theme-{theme}"),
                None => format!("thought-{}", id.unwrap_or(s.projection.session.next_id)),
            };
            strokes.push(twig(
                name.clone(),
                parent,
                end,
                ink,
                if entry.theme.is_some() {
                    (5.0 - entry.depth as f32, 9.0)
                } else if depth == 0 {
                    (3.5, 7.0)
                } else {
                    (2.5, 7.0)
                },
            ));
            let place = format!("place-{group}-{name}");
            if let Some(theme) = &entry.theme {
                // Its own thoughts peel off the incoming branch just before it lands.
                anchors.insert(theme.clone(), ([end[0] + inward * 30.0, end[1]], 30.0));
                labels.push(kit::place(
                    place,
                    [leaf_x, y],
                    kit::bough(
                        col()
                            .key(format!("theme-heading-{theme}"))
                            .w(leaf_w)
                            .h(54.0)
                            .px(14.0)
                            .gap(1.0)
                            .justify_center(),
                        ink,
                    )
                    .child(kit::eyebrow(
                        if entry.depth == 1 {
                            "SUBTHEME"
                        } else {
                            "CONCEPT GROUP"
                        },
                        ink.alpha(0.72),
                    ))
                    .child(kit::caption(words, 15.0, 600.0, ink).truncate()),
                ));
                continue;
            }
            let selected = id.is_some() && id == s.selected;
            let pinned = id.is_some_and(|id| {
                s.projection
                    .session
                    .thoughts
                    .iter()
                    .any(|t| t.id == id && t.pinned)
            });
            let note = match id {
                None => "Growing · Enter to keep".to_string(),
                Some(_) if s.modern_ui => String::from(if pinned { "Pinned" } else { "" }),
                Some(id) => format!("{:02}{}", id, if pinned { " · pinned" } else { "" }),
            };
            let mut node = kit::leaf(
                col()
                    .key(id.map_or_else(|| "draft-preview".into(), |id| format!("thought-{id}")))
                    // A kept draft becomes its thought on the spot; the place it holds does the
                    // arriving and the leaving.
                    .transition(rune::Transition::none())
                    .instant_changes()
                    .w(leaf_w)
                    .h(54.0)
                    .px(14.0)
                    .gap(1.0)
                    .justify_center(),
                ink,
                if selected {
                    1.0
                } else if id.is_none() {
                    0.45
                } else {
                    0.0
                },
            )
            .role("button")
            .focusable(id.is_some())
            .child(
                label(
                    short_label(words),
                    13.5,
                    if id.is_some() { INK } else { MUTED },
                )
                .font(TextStyle::new(13.5, 500.0))
                .wrap_anywhere()
                .max_lines(2),
            );
            if !note.is_empty() {
                node = node.child(kit::caption(note, 10.0, 600.0, ink));
            }
            if let Some(id) = id {
                node = node.on_press(move |s: &mut State| {
                    if s.replay.is_none() {
                        s.selected = Some(id);
                    }
                });
            }
            labels.push(kit::place(place, [leaf_x, y], node));
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
    let cloud_paper = || {
        canvas(
            Shape::new()
                .fill_path(&cloud, WHITE)
                .stroke_path(&cloud, 2.5, LineCap::Round, GOLD),
        )
        .absolute(0.0, 0.0)
        .size(width, height)
    };
    let synthesis_size = (width - 2.0 * (leaf_w + indent + 26.0)).clamp(250.0, 420.0);
    let synthesis_coverage = if cx.is_reduced_motion() {
        if s.gallery
            .layers
            .iter()
            .any(|l| l.key == generated_art::CENTRAL_KEY && l.retired.is_none())
        {
            1.0
        } else {
            0.0
        }
    } else {
        s.gallery.coverage(generated_art::CENTRAL_KEY)
    };
    // Let the synthesis take visual priority where the inner theme margins overlap it.
    let theme_visibility = |x: f32, y: f32, size: f32| {
        let half = synthesis_size / 2.0;
        if x < center[0] + half
            && x + size > center[0] - half
            && y < center[1] + half
            && y + size > center[1] - half
        {
            1.0 - 0.85 * synthesis_coverage
        } else {
            1.0
        }
    };
    let mut sheet = el().key("tree-sheet").w(width).h(height);
    let mut art_layer = el()
        .key("background-art")
        .absolute(0.0, 0.0)
        .size(width, height);
    if s.art_style != Style::Off && !mural {
        let size = (width / 2.0 - leaf_w - 94.0 - indent).clamp(56.0, 124.0);
        for layer in &s.art.layers {
            if cx.is_reduced_motion() && layer.retiring() {
                continue;
            }
            let slot = layer.concept.slot;
            let left = slot % 2 == 0;
            let x = if left {
                leaf_w + 25.0 + indent
            } else {
                width - leaf_w - 25.0 - size - indent
            };
            let y = offsets.get(slot / 2).copied().unwrap_or(height - 180.0)
                - if slot < 2 { 20.0 } else { 70.0 };
            let pigment = if s.art_style == Style::Ink {
                INK
            } else {
                color(Role::ALL[slot % 4])
            };
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
                .opacity(
                    opacity
                        * (1.0 - s.gallery.coverage(&layer.concept.key))
                        * theme_visibility(x, y, size),
                ),
            );
        }
    }
    if s.art_style != Style::Off {
        let size = (width / 2.0 - leaf_w - 94.0 - indent).clamp(56.0, 140.0);
        for layer in &s.gallery.layers {
            if mural {
                if layer.key != generated_art::CENTRAL_KEY
                    || layer.mural.is_none()
                    || (cx.is_reduced_motion() && layer.retired.is_some())
                {
                    continue;
                }
                // Crossfade complete scenes; a strip reveal would break continuity.
                let alpha = if cx.is_reduced_motion() {
                    1.0
                } else {
                    layer.opacity(s.gallery.now) * layer.progress(s.gallery.now)
                };
                art_layer = art_layer.child(
                    el().key(format!("generated-{}-{}", layer.key, layer.fingerprint))
                        .absolute(center[0] - mural_w / 2.0, center[1] - mural_h / 2.0)
                        .size(mural_w, mural_h)
                        .opacity(alpha)
                        .pixels(layer.image.pixels()),
                );
                continue;
            }
            if cx.is_reduced_motion() && layer.retired.is_some() {
                continue;
            }
            let slot = layer.slot;
            let x = if slot % 2 == 0 {
                leaf_w + 25.0 + indent
            } else {
                width - leaf_w - 25.0 - size - indent
            };
            let y = offsets.get(slot / 2).copied().unwrap_or(height - 180.0)
                - if slot < 2 { 20.0 } else { 70.0 };
            let central = layer.key == generated_art::CENTRAL_KEY;
            let size = if central { synthesis_size } else { size };
            let (x, y) = if central {
                (center[0] - size / 2.0, center[1] - size / 2.0)
            } else {
                (x, y)
            };
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
                .opacity(
                    opacity * progress.min(0.4) / 0.4
                        * if central {
                            0.82
                        } else {
                            theme_visibility(x, y, size)
                        },
                )
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
    let mut branches = el().key("tree-branches").size(width, height);
    for stroke in strokes {
        branches = branches.child(stroke);
    }
    if !mural {
        branches = branches.child(cloud_paper().key("tree-cloud"));
    }
    sheet = if mural {
        sheet.child(branches).child(art_layer)
    } else {
        sheet.child(art_layer).child(branches)
    };
    if !mural && synthesis_coverage > 0.0 && s.art_style != Style::Off {
        // Keep the title legible over the synthesis artwork.
        sheet = sheet.child(cloud_paper().key("synthesis-title-mask"));
    }
    for node in labels {
        sheet = sheet.child(node);
    }
    let seeking = s.busy && !cx.is_reduced_motion();
    let eyebrow = row()
        .gap(6.0)
        .center_y()
        .child(kit::icon(kit::Icon::Spark, 11.0, GOLD))
        .child(kit::eyebrow(
            if s.busy {
                "FINDING CONNECTIONS"
            } else {
                "THE CENTRAL IDEA"
            },
            GOLD,
        ))
        .opacity(if seeking { 0.55 + 0.45 * s.pulse } else { 1.0 });
    sheet = sheet.child(if let Some(hub) = hub {
        // The card hugs its title; the box around it only centres the card on the hub.
        let mut card = col()
            .px(26.0)
            .py(15.0)
            .gap(3.0)
            .center()
            .radius(26.0)
            .fill(WHITE)
            .border(1.5, GOLD.alpha(0.55))
            .shadow(1.0, 2.0, INK.alpha(0.05))
            .shadow(10.0, 30.0, GOLD.alpha(0.22))
            .child(eyebrow.mb(3.0));
        for line in balanced(&short_label(&s.projection.session.title)) {
            card = card.child(kit::caption(line, 18.0, 600.0, INK).truncate());
        }
        col()
            .key("central-idea")
            .absolute(hub[0] - 190.0, hub[1] - 60.0)
            .size(380.0, 120.0)
            .center()
            // The hub moves with the branches that start from it.
            .motion(kit::SNAP)
            .motion_intent(rune::MotionIntent::OVERSHOOT)
            .child(card.max_w(380.0))
    } else if mural {
        col()
            .key("central-idea")
            .absolute(center[0] - mural_w / 2.0, center[1] - mural_h / 2.0 - 76.0)
            .size(mural_w, 60.0)
            .center()
            .gap(6.0)
            .child(
                kit::caption(short_label(&s.projection.session.title), 20.0, 600.0, INK).truncate(),
            )
            .child(eyebrow)
    } else {
        col()
            .key("central-idea")
            .absolute(center[0] - 78.0, center[1] - 62.0)
            .w(156.0)
            .h(124.0)
            .center()
            .gap(5.0)
            .child(canvas(illustration(Role::Observation)).size(32.0, 26.0))
            .child(
                label(short_label(&s.projection.session.title), 16.0, INK)
                    .font(TextStyle::new(16.0, 600.0)),
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
            )
    });
    if s.modern_ui {
        // Frame the ideas and what they grow from, not the unused paper below them.
        let focus = if let Some(hub) = hub {
            extent.max(hub[1] + 60.0) + 9.0
        } else if mural {
            extent.max(center[1] + mural_h / 2.0) + 9.0
        } else {
            height
        };
        return workspace::canvas(s, sheet, [width, focus.min(height)]);
    }
    let map = col()
        .key("map")
        .on_layout(|s: &mut State, rect| {
            if s.replay.is_none() {
                s.map_viewport = [rect[2], rect[3]];
            }
        })
        .on_scroll_offset({
            let offset = s.map_scroll.clone();
            move |pos| {
                offset.set(pos.offset);
            }
        })
        .w(0.0)
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .h_full()
        .child(sheet)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    if s.replay.is_some() && s.map_viewport.iter().all(|v| *v > 0.0) {
        map.w(s.map_viewport[0])
            .min_w(s.map_viewport[0])
            .max_w(s.map_viewport[0])
            .h(s.map_viewport[1])
            .min_h(s.map_viewport[1])
            .max_h(s.map_viewport[1])
            .grow(0.0)
    } else {
        map
    }
}
fn save_video_button(s: &State) -> El {
    let has_video = s.first_video_frame_ms().is_some();
    enabled(
        kit::tool(
            Some(kit::Icon::Save),
            if s.video_export.is_some() {
                "Saving video…"
            } else {
                "Save video"
            },
            "export-video",
            |_| {},
        )
        .on_press_tx(State::choose_video_file)
        .help(if has_video {
            "Choose where to save the original video and audio. Live capture pauses while saving."
        } else {
            "This session has no recorded video frames. Scene playback is still available."
        }),
        has_video && s.can_play_session(),
    )
}
fn saved_video_button(s: &State) -> El {
    if let Some(path) = &s.saved_video {
        kit::tool(
            Some(kit::Icon::Folder),
            if cfg!(target_os = "macos") {
                "Show in Finder"
            } else {
                "Show in folder"
            },
            "show-video",
            State::show_saved_video,
        )
        .help(path.display().to_string())
    } else {
        el()
    }
}
fn session_video_controls(s: &State) -> El {
    if s.journal.is_none() || s.replay.is_some() || s.archive_only {
        return el();
    }
    row()
        .gap(2.0)
        .center_y()
        .child(enabled(
            kit::tool(
                Some(kit::Icon::Play),
                if s.first_video_frame_ms().is_some() {
                    "Play video"
                } else {
                    "Play session"
                },
                "replay",
                State::play_session,
            )
            .help(if s.can_play_session() {
                "Watch this session from the beginning with its original audio."
            } else {
                "Finish recording, transcription or map guidance before playback."
            }),
            s.can_play_session(),
        ))
        .child(save_video_button(s))
        .child(saved_video_button(s))
}
fn replay_controls(s: &State) -> El {
    let replay = s.replay.as_ref().unwrap();
    let end = s.journal.as_ref().map_or(0, |j| j.duration());
    // How much of the session has played, as a share of the track.
    let played = if end == 0 {
        0.0
    } else {
        (replay.at_ms as f32 / end as f32).clamp(0.0, 1.0)
    };
    let transport = row()
        .gap(4.0)
        .center_y()
        .child(kit::tool(
            Some(kit::Icon::Rewind),
            "−10s",
            "replay-back",
            |s| s.seek_replay(-10000),
        ))
        .child(kit::primary(
            Some(if replay.playing {
                kit::Icon::Pause
            } else {
                kit::Icon::Play
            }),
            if replay.playing { "Pause" } else { "Play" },
            "replay-play",
            INK,
            |s| {
                let end = s.journal.as_ref().map_or(0, |j| j.duration());
                let start = s.replay_start_ms();
                if let Some(r) = &mut s.replay {
                    if r.at_ms >= end {
                        r.at_ms = start;
                    }
                    r.playing = !r.playing;
                    r.last_tick = None;
                }
            },
        ))
        .child(kit::tool(
            Some(kit::Icon::Forward),
            "+10s",
            "replay-forward",
            |s| s.seek_replay(10000),
        ))
        .child(kit::tool(
            Some(kit::Icon::Next),
            "Next event",
            "replay-next",
            State::next_replay_event,
        ))
        .child(
            kit::caption(
                format!(
                    "{} / {}",
                    tacit::timeline::timestamp(replay.at_ms),
                    tacit::timeline::timestamp(end)
                ),
                12.5,
                600.0,
                INK,
            )
            .ml(8.0),
        );
    let actions = row()
        .gap(2.0)
        .center_y()
        .child(if s.first_video_frame_ms().is_some() {
            kit::tool(
                Some(kit::Icon::Film),
                if s.pixel_replay {
                    "Scene replay"
                } else {
                    "Video playback"
                },
                "replay-renderer",
                |s| {
                    s.pixel_replay = !s.pixel_replay;
                    s.seek_replay(0);
                    if let Some(replay) = &mut s.replay {
                        replay.last_projected = None;
                    }
                },
            )
        } else {
            kit::caption("Scene playback · no recorded video", 11.5, 500.0, MUTED).px(8.0)
        })
        .child(save_video_button(s))
        .child(saved_video_button(s))
        .child(if s.archive_only {
            kit::caption("Saved archive", 11.5, 500.0, MUTED).px(8.0)
        } else {
            enabled(
                kit::tool(
                    Some(kit::Icon::Back),
                    "Back to live",
                    "back-live",
                    State::toggle_replay,
                ),
                !s.video_busy(),
            )
        });
    kit::card(col().key("replay-controls"))
        .gap(10.0)
        .padding(12.0)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .flex_wrap()
                .child(transport)
                .child(el().grow(1.0))
                .child(actions),
        )
        .child(
            row()
                .h(4.0)
                .mx(6.0)
                .radius(2.0)
                .fill(kit::wash(1.6))
                .clip_rounded(2.0)
                .child(el().w(0.0).grow(played).h_full().fill(GREEN))
                .child(el().w(0.0).grow(1.0 - played)),
        )
        .child(
            label(s.replay_caption(), 11.5, MUTED)
                .px(6.0)
                .wrap_anywhere(),
        )
}
fn sessions_button(s: &State) -> El {
    if s.library_base.is_none() {
        return el();
    }
    kit::tool(Some(kit::Icon::Sidebar), "Sessions", "sessions", |s| {
        if !s.read_only
            && s.replay.is_none()
            && let Some(journal) = &mut s.journal
            && let Err(e) = journal.end_pixels()
        {
            s.status = format!("Cannot pause capture: {e}");
            return;
        }
        s.refresh_sessions();
        s.session_name_edit = TextEdit::new(&s.session_name);
        s.sessions_open = !s.sessions_open;
    })
}
fn sessions_view(s: &State) -> El {
    let mut items = col().gap(8.0).grow(1.0).min_h(0.0).scroll(ScrollSpec {
        stick_bottom: false,
    });
    for (index, entry) in s.session_entries.iter().enumerate() {
        let path = entry.path.clone();
        let current = s.path.as_ref() == Some(&path);
        items = items.child(
            button(
                &format!(
                    "{}{}",
                    entry.name,
                    if current { "  ·  current" } else { "" }
                ),
                &format!("session-{index}"),
                move |s| s.switch_session(path.clone()),
            )
            .fill(if current { LINE } else { WHITE }),
        );
    }
    col()
        .w_full()
        .h_full()
        .padding(28.0)
        .gap(18.0)
        .fill(PAPER)
        .child(
            row()
                .center_y()
                .child(label("Your sessions", 28.0, INK).grow(1.0))
                .child(button("Back", "sessions-back", |s| s.sessions_open = false)),
        )
        .child(label(
            "Separate thoughts, recordings, artwork, and replay history for every session.",
            14.0,
            MUTED,
        ))
        .child(items)
        .child(
            text_input(&s.session_name_edit, TextStyle::new(16.0, 400.0))
                .key("session-name")
                .h(44.0)
                .color(INK)
                .placeholder("Session name", MUTED)
                .on_edit(|s: &mut State| &mut s.session_name_edit),
        )
        .child(
            row()
                .gap(12.0)
                .child(button(
                    "New session",
                    "session-create",
                    State::create_session,
                ))
                .child(button(
                    "Rename current",
                    "session-rename",
                    State::rename_session,
                )),
        )
        .child(label(s.status.clone(), 12.0, MUTED))
}
fn legacy_view(s: &State, cx: &mut Cx<State>) -> El {
    if s.sessions_open {
        return sessions_view(s);
    }
    if s.replay.is_some() {
        cx.reduced_motion(s.reduced_motion);
    }
    let reduced_motion = cx.is_reduced_motion();
    let wide = if s.replay.is_some() {
        s.viewport[0] >= 1080.0
    } else {
        cx.layout_size("root").is_none_or(|size| size[0] >= 1080.0)
    };
    let header = row()
        .w_full()
        .center_y()
        .gap(16.0)
        .child(label("tacit", 32.0, INK).font(TextStyle::new(32.0, 500.0).tracking(-1.5)))
        .child(
            label(
                if !s.session_name.is_empty() {
                    &s.session_name
                } else {
                    "A FIELD GUIDE TO YOUR THINKING"
                },
                10.0,
                MUTED,
            )
            .grow(1.0),
        )
        .child(sessions_button(s))
        .child(button(
            if s.story {
                "Show map"
            } else {
                "Read the thread"
            },
            "toggle-story",
            |s| {
                if s.replay.is_none() {
                    s.story = !s.story;
                }
            },
        ))
        .child(button(s.art_style.label(), "art-style", |s| {
            if s.replay.is_none() {
                s.art_style = s.art_style.toggle();
            }
        }))
        .child(button("Undo", "undo", State::undo));
    let title = col()
        .gap(8.0)
        .child(label("Let your thoughts branch out.", 20.0, INK))
        .child(label(
            format!(
                "{} thoughts  /  one living tree  /  follow a branch",
                s.projection.session.thoughts.len()
            ),
            12.0,
            MUTED,
        ));
    let main = if s.story {
        col().key("story").on_scroll_offset({let offset=s.story_scroll.clone();move |p|{offset.set(p.offset);}}).grow(1.0).min_h(0.0).gap(20.0).padding(26.0)
            .child(label("THE THREAD SO FAR", 11.0, GREEN))
            .child(label(s.projection.session.narrative(), 23.0, INK).selectable(true))
            .child(label(if s.projection.session.interpretation.is_some() { "A model interpretation. Bracketed numbers refer to your original thoughts; relationships remain hypotheses." } else { "A reading order assembled from your words, not an inferred causal explanation." }, 12.0, MUTED))
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
            .child(if s.direction_mode {
                button("Cancel", "voice-cancel", |s| {
                    s.voice_command(VoiceCommand::Cancel)
                })
            } else {
                el()
            })
            .child(
                button(
                    if s.direction_mode {
                        "Stop & direct"
                    } else {
                        "Finish recording"
                    },
                    "voice-stop",
                    |s| s.voice_command(VoiceCommand::Finish),
                )
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
        .child(
            row()
                .center_y()
                .gap(12.0)
                .child(button(
                    if s.direction_mode {
                        "Brainstorm"
                    } else {
                        "Brainstorm ✓"
                    },
                    "mode-brainstorm",
                    |s| {
                        if s.direction_mode {
                            s.toggle_direction();
                        }
                    },
                ))
                .child(button(
                    if s.direction_mode {
                        "Direct the map ✓"
                    } else {
                        "Direct the map"
                    },
                    "mode-direct",
                    |s| {
                        if !s.direction_mode {
                            s.toggle_direction();
                        }
                    },
                ))
                .child(label(
                    if s.direction_mode {
                        "Instructions change the map; they never become thoughts."
                    } else {
                        "Capture ideas · switch to Direct the map for editing instructions"
                    },
                    12.0,
                    MUTED,
                )),
        )
        .child(voice_controls)
        .child(if s.live_text.is_empty() {
            el()
        } else {
            col()
                .gap(3.0)
                .child(label(
                    "LIVE TRANSCRIPT · provisional until the speech segment finishes",
                    10.0,
                    MUTED,
                ))
                .child(
                    label(s.live_text.clone(), 14.0, INK)
                        .key("live-transcript")
                        .max_h(56.0)
                        .scroll(ScrollSpec { stick_bottom: true }),
                )
        })
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
                    text_input(
                        if s.direction_mode {
                            &s.direction_draft
                        } else {
                            &s.draft
                        },
                        TextStyle::new(16.0, 400.0),
                    )
                    .key("thought-input")
                    .grow(1.0)
                    .min_w(0.0)
                    .h(44.0)
                    .color(INK)
                    .placeholder(
                        if s.direction_mode {
                            "Move the budget thoughts into Costs; put Decisions first…"
                        } else {
                            "A thought, a question, a loose thread…"
                        },
                        MUTED,
                    )
                    .on_edit(|s: &mut State| {
                        if s.direction_mode {
                            &mut s.direction_draft
                        } else {
                            &mut s.draft
                        }
                    })
                    .on_submit(|s: &mut State, _| s.capture()),
                )
                .child(
                    button(
                        if s.direction_mode {
                            if s.directing() {
                                "Considering…"
                            } else {
                                "Apply direction ↗"
                            }
                        } else {
                            "Keep thought ↗"
                        },
                        "capture",
                        State::capture,
                    )
                    .fill(PAPER),
                ),
        )
        .child(label(
            if s.direction_mode {
                s.direction_status.clone()
            } else if s.draft.text.trim().is_empty() {
                "Enter to keep  ·  Shift+Enter for a new line  ·  your words stay yours".into()
            } else if !s.projection.session.categories.is_empty() {
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
        .on_layout(move |s: &mut State, rect| {
            if s.replay.is_none() {
                s.viewport = [rect[2], rect[3]];
                s.reduced_motion = reduced_motion;
            }
        })
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
    let composer = if s.replay.is_some() {
        replay_controls(s)
    } else {
        composer
    };
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
                button("Retry art", "retry-art", |s| {
                    if s.replay.is_none() {
                        s.gallery.compose()
                    }
                })
            } else {
                el()
            })
            .child(if s.journal.is_some() && !s.archive_only {
                button(
                    if s.replay.is_some() {
                        "Back to live"
                    } else {
                        "Replay"
                    },
                    "replay",
                    State::toggle_replay,
                )
            } else {
                el()
            })
            .child(
                if s.replay.is_none()
                    && s.journal.as_ref().is_some_and(|j| {
                        j.projection.audio.iter().any(|a| {
                            !j.projection
                                .transcripts
                                .iter()
                                .any(|(id, _, _)| id == &a.id)
                        })
                    })
                {
                    button(
                        "Retry transcript",
                        "retry-transcript",
                        State::retry_transcript,
                    )
                } else {
                    el()
                },
            )
            .child(if s.replay.is_none() {
                button("Retry save", "save", State::save)
            } else {
                el()
            }),
    )
}
fn inspector(s: &State) -> El {
    let mut panel = col()
        .key("inspector")
        .on_scroll_offset({
            let offset = s.inspector_scroll.clone();
            move |p| {
                offset.set(p.offset);
            }
        })
        .w(230.0)
        .h_full()
        .gap(16.0)
        .child(label("IN THE MARGIN", 10.0, MUTED))
        .child(label("Follow the thought.", 22.0, INK));
    if let Some(t) = s
        .projection
        .session
        .thoughts
        .iter()
        .chain(s.projection.session.retired.iter().map(|r| &r.thought))
        .find(|t| Some(t.id) == s.selected)
    {
        panel = panel
            .child(label("CONCEPT", 10.0, color(t.role)))
            .child(
                label(
                    t.concept.as_deref().unwrap_or("Concept not yet distilled"),
                    22.0,
                    INK,
                )
                .key("selected-concept")
                .selectable(true),
            )
            .child(label(
                format!("ORIGINAL WORDS · {:02}", t.id),
                10.0,
                color(t.role),
            ))
            .child(
                label(t.text.clone(), 18.0, INK)
                    .key("selected-original")
                    .wrap_anywhere()
                    .selectable(true),
            )
            .child(label(s.projection.session.placement_path(t), 12.0, MUTED))
            .child(
                if s.read_only
                    || s.projection
                        .session
                        .retired
                        .iter()
                        .any(|r| r.thought.id == t.id)
                {
                    el()
                } else {
                    button("Move to next branch", "move", State::move_selected)
                },
            )
            .child(
                if s.read_only
                    || s.projection
                        .session
                        .retired
                        .iter()
                        .any(|r| r.thought.id == t.id)
                {
                    el()
                } else {
                    button("Remove thought", "remove", State::remove_selected)
                },
            );
    } else {
        panel = panel.child(label(
            "Select a thought to inspect its original words or change where it belongs.",
            14.0,
            MUTED,
        ));
    }
    if let Some(retired) = s
        .projection
        .session
        .retired
        .iter()
        .find(|r| Some(r.thought.id) == s.selected)
    {
        panel = panel
            .child(label("PRIOR CONCEPT", 10.0, MUTED))
            .child(label(retired.reason.clone(), 14.0, INK));
        for id in &retired.replaced_by {
            let id = *id;
            panel = panel.child(button(
                &format!("Follow replacement {id}"),
                &format!("replacement-{id}"),
                move |s| s.selected = Some(id),
            ));
        }
    } else if !s.read_only
        && s.projection
            .session
            .thoughts
            .iter()
            .any(|t| Some(t.id) == s.selected && t.pinned)
    {
        panel = panel.child(button("Allow AI to revise", "unpin", State::unpin_selected));
    }
    if !s.projection.session.retired.is_empty() {
        panel = panel.child(label("HOW THE MAP EVOLVED", 10.0, MUTED));
        for retired in s.projection.session.retired.iter().rev() {
            let id = retired.thought.id;
            panel = panel.child(button(
                &retired
                    .thought
                    .concept
                    .clone()
                    .unwrap_or_else(|| format!("Prior thought {id}")),
                &format!("retired-{id}"),
                move |s| s.selected = Some(id),
            ));
        }
    }
    let selected_theme = s
        .projection
        .session
        .thoughts
        .iter()
        .chain(s.projection.session.retired.iter().map(|r| &r.thought))
        .find(|t| Some(t.id) == s.selected)
        .and_then(|t| t.category.as_ref())
        .and_then(|id| {
            s.projection
                .session
                .category_path(Some(id))
                .first()
                .map(|c| format!("theme-{}", c.id))
        });
    if s.art_style != Style::Off
        && let Some(art) = s.gallery.layers.iter().rev().find(|l| {
            l.retired.is_none()
                && if s.gallery.mural_layout {
                    l.mural.is_some() && l.key == generated_art::CENTRAL_KEY
                } else {
                    selected_theme.as_ref().is_none_or(|key| &l.key == key)
                }
        })
    {
        panel = panel.child(label("VISUAL THREAD", 10.0, MUTED)).child(
            el().key("art-detail").pixels(art.image.pixels()).size(
                230.0,
                if s.gallery.mural_layout {
                    230.0 * 2.0 / 3.0
                } else {
                    230.0
                },
            ),
        );
    }
    if let Some(i) = &s.projection.session.interpretation {
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
            if s.projection.session.categories.is_empty() {
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
    if s.projection
        .session
        .retired
        .iter()
        .any(|r| Some(r.thought.id) == s.selected)
    {
        return inspector(s).max_h(240.0);
    }
    let mut r = row().gap(10.0).center_y();
    if let Some(retired) = s.projection.session.retired.last() {
        let id = retired.thought.id;
        r = r.child(button("Prior concepts", "concept-history", move |s| {
            s.selected = Some(id)
        }));
    }
    if let Some(t) = s
        .projection
        .session
        .thoughts
        .iter()
        .chain(s.projection.session.retired.iter().map(|r| &r.thought))
        .find(|t| Some(t.id) == s.selected)
    {
        r = r
            .child(
                label(
                    format!(
                        "Selected {:02} · {}",
                        t.id,
                        s.projection.session.placement_path(t)
                    ),
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
    if let Some(t) = s
        .projection
        .session
        .thoughts
        .iter()
        .chain(s.projection.session.retired.iter().map(|r| &r.thought))
        .find(|t| Some(t.id) == s.selected)
    {
        col()
            .gap(6.0)
            .child(r)
            .child(
                label(
                    t.concept.as_deref().unwrap_or("Concept not yet distilled"),
                    15.0,
                    INK,
                )
                .key("selected-concept")
                .selectable(true),
            )
            .child(
                label(t.text.clone(), 13.0, INK)
                    .key("selected-original")
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
pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    if s.replay.is_some() && s.pixel_replay {
        workspace::video(s, cx)
    } else if !s.modern_ui {
        legacy_view(s, cx)
    } else {
        workspace::view(s, cx)
    }
}
pub fn on_key(s: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    if s.modern_ui && s.replay.is_none() && !s.story {
        match event.key {
            Key::Command('+') | Key::Command('=') => {
                s.zoom_center(1.2);
                tx.instant();
                return true;
            }
            Key::Command('-') => {
                s.zoom_center(1.0 / 1.2);
                tx.instant();
                return true;
            }
            Key::Command('0') => {
                s.zoom_center(1.0 / s.camera_zoom.get());
                tx.instant();
                return true;
            }
            Key::Command('*') => {
                s.fit_map();
                tx.instant();
                return true;
            }
            _ => {}
        }
    }
    match event.key {
        Key::Command('s') => {
            s.save();
            true
        }
        Key::Escape => {
            if s.sessions_open {
                s.sessions_open = false;
                return true;
            }
            if s.replay.is_some() {
                return true;
            }
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
    s.wake = Some(wake.clone());
    s.image_cache = rune::ImageCache::new(wake.clone());
    if s.replay.is_some() {
        s.replay_images.clear();
        if let Err(error) = s.prepare_replay() {
            s.status = format!("Cannot replay exactly: {error}");
            s.replay = None;
            s.read_only = true;
            return;
        }
    }
    if let Some(j) = &s.journal {
        let at = s.replay.as_ref().map_or(j.now_ms(), |r| r.at_ms);
        let scene = if s.replay.is_some() {
            j.replay_scene_at(at).expect("validated presentation")
        } else {
            j.scene_at(at)
        };
        s.restore_scene(scene, at as f64 / 1000.0);
    }
    if s.replay.is_none() {
        s.refresh_sessions();
        s.live_text.clear();
    }
    if s.read_only || rune::is_headless() {
        return;
    }
    s.recover_transcripts();
    let voice_wake = wake.clone();
    let audio_root = s
        .journal
        .as_ref()
        .map(|j| j.root.join("recordings"))
        .unwrap_or_else(|| tacit::timeline::root_for(&storage::default_path()).join("recordings"));
    s.voice_worker = Some(voice::Worker::start(audio_root, move || voice_wake()));
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
            s.organizer_status = AGENT_IDLE.into();
            if std::env::var("TACIT_IMAGE_ART").as_deref() != Ok("off") {
                s.gallery.worker = Some(generated_art::Worker::start(config.clone(), wake.clone()));
                s.gallery.status = "Art · ready to compose a mural".into();
            }
            let recording_wake = wake.clone();
            s.recording_worker = Some(tacit::recording::Worker::start(config.clone(), move || {
                recording_wake()
            }));
            s.agent = Some(tacit::agent::Worker::start(config, move || wake()));
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
pub fn poll(s: &mut State, frame_now: f64, tx: &mut Tx) -> bool {
    let mut export_changed = false;
    if let Some(receiver) = &s.video_export {
        match receiver.try_recv() {
            Ok(result) => {
                match result {
                    Ok(path) => {
                        s.status = format!("Video saved: {}", path.display());
                        s.saved_video = Some(path);
                    }
                    Err(error) => s.status = format!("Video export failed: {error}"),
                }
                s.video_export = None;
                export_changed = true;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                s.status = "Video export stopped unexpectedly".into();
                s.video_export = None;
                export_changed = true;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                tx.wake_after(std::time::Duration::from_millis(100))
            }
        }
    }
    if s.replay.is_some() {
        let replay = s.replay.as_mut().unwrap();
        if replay.playing {
            if let Some((start, at_ms)) = replay.last_tick {
                replay.at_ms = at_ms.saturating_add(((frame_now - start).max(0.0) * 1000.0) as u64);
            } else {
                replay.last_tick = Some((frame_now, replay.at_ms));
            }
            if let (Some((id, player)), Some(journal)) = (&s.replay_audio, &s.journal)
                && let Some(audio) = journal.projection.audio.iter().find(|a| &a.id == id)
                && replay.at_ms >= audio.start_ms
                && replay.at_ms < audio.start_ms + audio.duration_ms
            {
                match player.state() {
                    rune::PlayerState::Playing => {
                        replay.at_ms = audio.start_ms + (player.time() * 1000.0).round() as u64;
                        replay.last_tick = Some((frame_now, replay.at_ms));
                    }
                    rune::PlayerState::Loading => {
                        replay.at_ms = replay
                            .last_projected
                            .unwrap_or(audio.start_ms)
                            .max(audio.start_ms);
                        replay.last_tick = Some((frame_now, replay.at_ms));
                    }
                    rune::PlayerState::Failed(error) => {
                        replay.playing = false;
                        s.status = format!("Replay stopped: audio unavailable: {error}");
                    }
                    _ => {}
                }
            }
            let end = s.journal.as_ref().map_or(0, |j| j.duration());
            if replay.at_ms >= end {
                replay.at_ms = end;
                replay.playing = false;
            }
            tx.wake_after(std::time::Duration::from_millis(16));
        }
        if !replay.playing {
            replay.last_tick = None;
        }
        let (at, playing) = (replay.at_ms, replay.playing);
        let changed = replay.last_projected != Some(at);
        replay.last_projected = Some(at);
        let projected_scene = if changed {
            s.journal.as_ref().map(|j| {
                let projection = j.at(at).expect("validated timeline");
                let scene = projection.replay_scene().expect("validated presentation");
                (projection, scene)
            })
        } else {
            None
        };
        if let Some((projection, scene)) = projected_scene {
            let scroll = scene.map_scroll;
            let story_scroll = scene.story_scroll;
            let inspector_scroll = scene.inspector_scroll;
            let workspace = scene.workspace.clone();
            s.restore_projected_scene(scene, at as f64 / 1000.0, Some(projection));
            tx.scroll_to_instant("map", scroll[0], scroll[1]);
            tx.scroll_to_instant("story", story_scroll[0], story_scroll[1]);
            tx.scroll_to_instant("inspector", inspector_scroll[0], inspector_scroll[1]);
            if let Some(w) = &workspace {
                tx.scroll_to_instant(
                    "transcript-scroll",
                    w.transcript_scroll[0],
                    w.transcript_scroll[1],
                );
            }
        }
        let audio = s
            .journal
            .as_ref()
            .and_then(|j| {
                j.projection.audio.iter().find(|a| {
                    at.saturating_add(1000) >= a.start_ms && at < a.start_ms + a.duration_ms
                })
            })
            .cloned();
        if let Some(audio) = audio {
            if s.replay_audio
                .as_ref()
                .is_none_or(|(id, _)| id != &audio.id)
            {
                if let Some((_, p)) = s.replay_audio.take() {
                    p.pause();
                }
                if let Some(path) = s.journal.as_ref().and_then(|j| j.asset(&audio.asset).ok()) {
                    let player = rune::Player::audio(path);
                    player.seek(at.saturating_sub(audio.start_ms) as f64 / 1000.0);
                    s.replay_audio = Some((audio.id, player));
                }
            }
            if let Some((_, p)) = &s.replay_audio {
                if let rune::PlayerState::Failed(error) = p.state() {
                    s.status = format!("Replay audio unavailable: {error}");
                }
                let desired = at.saturating_sub(audio.start_ms) as f64 / 1000.0;
                if (p.time() - desired).abs() > 0.15 {
                    p.seek(desired);
                }
                if playing && at >= audio.start_ms {
                    if !p.is_playing() {
                        p.play();
                    }
                } else {
                    p.pause();
                }
            }
        } else if let Some((_, p)) = s.replay_audio.take() {
            p.pause();
        }
        return changed || playing || export_changed;
    }
    let now = s
        .journal
        .as_ref()
        .map_or(frame_now, |j| j.now_ms() as f64 / 1000.0);
    let mut changed = poll_live(s, now, tx);
    while s.steps.front().is_some_and(|step| step.0 <= frame_now) {
        s.projection.session = s.steps.pop_front().expect("a step is due").1;
        changed = true;
    }
    // The compositor commits the final layout and original pixels together. Polling
    // before layout must not serialize and sync a second presentation for that frame.
    if !s.capturing_pixels() {
        s.record_presentation("scene changed");
    }
    changed || export_changed
}
fn poll_live(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    let art_deadline = if s.gallery.mural_layout {
        None
    } else {
        s.art.update(&s.projection.session, now)
    };
    let image_changed = s
        .gallery
        .update(&s.projection.session, s.art_style, s.busy, now, tx);
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
        // Words on their way are not a pause, even before they land as a thought.
        if matches!(
            event,
            VoiceEvent::Partial { .. } | VoiceEvent::Segment { .. }
        ) {
            s.pace.speaking(now);
        }
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
        s.pace.speaking(now);
        if !s.modern_ui && !s.draft.text.trim().is_empty() {
            tx.scroll_into_view("draft-preview");
        }
        changed = true;
    }
    if s.revealed_selection != s.selected {
        s.revealed_selection = s.selected;
        if s.modern_ui && s.selected.is_some() {
            tx.scroll_to_instant("transcript-scroll", 0.0, 0.0);
        }
        if !s.modern_ui
            && let Some(id) = s.selected
        {
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
    // Guidance waits for the agent, and without one nothing would ever take it from the queue.
    if (s.read_only || s.agent.is_none()) && s.queued_direction.take().is_some() {
        s.direction_status = "The map agent is not running; your instruction is kept here".into();
        changed = true;
    }
    if s.busy {
        s.pulse = ((now * 3.0).sin() as f32 + 1.0) / 2.0;
        changed = true;
    }
    changed |= if s.agent.is_some() {
        attend(s, now, tx)
    } else if s.worker.is_some() {
        organize_locally(s, now, tx)
    } else {
        std::mem::take(&mut s.busy)
    };
    if s.busy {
        tx.wake_after(std::time::Duration::from_millis(33));
    }
    changed
}
/// The map agent's side of a frame: take in the turns that came back, then ask for the next.
fn attend(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    use tacit::agent::{self, Tier, brief, pace::Due};
    let mut changed = false;
    while !s.read_only
        && let Some(response) = s.agent.as_ref().and_then(|a| a.receive.try_recv().ok())
    {
        changed = true;
        let tier = response.tier;
        if tier == Tier::Direct {
            s.receive_direction(response, now);
            continue;
        }
        let turn = match response.result {
            Ok(turn) if s.pace.current(response.epoch) => turn,
            // Briefed before the user took the map into their own hands.
            Ok(_) => {
                s.pace.dropped(tier);
                if !s.directing() {
                    s.organizer_status = AGENT_IDLE.into();
                }
                continue;
            }
            Err(error) => {
                s.pace.failed(tier, now);
                s.organizer_status = error;
                continue;
            }
        };
        let waiting =
            |s: &State| brief::waiting(&s.projection.session, &s.pending_recordings).len();
        let before = waiting(s);
        // The speaker may have said more since the turn was briefed, so its edits are tried
        // against the map as it is now.
        let outcome = agent::apply(
            &s.projection.session,
            &turn.ops,
            tier,
            &s.pending_recordings,
        );
        let said = report(&s.projection.session, tier, &outcome.ops, &turn.note);
        if !outcome.ops.is_empty() {
            // Filing keeps up with the speaker. A restructure is a step they can undo.
            if tier == Tier::Steward {
                s.checkpoint();
            }
            let change = SessionEvent::Mapped {
                ops: outcome.ops,
                pass: tier.name().into(),
                note: turn.note,
            };
            if let Err(error) = s.dispatch_session(change) {
                if tier == Tier::Steward {
                    s.history.pop();
                }
                s.pace.failed(tier, now);
                s.organizer_status = error;
                continue;
            }
            s.settled_at = now;
            s.save();
        }
        s.pace
            .landed(tier, &s.projection.session, waiting(s) < before, now);
        if turn.escalate.is_some() {
            s.pace.call();
        }
        s.organizer_status = said;
    }
    if s.read_only {
        // Nothing more can be written, so no turn is worth paying for.
        (s.agent, s.busy) = (None, false);
        return true;
    }
    changed |= s.start_queued_direction(now);
    let directing = s.directing();
    match s
        .pace
        .due(&s.projection.session, &s.pending_recordings, now)
    {
        Due::Ask(tier) => {
            if s.ask(tier, None, now) {
                s.organizer_status = match tier {
                    Tier::Place => "Finding a home for your thoughts…",
                    _ => "✦ Stepping back · refining the whole map…",
                }
                .into();
            }
            changed = true;
        }
        Due::At(at) => tx.wake_after(std::time::Duration::from_secs_f64((at - now).max(0.01))),
        Due::Rest => {
            if agent::at_limit(&s.projection.session) && s.organizer_status != agent::AT_LIMIT {
                s.organizer_status = agent::AT_LIMIT.into();
                changed = true;
            }
        }
    }
    if directing && !s.directing() {
        // Its lane went quiet for good, and the pace gave up on it.
        s.direction_status = "The map agent did not answer; your instruction is kept here".into();
        s.organizer_status = AGENT_IDLE.into();
        changed = true;
    }
    let busy = s.agent.is_some() && s.pace.busy();
    changed |= busy != s.busy;
    s.busy = busy;
    changed
}
/// The agent's status line when it has nothing to report.
const AGENT_IDLE: &str = "Map agent · files each thought, steps back in the pauses";
/// What a turn's edits did to the map as it stood in `before`, for the status line.
fn report(
    before: &Session,
    tier: tacit::agent::Tier,
    ops: &[tacit::agent::Op],
    note: &str,
) -> String {
    use tacit::agent::{Op, Tier};
    if tier == Tier::Steward {
        return match (ops.is_empty(), note.trim()) {
            (true, _) => "Stepped back · the map holds as it is".into(),
            (false, "") => "✦ Map refined".into(),
            (false, note) => format!("✦ {note}"),
        };
    }
    let parked = |id: &u64| before.parked.iter().any(|p| p.thought.id == *id);
    let (mut filed, mut back, mut aside) = (0, 0, 0);
    for op in ops {
        match op {
            Op::Place { thought, .. } if parked(thought) => back += 1,
            Op::Place { .. } => filed += 1,
            Op::Park { .. } => aside += 1,
            _ => {}
        }
    }
    let thoughts = |n: usize| format!("{n} thought{}", if n == 1 { "" } else { "s" });
    let mut said = [
        (filed, format!("filed {}", thoughts(filed))),
        (back, format!("brought {} back", thoughts(back))),
        (aside, format!("parked {}", thoughts(aside))),
    ]
    .into_iter()
    .filter_map(|(count, part)| (count > 0).then_some(part))
    .collect::<Vec<_>>()
    .join(" · ");
    if said.is_empty() {
        return "Still weighing where these belong…".into();
    }
    said[..1].make_ascii_uppercase();
    said
}
/// The local model's side of a frame: one pass over the whole map after each change.
fn organize_locally(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    while let Some(response) = s.worker.as_ref().and_then(|w| w.receive.try_recv().ok()) {
        if response.revision != s.revision {
            continue;
        }
        s.busy = false;
        match response.result.and_then(|interpretation| {
            s.dispatch_session(SessionEvent::Organized {
                interpretation,
                pass: "fast".into(),
            })
        }) {
            Ok(()) => {
                s.organizer_status = "Organized by local model · suggestions".into();
                s.settled_at = now;
                s.save();
            }
            Err(e) => s.organizer_status = format!("{e} · keeping local organization"),
        }
        changed = true;
    }
    if s.observed != s.revision {
        s.observed = s.revision;
        s.due = now + 0.18;
        s.busy = false;
    }
    if s.requested != Some(s.revision) && !s.projection.session.thoughts.is_empty() {
        if now >= s.due {
            let request = tacit::organizer::Request {
                revision: s.revision,
                session: s.projection.session.clone(),
            };
            if s.worker
                .as_ref()
                .is_some_and(|w| w.send.send(request).is_ok())
            {
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
    fn live_speech_is_incremental_idempotent_and_direction_safe() {
        let mut s = State::empty();
        let id = "recording-live".to_owned();
        s.receive_voice(
            VoiceEvent::Started {
                id: id.clone(),
                started: std::time::Instant::now(),
            },
            0.0,
        );
        s.receive_voice(
            VoiceEvent::Partial {
                id: id.clone(),
                text: "An unfinished".into(),
            },
            0.1,
        );
        assert!(s.projection.session.thoughts.is_empty());
        assert_eq!(s.live_text, "An unfinished");
        let cue = tacit::timeline::Cue {
            start_ms: 0,
            end_ms: 2000,
            text: "An idea about gardens.".into(),
        };
        s.receive_voice(
            VoiceEvent::Segment {
                id: id.clone(),
                segment: 0,
                cue: cue.clone(),
            },
            2.0,
        );
        assert_eq!(s.projection.session.thoughts.len(), 1);
        assert!(s.live_text.is_empty());
        let snapshot = serde_json::to_value(&s.projection.session).unwrap();
        s.receive_voice(
            VoiceEvent::Segment {
                id: id.clone(),
                segment: 0,
                cue: cue.clone(),
            },
            2.1,
        );
        assert_eq!(
            serde_json::to_value(&s.projection.session).unwrap(),
            snapshot
        );
        // Provenance survives removal, so reconnection cannot resurrect the source.
        s.projection.session.thoughts.clear();
        s.receive_voice(
            VoiceEvent::Segment {
                id,
                segment: 0,
                cue: cue.clone(),
            },
            2.2,
        );
        assert!(s.projection.session.thoughts.is_empty());
        s.direction_mode = true;
        s.receive_voice(
            VoiceEvent::Started {
                id: "recording-direct".into(),
                started: std::time::Instant::now(),
            },
            3.0,
        );
        s.receive_voice(
            VoiceEvent::Segment {
                id: "recording-direct".into(),
                segment: 0,
                cue,
            },
            4.0,
        );
        assert!(s.projection.session.thoughts.is_empty());
        assert!(!s.directing());
    }

    fn mural_fixture() -> tacit::artist::Mural {
        tacit::artist::Mural {
            identity: "fixture-session".into(),
            content: "fixture-content".into(),
            render_policy: "fixture-policy".into(),
            direction: tacit::artist::Direction {
                story: "A team struggles to decide who owns a decision".into(),
                connections: vec![tacit::artist::VisualConnection {
                    thought_ids: vec![2],
                    depiction: "Colleagues hesitate over an unclaimed work item".into(),
                }],
                scene: "Colleagues pause over an unclaimed decision in a modern workspace".into(),
                anchors: vec!["The shared project display".into()],
                change: "Show the unresolved handoff, keeping the decision visibly pending".into(),
            },
            evidence: "fixture-evidence".into(),
            artist_model: tacit::artist::MODEL.into(),
            image_model: generated_art::MODEL.into(),
            parent: Some("previous-scene".into()),
        }
    }
    #[test]
    fn mural_has_an_unobstructed_center_and_legacy_replay_keeps_its_title_cloud() {
        for (size, mural) in [(1680.0, 1000.0), (820.0, 640.0)]
            .into_iter()
            .flat_map(|size| [(size, true), (size, false)])
        {
            let mut rt = runtime(size);
            rt.state = State::hierarchy_demo();
            rt.state.gallery.mural_layout = mural;
            let image = rt.state.image_cache.rgba(1, 1, vec![100, 160, 200, 255]);
            image.wait().unwrap();
            rt.state.gallery.layers.push(generated_art::Layer {
                mural: mural.then(mural_fixture),
                encoded: None,
                key: generated_art::CENTRAL_KEY.into(),
                fingerprint: "fixture".into(),
                slot: 0,
                image,
                born: -10.0,
                retired: None,
            });
            rt.frame(1.0);
            let art = rt.find("generated-map-synthesis-fixture").unwrap();
            let title = rt.find("central-idea").unwrap();
            assert!((art.0 - title.0).abs() < 1.0);
            if mural {
                assert!(title.1 < art.1 - 60.0);
                assert!(rt.find("synthesis-title-mask").is_none());
                assert!(rt.find("theme-art-0").is_none());
            } else {
                assert!((art.1 - title.1).abs() < 1.0);
                assert!(rt.find("synthesis-title-mask").is_some());
            }
            assert!(rt.find("thought-1").is_some());
        }
    }

    #[test]
    fn nested_groups_render_once_and_keep_their_parent_order() {
        for size in [(1680.0, 1000.0), (820.0, 640.0)] {
            let mut rt = runtime(size);
            rt.state = State::hierarchy_demo();
            rt.frame(1.0);
            let root = rt.find("theme-heading-time").unwrap();
            let sub = rt.find("theme-heading-meetings").unwrap();
            let group = rt.find("theme-heading-focus").unwrap();
            assert!(root.0 > sub.0 && sub.0 > group.0);
            assert!(root.1 < sub.1 && sub.1 < group.1);
            let mut entries = Vec::new();
            theme_entries(&rt.state.projection.session, "time", 0, &mut entries);
            assert_eq!(entries.iter().filter(|e| e.id == Some(1)).count(), 1);
            assert_eq!(entries.iter().find(|e| e.id == Some(1)).unwrap().depth, 2);
            assert!(rt.find("thought-1").is_some());
        }
    }

    #[test]
    fn selected_concept_and_original_are_available_in_both_layouts() {
        for size in [(1280.0, 860.0), (820.0, 640.0)] {
            let mut rt = runtime(size);
            let original = rt.state.projection.session.thoughts[0].text.clone();
            rt.state.projection.session.thoughts[0].concept = Some("Meeting overload".into());
            rt.frame(1.0);
            click(&mut rt, "thought-1");
            assert!(rt.find("selected-concept").is_some());
            assert!(rt.find("selected-original").is_some());
            assert_eq!(
                rt.state.projection.session.thoughts[0].map_label(),
                "Meeting overload"
            );
            assert_eq!(rt.state.projection.session.thoughts[0].text, original);
        }
    }
    #[test]
    fn direction_speech_never_becomes_a_thought_and_keeps_the_brainstorm_draft() {
        let mut s = State::empty();
        s.direction_mode = true;
        s.draft = TextEdit::new("An unfinished idea");
        s.receive_voice(VoiceEvent::Transcript("Put decisions first".into()), 1.0);
        assert!(s.projection.session.thoughts.is_empty());
        assert_eq!(s.draft.text, "An unfinished idea");
        assert_eq!(s.direction_draft.text, "Put decisions first");
        // No provider: retain instruction, never fall back to capture.
        assert!(!s.directing() && s.queued_direction.is_none());
        s.voice_state = VoiceState::Recording;
        s.toggle_direction();
        assert!(s.direction_mode);
    }
    #[test]
    fn spoken_guidance_resumes_once_after_splitting_and_refines_the_latest_map() {
        use std::sync::mpsc;
        let mut s = State::empty();
        let recording = "Hosting costs doubled. We could remove unused replicas.";
        s.draft = TextEdit::new(recording);
        s.capture();
        s.draft = TextEdit::new("New hires need mentors.");
        s.capture();
        s.pending_recordings.extend([1, 2]);
        let (send, _split_requests) = mpsc::channel();
        let (split_results, receive) = mpsc::channel();
        s.recording_worker = Some(tacit::recording::Worker { send, receive });
        let (_placing, _stewarding, mut requests, answer) = lanes(&mut s);
        s.direction_mode = true;
        let instruction = "Could you separate hiring from infrastructure costs?";
        s.receive_voice(VoiceEvent::Transcript(instruction.into()), 1.0);
        assert_eq!(s.queued_direction.as_deref(), Some(instruction));
        assert_eq!(s.direction_draft.text, instruction);
        assert_eq!(s.projection.session.thoughts.len(), 2);
        poll_live(&mut s, 1.5, &mut Tx::default());
        assert!(!s.directing());
        assert!(requests.try_recv().is_err());

        split_results
            .send(tacit::recording::Response {
                id: 1,
                text: recording.into(),
                result: Ok(vec![
                    "Hosting costs doubled. ".into(),
                    "We could remove unused replicas.".into(),
                ]),
            })
            .unwrap();
        poll_live(&mut s, 2.0, &mut Tx::default());
        assert!(requests.try_recv().is_err());
        // Failed splitting also finishes the wait, using the retained full thought.
        split_results
            .send(tacit::recording::Response {
                id: 2,
                text: "New hires need mentors.".into(),
                result: Err("Synthetic split failure".into()),
            })
            .unwrap();
        poll_live(&mut s, 3.0, &mut Tx::default());
        let request = requests.try_recv().unwrap();
        assert_eq!(request.instruction.as_deref(), Some(instruction));
        assert_eq!(request.session, s.projection.session);
        assert_eq!(request.session.thoughts.len(), 3);
        assert!(s.queued_direction.is_none());
        assert!(s.directing());
        assert!(!s.can_switch_session());
        let before = request.session.clone();
        let mut ops = vec![grow("hiring", "Hiring"), grow("cloud-costs", "Cloud costs")];
        let home = |id| if id == 2 { "hiring" } else { "cloud-costs" };
        ops.extend(before.thoughts.iter().map(|t| file(t.id, home(t.id))));
        let note = "Separated hiring from infrastructure costs.";
        answer.send(directed(&request, ops, note)).unwrap();
        poll_live(&mut s, 4.0, &mut Tx::default());
        assert!(requests.try_recv().is_err());
        assert!(!s.directing());
        assert!(s.direction_draft.text.is_empty());
        assert_eq!(s.direction_status, note);
        assert_eq!(s.projection.session.categories.len(), 2);
        assert_eq!(s.projection.session.directions, [instruction]);
        assert!(
            s.projection
                .session
                .thoughts
                .iter()
                .all(|t| t.category.is_some() && !t.pinned && t.text != instruction)
        );
        s.undo();
        assert_eq!(s.projection.session, before);
    }
    #[test]
    fn undo_cancels_queued_guidance_and_keeps_its_draft() {
        let mut s = State::empty();
        s.pending_recordings.insert(1);
        s.direction_draft = TextEdit::new("Refine these categories");
        let (_placing, _stewarding, mut requests, _answer) = lanes(&mut s);
        s.submit_direction();
        assert!(s.queued_direction.is_some());
        s.undo();
        assert!(s.queued_direction.is_none());
        // The capture it waited for finishes: nothing is left to send.
        s.pending_recordings.clear();
        poll_live(&mut s, 1.0, &mut Tx::default());
        assert_eq!(s.direction_draft.text, "Refine these categories");
        assert!(requests.try_recv().is_err());
        assert!(!s.directing());
    }
    #[test]
    fn undo_takes_back_guidance_in_flight_and_a_late_answer_changes_nothing() {
        let mut s = State::empty();
        s.draft = TextEdit::new("Hosting costs doubled");
        s.capture();
        let (_placing, _stewarding, mut requests, answer) = lanes(&mut s);
        let words = "Start a branch for costs";
        let first = instruct(&mut s, &mut requests, words, 1.0);
        assert!(s.directing() && !s.can_switch_session());
        let before = s.projection.session.clone();
        s.undo();
        // The instruction is what undo took back, not the thought before it.
        assert!(!s.directing());
        assert_eq!(s.projection.session, before);
        assert_eq!(s.direction_draft.text, words);
        let ops = || vec![grow("costs", "Costs"), file(1, "costs")];
        answer.send(directed(&first, ops(), "Done")).unwrap();
        poll_live(&mut s, 2.0, &mut Tx::default());
        assert_eq!(s.projection.session, before);
        // Asked again, it is a new turn: the first answer still counts for nothing.
        let second = instruct(&mut s, &mut requests, words, 3.0);
        answer.send(directed(&first, ops(), "Done")).unwrap();
        poll_live(&mut s, 4.0, &mut Tx::default());
        assert!(s.directing());
        assert_eq!(s.projection.session, before);
        answer.send(directed(&second, ops(), "")).unwrap();
        poll_live(&mut s, 5.0, &mut Tx::default());
        assert!(!s.directing());
        assert_eq!(s.projection.session.categories.len(), 1);
        assert_eq!(s.direction_status, "Guidance applied to the map");
        assert!(s.direction_draft.text.is_empty());
    }
    #[test]
    fn guidance_parks_holds_and_brings_thoughts_back_on_the_users_word() {
        use tacit::agent::{Op, Tier};
        use tacit::model::ParkKind;
        let mut s = State::empty();
        for words in [
            "Signup asks for fourteen fields",
            "Pricing tiers confuse buyers",
            "The coffee machine is broken",
        ] {
            s.draft = TextEdit::new(words);
            s.capture();
        }
        let (mut placing, _stewarding, mut requests, answer) = lanes(&mut s);
        let park = |thought, kind, topic: &str| Op::Park {
            thought,
            kind,
            topic: topic.into(),
            reason: "Not what this session is about".into(),
            concept: Some(format!("Concept {thought}")),
        };
        let parked = |s: &State, id| {
            let lot = s.projection.session.parked.iter();
            lot.filter(|p| p.thought.id == id)
                .map(|p| (p.kind, p.held))
                .next()
        };
        // The agent files one thought and sets two aside on its own judgement.
        poll_live(&mut s, 1.0, &mut Tx::default());
        let asked = placing.try_recv().unwrap();
        let ops = vec![
            grow("signup", "Signup"),
            file(1, "signup"),
            park(2, ParkKind::Later, "Pricing"),
            park(3, ParkKind::OffTopic, "Office"),
        ];
        answer
            .send(turned(asked.epoch, Tier::Place, ops, ""))
            .unwrap();
        poll_live(&mut s, 2.0, &mut Tx::default());
        assert_eq!(parked(&s, 2), Some((ParkKind::Later, false)));
        assert_eq!(parked(&s, 3), Some((ParkKind::OffTopic, false)));
        let agents = s.projection.session.clone();

        let words = "Bring pricing back, keep the coffee machine out, and leave signup for later";
        let request = instruct(&mut s, &mut requests, words, 3.0);
        assert!(request.holds.is_empty());
        let ops = vec![
            grow("pricing", "Pricing"),
            file(2, "pricing"),
            park(3, ParkKind::OffTopic, "Office"),
            park(1, ParkKind::Later, "Signup"),
        ];
        answer
            .send(directed(&request, ops, "Pricing is back on the map"))
            .unwrap();
        poll_live(&mut s, 4.0, &mut Tx::default());
        let session = &s.projection.session;
        let back = session.thoughts.iter().find(|t| t.id == 2).unwrap();
        assert!(back.kept && back.category.as_deref() == Some("pricing"));
        // What the user decided is held against the agent's own later turns.
        assert_eq!(parked(&s, 3), Some((ParkKind::OffTopic, true)));
        assert_eq!(parked(&s, 1), Some((ParkKind::Later, true)));
        assert_eq!(s.direction_status, "Pricing is back on the map");
        assert_eq!(s.organizer_status, "✦ Map follows your guidance");
        // One step back is the parking lot as the agent left it.
        s.undo();
        assert_eq!(s.projection.session, agents);
    }
    #[test]
    fn final_transcript_only_adds_missing_live_windows() {
        let mut s = State::empty();
        s.active_recording = Some("live".into());
        s.projection.audio.push(tacit::timeline::Audio {
            id: "live".into(),
            asset: "audio.wav".into(),
            start_ms: 0,
            duration_ms: 2000,
            sample_rate: 16000,
            samples: 32000,
        });
        let first = tacit::timeline::Cue {
            start_ms: 0,
            end_ms: 1000,
            text: "Plant gardens.".into(),
        };
        let second = tacit::timeline::Cue {
            start_ms: 1000,
            end_ms: 2000,
            text: "Fund libraries.".into(),
        };
        s.receive_speech_segment("live", 0, first.clone());
        let finish = || VoiceEvent::TimedTranscript {
            id: "live".into(),
            text: "Plant gardens. Fund libraries.".into(),
            cues: vec![first.clone(), second.clone()],
        };
        s.receive_voice(finish(), 2.0);
        assert_eq!(s.projection.session.thoughts.len(), 2);
        assert_eq!(s.projection.transcripts.len(), 1);
        assert_eq!(s.projection.speech_segments["live"].len(), 2);
        s.receive_voice(finish(), 2.1);
        assert_eq!(s.projection.session.thoughts.len(), 2);
        assert_eq!(s.projection.transcripts.len(), 1);
    }

    #[test]
    fn recorded_direction_intent_survives_mode_changes_and_is_not_transcript_linked() {
        let mut s = State::empty();
        s.projection.audio.push(tacit::timeline::Audio {
            id: "instruction".into(),
            asset: "audio.wav".into(),
            start_ms: 0,
            duration_ms: 1000,
            sample_rate: 16000,
            samples: 16000,
        });
        s.projection
            .apply(&tacit::timeline::Event::RecordingDirected {
                id: "instruction".into(),
            })
            .unwrap();
        s.direction_mode = false;
        s.receive_voice(
            VoiceEvent::TimedTranscript {
                id: "instruction".into(),
                text: "Rename the map".into(),
                cues: vec![tacit::timeline::Cue {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "Rename the map".into(),
                }],
            },
            1.0,
        );
        assert!(s.projection.session.thoughts.is_empty());
        assert!(s.projection.transcript_links.is_empty());
        assert_eq!(s.projection.transcripts.len(), 1);
        assert!(s.direction_mode);
        assert_eq!(s.direction_draft.text, "Rename the map");
    }
    #[test]
    fn guidance_is_journaled_replayable_undoable_and_lands_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = State::open_session(dir.path().join("session.json")).unwrap();
        s.draft = TextEdit::new("A real idea");
        s.capture();
        let (_placing, _stewarding, mut requests, answer) = lanes(&mut s);
        let instruction = "Call this map Research";
        let request = instruct(&mut s, &mut requests, instruction, 1.0);
        let before = s.projection.session.clone();
        let response = || {
            let title = tacit::agent::Op::Title {
                title: "Research".into(),
            };
            directed(&request, vec![title], "Renamed the map to Research")
        };
        answer.send(response()).unwrap();
        poll_live(&mut s, 2.0, &mut Tx::default());
        assert_eq!(s.projection.session.title, "Research");
        assert_eq!(s.projection.session.thoughts, before.thoughts);
        assert_eq!(s.projection.session.directions, [instruction]);
        assert_eq!(s.direction_status, "Renamed the map to Research");
        let j = s.journal.as_ref().unwrap();
        assert_eq!(j.at(j.duration()).unwrap().session, s.projection.session);
        let changed = s.projection.session.clone();
        // The same answer again: nobody is waiting for it any more.
        answer.send(response()).unwrap();
        poll_live(&mut s, 3.0, &mut Tx::default());
        assert_eq!(s.projection.session, changed);
        s.undo();
        assert_eq!(s.projection.session, before);
    }
    #[test]
    fn switching_sessions_preserves_drafts_and_keeps_histories_separate() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("session.json");
        let other = tacit::sessions::create(&base, "Second").unwrap();
        let mut state = State::open_session(base.clone()).unwrap();
        state.library_base = Some(base.clone());
        state.draft = TextEdit::new("First thought");
        state.capture();
        state.draft = TextEdit::new("Unfinished draft");
        state.switch_session(other.path.clone());
        assert_eq!(state.path.as_ref(), Some(&other.path));
        assert!(state.projection.session.thoughts.is_empty());
        assert!(state.history.is_empty());
        state.draft = TextEdit::new("Second thought");
        state.capture();
        state.switch_session(base.clone());
        assert_eq!(state.draft.text, "Unfinished draft");
        assert_eq!(state.projection.session.thoughts.len(), 1);
        assert_eq!(state.projection.session.thoughts[0].text, "First thought");
        state.voice_state = VoiceState::Recording;
        state.switch_session(other.path.clone());
        assert_eq!(state.path.as_ref(), Some(&base));
        state.voice_state = VoiceState::Ready;
        let _lock = storage::lock(&other.path).unwrap();
        state.switch_session(other.path);
        assert_eq!(state.path.as_ref(), Some(&base));
        assert_eq!(state.draft.text, "Unfinished draft");
    }

    #[test]
    fn new_session_does_not_inherit_the_rename_field_and_gets_a_topic() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("session.json");
        let mut rt = runtime((820.0, 640.0));
        rt.state = State::open_session(base.clone()).unwrap();
        rt.state.library_base = Some(base.clone());
        rt.state.refresh_sessions();
        rt.state.sessions_open = true;
        rt.state.session_name_edit = TextEdit::new("The previous conversation");
        rt.invalidate();
        click(&mut rt, "session-create");
        assert_ne!(rt.state.path.as_ref(), Some(&base));
        assert_eq!(rt.state.session_name, "Conversation 1");
        assert!(rt.state.projection.session.thoughts.is_empty());
        rt.state.draft = TextEdit::new("How should we improve onboarding?");
        rt.state.capture();
        assert_eq!(rt.state.session_name, "How should we improve onboarding?");
        let first = rt.state.path.clone();
        click(&mut rt, "sessions");
        click(&mut rt, "session-create");
        assert_ne!(rt.state.path, first);
        assert!(rt.state.session_name.starts_with("Conversation "));
        rt.state.switch_session(first.unwrap());
        assert_eq!(rt.state.session_name, "How should we improve onboarding?");
    }
    #[test]
    fn failed_journal_write_never_publishes_a_live_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let journal = tacit::timeline::Journal::open(&path, Session::default()).unwrap();
        drop(journal);
        let mut state = State::empty();
        // The OS rejects writes to this read-only file, exercising the real commit boundary.
        state.journal = Some(tacit::timeline::Journal::read(&path).unwrap());
        state.draft = TextEdit::new("Keep this draft if saving fails");
        let before = state.projection.clone();
        state.capture();
        assert_eq!(state.projection, before);
        assert_eq!(state.draft.text, "Keep this draft if saving fails");
        assert!(state.read_only);
        assert_eq!(
            tacit::timeline::Journal::read(&path).unwrap().projection,
            before
        );
    }
    #[test]
    fn live_capture_and_correction_render_the_committed_projection() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = State::empty();
        state.journal = Some(
            tacit::timeline::Journal::open(&dir.path().join("session.json"), Session::default())
                .unwrap(),
        );
        state.draft = TextEdit::new("An idea");
        state.capture();
        state.move_selected();
        let journal = state.journal.as_ref().unwrap();
        assert_eq!(state.projection, journal.projection);
        assert_eq!(journal.at(journal.duration()).unwrap(), state.projection);
        assert!(journal.entries.iter().any(|e| matches!(
            &e.event,
            tacit::timeline::Event::SessionChanged {
                change: SessionEvent::Captured { .. }
            }
        )));
        assert!(
            !journal
                .entries
                .iter()
                .any(|e| matches!(&e.event, tacit::timeline::Event::Scene { .. }))
        );
    }
    #[test]
    fn regenerated_art_with_the_same_prompt_retains_each_actual_image() {
        use base64::Engine as _;
        for key in ["theme-one", generated_art::CENTRAL_KEY] {
            let dir = tempfile::tempdir().unwrap();
            let mut state = State::empty();
            state.journal = Some(
                tacit::timeline::Journal::open(
                    &dir.path().join("session.json"),
                    Session::default(),
                )
                .unwrap(),
            );
            let mut assets = Vec::new();
            for encoded in [
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==",
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYPj/HwADAgH/5ncLrgAAAABJRU5ErkJggg==",
            ] {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .unwrap();
                let image = state.image_cache.load(bytes.clone());
                image.wait().unwrap();
                state.gallery.layers = vec![generated_art::Layer {
                    mural: (key == generated_art::CENTRAL_KEY).then(mural_fixture),
                    key: key.into(),
                    fingerprint: "same-prompt".into(),
                    slot: 0,
                    image,
                    encoded: Some(std::sync::Arc::new(bytes.clone())),
                    born: 0.0,
                    retired: None,
                }];
                state.record_presentation("art arrived");
                assert!(!state.read_only);
                let j = state.journal.as_ref().unwrap();
                assert_eq!(j.projection.scene().artwork[0].key, key);
                assert_eq!(
                    j.projection.artwork.last().unwrap().scope
                        == tacit::timeline::ArtworkScope::Map,
                    key == generated_art::CENTRAL_KEY
                );
                let asset = j.projection.scene().artwork[0].asset.clone();
                assert_eq!(std::fs::read(j.asset(&asset).unwrap()).unwrap(), bytes);
                assets.push(asset);
            }
            let saved = state.journal.as_ref().unwrap().projection.scene();
            state.gallery.layers.clear();
            state.restore_scene(saved, 10.0);
            if key == generated_art::CENTRAL_KEY {
                let layer = &state.gallery.layers[0];
                assert_eq!(layer.mural, Some(mural_fixture()));
                assert!(layer.encoded.is_some());
                let reopened =
                    tacit::timeline::Journal::read(&state.journal.as_ref().unwrap().root).unwrap();
                assert_eq!(
                    reopened.projection.scene().artwork[0].mural,
                    Some(mural_fixture())
                );
                assert_eq!(
                    reopened.projection.artwork.last().unwrap().mural,
                    Some(mural_fixture())
                );
            }
            assert_ne!(assets[0], assets[1]);
            state.journal.as_ref().unwrap().validate_assets().unwrap();
        }
    }
    #[test]
    fn replay_uses_saved_projection_and_cannot_edit_live_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = runtime((1280.0, 860.0));
        rt.state.journal = Some(
            tacit::timeline::Journal::open(
                &dir.path().join("session.json"),
                rt.state.projection.session.clone(),
            )
            .unwrap(),
        );
        rt.state.draft = TextEdit::new("A saved idea");
        rt.state.capture();
        rt.state.art_style = Style::Marker;
        rt.state.record_presentation("style");
        let expected = rt.state.projection.session.clone();
        rt.state.toggle_replay();
        let count = rt.state.journal.as_ref().unwrap().entries.len();
        rt.state.seek_replay(i64::MAX);
        rt.frame(0.0);
        assert_eq!(rt.state.projection.session, expected);
        assert_eq!(rt.state.art_style, Style::Marker);
        rt.state.undo();
        rt.state.draft = TextEdit::new("must not be saved");
        rt.state.capture();
        rt.frame(0.1);
        assert_eq!(rt.state.projection.session, expected);
        assert_eq!(rt.state.journal.as_ref().unwrap().entries.len(), count);
        rt.state.toggle_replay();
        assert!(!rt.state.read_only);
        assert_eq!(rt.state.projection.session, expected);
        assert_eq!(rt.state.art_style, Style::Ink);
    }
    fn video_runtime(dir: &std::path::Path, size: (f32, f32)) -> Runtime<State> {
        let mut rt = runtime(size);
        rt.state.session_name = "Make room for meaningful work".into();
        rt.state.journal = Some(
            tacit::timeline::Journal::open(
                &dir.join("session.json"),
                rt.state.projection.session.clone(),
            )
            .unwrap(),
        );
        rt.state
            .record_pixels((2, 2), 1.0, &[255, 0, 0, 255].repeat(4))
            .unwrap();
        let journal = rt.state.journal.as_mut().unwrap();
        let asset = journal
            .import_bytes(
                &tacit::video_archive::png_bytes(2, 2, &[0, 0, 255, 255].repeat(4)).unwrap(),
                "png",
            )
            .unwrap();
        let frame = tacit::timeline::PixelFrame {
            asset,
            width: 2,
            height: 2,
            scale: 1.0,
            content_hash: tacit::timeline::content_hash(&journal.projection.session).unwrap(),
        };
        journal
            .present_pixels(journal.now_ms() + 20_000, journal.projection.scene(), frame)
            .unwrap();
        rt.state.projection = journal.projection.clone();
        rt.invalidate();
        rt.frame(1.0);
        rt
    }
    #[test]
    fn video_actions_play_original_frames_and_restore_live_at_both_sizes() {
        for size in [(820.0, 640.0), (1280.0, 860.0)] {
            let dir = tempfile::tempdir().unwrap();
            let mut rt = video_runtime(dir.path(), size);
            let expected = rt.state.projection.session.clone();
            for key in ["replay", "export-video"] {
                let rect = rt
                    .elements()
                    .iter()
                    .find(|e| e.key.as_deref() == Some(key))
                    .map(|e| e.rect)
                    .unwrap();
                assert!(
                    rect[0] >= 0.0
                        && rect[1] >= 0.0
                        && rect[0] + rect[2] <= size.0 + 1.0
                        && rect[1] + rect[3] <= size.1 + 1.0,
                    "{key} must remain visible at {size:?}: {rect:?}"
                );
            }
            click(&mut rt, "replay");
            rt.frame(1.0); // Playback projection runs on the frame after the action.
            assert!(rt.state.pixel_replay);
            assert!(rt.state.replay.as_ref().unwrap().playing);
            let first_asset = rt.state.pixel_image.as_ref().unwrap().0.clone();
            assert!(!rt.state.capturing_pixels());
            click(&mut rt, "replay-play");
            assert!(!rt.state.replay.as_ref().unwrap().playing);
            click(&mut rt, "replay-forward");
            click(&mut rt, "replay-forward");
            rt.frame(1.0);
            assert_ne!(rt.state.pixel_image.as_ref().unwrap().0, first_asset);
            click(&mut rt, "replay-back");
            rt.frame(1.0);
            assert_eq!(rt.state.pixel_image.as_ref().unwrap().0, first_asset);
            click(&mut rt, "replay-renderer");
            assert!(!rt.state.pixel_replay);
            click(&mut rt, "replay-renderer");
            assert!(rt.state.pixel_replay);
            click(&mut rt, "back-live");
            assert!(rt.state.replay.is_none());
            assert!(rt.state.capturing_pixels());
            assert_eq!(rt.state.projection.session, expected);
        }
    }
    #[test]
    fn save_video_dialog_can_cancel_and_reject_existing_files_without_stopping_capture() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = video_runtime(dir.path(), (820.0, 640.0));
        click(&mut rt, "export-video");
        let (kind, options) = rt.file_dialog().unwrap();
        assert_eq!(kind, rune::FileDialogKind::Save);
        assert!(
            options
                .file_name
                .as_ref()
                .unwrap()
                .starts_with("Make room for meaningful work-")
        );
        assert!(!rt.state.can_switch_session());
        rt.resolve_file_dialog(None).unwrap();
        rt.frame(1.0);
        assert!(rt.state.capturing_pixels());
        assert!(rt.state.replay.is_none());
        assert!(!rt.state.video_busy());
        let existing = dir.path().join("keep.mkv");
        std::fs::write(&existing, b"keep this file").unwrap();
        click(&mut rt, "export-video");
        rt.resolve_file_dialog(Some(existing.clone())).unwrap();
        rt.frame(1.0);
        assert_eq!(std::fs::read(existing).unwrap(), b"keep this file");
        assert!(rt.state.status.contains("already exists"));
        assert!(rt.state.capturing_pixels());
        assert!(!rt.state.video_busy());
    }
    #[test]
    fn save_video_from_live_exports_to_chosen_path_and_can_return_to_voice() {
        if std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .is_err()
        {
            eprintln!("ffmpeg unavailable; UI codec integration test skipped");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut rt = video_runtime(dir.path(), (820.0, 640.0));
        click(&mut rt, "export-video");
        let destination = dir.path().join("my session video");
        rt.resolve_file_dialog(Some(destination.clone())).unwrap();
        rt.frame(1.0);
        assert!(rt.state.replay.is_some());
        assert!(!rt.state.capturing_pixels());
        if rt.state.video_export.is_some() {
            rt.state.toggle_replay();
            assert!(
                rt.state.replay.is_some(),
                "capture must stay closed during export"
            );
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while rt.state.video_export.is_some() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
            rt.frame(1.0);
        }
        assert!(!rt.state.video_busy(), "export timed out");
        let output = destination.with_extension("mkv");
        assert_eq!(
            rt.state.saved_video.as_ref(),
            Some(&output),
            "{}",
            rt.state.status
        );
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
        assert!(rt.find("show-video").is_some());
        let probe = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v",
                "-show_entries",
                "stream=codec_name,width,height",
                "-of",
                "csv=p=0",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert!(probe.status.success());
        assert_eq!(String::from_utf8(probe.stdout).unwrap().trim(), "ffv1,2,2");
        click(&mut rt, "back-live");
        assert!(rt.state.capturing_pixels());
        assert!(rt.find("replay").is_some());
        assert!(rt.find("export-video").is_some());
    }
    #[test]
    fn video_controls_respect_voice_work_and_scene_only_archives() {
        let dir = tempfile::tempdir().unwrap();
        let mut rt = video_runtime(dir.path(), (820.0, 640.0));
        for voice in [
            VoiceState::Starting,
            VoiceState::Recording,
            VoiceState::Finishing,
            VoiceState::Cancelling,
        ] {
            rt.state.voice_state = voice;
            rt.invalidate();
            click(&mut rt, "replay");
            click(&mut rt, "export-video");
            assert!(rt.state.replay.is_none());
            assert!(rt.file_dialog().is_none());
        }
        rt.state.voice_state = VoiceState::Ready;
        rt.state.journal = Some(
            tacit::timeline::Journal::open(
                &dir.path().join("older-session.json"),
                rt.state.projection.session.clone(),
            )
            .unwrap(),
        );
        rt.invalidate();
        click(&mut rt, "export-video");
        assert!(rt.file_dialog().is_none());
        click(&mut rt, "replay");
        assert!(rt.state.replay.is_some());
        assert!(!rt.state.pixel_replay);
        assert!(rt.find("replay-renderer").is_none());
        // Read-only archives retain export controls whenever original frames exist.
        let mut rt = video_runtime(&dir.path().join("read-only"), (820.0, 640.0));
        rt.state.toggle_replay();
        rt.state.archive_only = true;
        rt.invalidate();
        click(&mut rt, "export-video");
        assert!(rt.file_dialog().is_some());
        assert!(rt.find("back-live").is_none());
        rt.resolve_file_dialog(None).unwrap();
        rt.frame(1.0);
    }
    #[test]
    fn the_session_video_keeps_the_frame_of_the_scene_it_stands_in_for() {
        let place = |rt: &Runtime<State>, key: &str| {
            rt.elements()
                .iter()
                .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
                .map(|e| e.target)
                .unwrap_or_else(|| panic!("missing {key}"))
        };
        for size in [(820.0, 640.0), (1280.0, 860.0)] {
            let dir = tempfile::tempdir().unwrap();
            let mut rt = video_runtime(dir.path(), size);
            click(&mut rt, "replay");
            rt.frame(1.0);
            click(&mut rt, "replay-play");
            rt.frame(1.0);
            assert!(rt.state.pixel_replay);
            assert!(!rt.state.replay.as_ref().unwrap().playing);
            // Trading the video for the scene moves neither the controls nor the status.
            let chrome = ["replay-play", "status-bar"];
            let video = chrome.map(|key| place(&rt, key));
            click(&mut rt, "replay-renderer");
            rt.frame(1.0);
            assert!(!rt.state.pixel_replay);
            assert_eq!(chrome.map(|key| place(&rt, key)), video, "{size:?}");
            click(&mut rt, "replay-renderer");
            rt.frame(1.0);
            // The sessions arrive over the video the way they do over the map.
            rt.state.sessions_open = true;
            rt.invalidate();
            rt.frame(1.0);
            rt.frame(1.0);
            let drawer = place(&rt, "session-drawer");
            assert!(
                drawer[0] >= 0.0 && drawer[0] + drawer[2] <= size.0,
                "{drawer:?}"
            );
            assert_eq!(rt.find("sessions-scrim").is_some(), size.0 < 1080.0);
            click(&mut rt, "sessions-back");
            assert!(!rt.state.sessions_open);
            rt.frame(1.0);
            // A stretch with no captured pixels says so from the middle of the stage.
            rt.state.pixel_image = None;
            rt.invalidate();
            rt.frame(1.0);
            let gap = place(&rt, "video-gap");
            let words = rt
                .elements()
                .iter()
                .find(|e| {
                    e.text.as_deref() == Some("No original pixels were captured at this time.")
                })
                .map(|e| e.target)
                .unwrap();
            let middle = |r: [f32; 4]| [r[0] + r[2] / 2.0, r[1] + r[3] / 2.0];
            assert!(
                (middle(words)[0] - middle(gap)[0]).abs() < 1.0,
                "{words:?} {gap:?}"
            );
            assert!(
                words[1] > gap[1] && words[1] + words[3] < gap[1] + gap[3],
                "{words:?} {gap:?}"
            );
            assert!(gap[1] + gap[3] <= place(&rt, "replay-play")[1], "{gap:?}");
        }
    }
    #[test]
    fn a_long_thought_grows_the_composer_and_keeps_send_within_reach() {
        let mut rt = runtime((820.0, 640.0));
        click(&mut rt, "thought-input");
        let rect = |rt: &Runtime<State>, key: &str| {
            rt.elements()
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .map(|e| e.rect)
                .unwrap_or_else(|| panic!("missing {key}"))
        };
        let line = rect(&rt, "thought-lines")[3];
        rt.key(KeyEvent {
            key: Key::Text("What if every recurring meeting had to earn its place? ".repeat(12)),
            mods: Mods::default(),
        });
        rt.frame(1.0);
        rt.frame(1.0);
        let lines = rect(&rt, "thought-lines");
        assert!(
            lines[3] > line * 3.5 && lines[3] <= line * 4.0 + 0.5,
            "a long thought shows four lines, then scrolls: {line} -> {}",
            lines[3]
        );
        let map = rect(&rt, "map");
        assert!(map[3] >= 200.0, "the map keeps its room: {map:?}");
        let send = rect(&rt, "capture");
        assert!(send[1] + send[3] <= 640.0 && send[0] + send[2] <= 820.0);
        click(&mut rt, "capture");
        assert_eq!(rt.state.projection.session.thoughts.len(), 7);
        assert!(rt.state.draft.text.is_empty());
        rt.frame(1.0);
        assert!((rect(&rt, "thought-lines")[3] - line).abs() < 0.5);
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
        assert_eq!(rt.state.projection.session.thoughts.len(), 6);
        assert!(rt.find("draft-preview").is_some());
        rt.key(KeyEvent {
            key: Key::Enter,
            mods: Mods::default(),
        });
        rt.frame(1.0);
        assert_eq!(rt.state.projection.session.thoughts.len(), 7);
        assert_eq!(
            rt.state.projection.session.thoughts[6].role,
            Role::Possibility
        );
        assert!(rt.state.draft.text.is_empty());
        click(&mut rt, "undo");
        assert_eq!(rt.state.projection.session.thoughts.len(), 6);
    }
    #[test]
    fn a_starter_opens_the_first_thought_in_its_lens() {
        let open = |state: State, size| {
            let mut rt = Runtime::new(
                state,
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
        };
        for (role, _, stem) in workspace::STARTERS {
            assert_eq!(tacit::model::classify(stem), role, "{stem:?}");
        }
        for size in [(820.0, 640.0), (1280.0, 860.0)] {
            let mut rt = open(State::empty(), size);
            assert!(rt.find("empty-map").is_some());
            // Guidance has nothing to organize yet, so a starter returns to brainstorming.
            click(&mut rt, "mode-direct");
            assert!(rt.state.direction_mode);
            click(&mut rt, "starter-possibility");
            assert!(!rt.state.direction_mode);
            assert_eq!(rt.state.draft.text, "What if ");
            assert!(rt.find("empty-map").is_none());
            assert!(rt.find("draft-preview").is_some());
            // The composer holds the keyboard, so the thought continues without another click.
            rt.key(KeyEvent {
                key: Key::Text("we met outside?".into()),
                mods: Mods::default(),
            });
            rt.key(KeyEvent {
                key: Key::Enter,
                mods: Mods::default(),
            });
            rt.frame(1.0);
            let thoughts = &rt.state.projection.session.thoughts;
            assert_eq!(thoughts.len(), 1);
            assert_eq!(thoughts[0].text, "What if we met outside?");
            assert_eq!(thoughts[0].role, Role::Possibility);
        }
        let archive = State {
            read_only: true,
            ..State::empty()
        };
        let rt = open(archive, (820.0, 640.0));
        assert!(rt.find("empty-map").is_some());
        assert!(rt.find("starters").is_none());
    }
    #[test]
    fn selection_correction_delete_and_undo_work_at_minimum_size() {
        let mut rt = runtime((820.0, 640.0));
        click(&mut rt, "thought-1");
        click(&mut rt, "move");
        assert_eq!(rt.state.projection.session.thoughts[0].role, Role::Question);
        assert!(rt.state.projection.session.thoughts[0].pinned);
        click(&mut rt, "remove");
        assert_eq!(rt.state.projection.session.thoughts.len(), 5);
        click(&mut rt, "undo");
        assert_eq!(rt.state.projection.session.thoughts.len(), 6);
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
        let before = state.projection.session.clone();
        let proposal = tacit::model::Interpretation {
            categories: vec![],
            title: "Outdated title".into(),
            narrative: "Outdated interpretation".into(),
            assignments: before
                .thoughts
                .iter()
                .map(|t| tacit::model::Assignment {
                    concept: t.concept.clone(),
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
        assert_eq!(state.projection.session, before);
        response_tx
            .send(tacit::organizer::Response {
                revision: 2,
                result: Err("test timeout".into()),
            })
            .unwrap();
        assert!(poll(&mut state, 2.0, &mut Tx::default()));
        assert_eq!(state.projection.session, before);
        assert!(state.organizer_status.contains("test timeout"));
    }
    type Asked = tokio::sync::mpsc::UnboundedReceiver<tacit::agent::worker::Request>;
    type Answer = std::sync::mpsc::Sender<tacit::agent::worker::Response>;
    /// A map agent whose lanes the test reads and answers by hand: the placer's, the
    /// steward's, and the one for what the user asks of the map.
    fn lanes(s: &mut State) -> (Asked, Asked, Asked, Answer) {
        let (place, placing) = tokio::sync::mpsc::unbounded_channel();
        let (steward, stewarding) = tokio::sync::mpsc::unbounded_channel();
        let (direct, directing) = tokio::sync::mpsc::unbounded_channel();
        let (answer, receive) = std::sync::mpsc::channel();
        s.agent = Some(tacit::agent::Worker {
            place,
            steward,
            direct,
            receive,
        });
        (placing, stewarding, directing, answer)
    }
    /// The same for a test in which nobody gives the map an instruction.
    fn agent(s: &mut State) -> (Asked, Asked, Answer) {
        let (placing, stewarding, _, answer) = lanes(s);
        (placing, stewarding, answer)
    }
    fn turned(
        epoch: u64,
        tier: tacit::agent::Tier,
        ops: Vec<tacit::agent::Op>,
        note: &str,
    ) -> tacit::agent::worker::Response {
        let turn = tacit::agent::Turn {
            tier,
            ops,
            note: note.into(),
            escalate: None,
            remember: false,
            refused: vec![],
            unmet: 0,
            garbled: false,
            usage: Default::default(),
        };
        tacit::agent::worker::Response {
            epoch,
            tier,
            instruction: None,
            result: Ok(turn),
        }
    }
    /// The agent's answer to the instruction a request carried.
    fn directed(
        request: &tacit::agent::worker::Request,
        ops: Vec<tacit::agent::Op>,
        note: &str,
    ) -> tacit::agent::worker::Response {
        let mut response = turned(request.epoch, tacit::agent::Tier::Direct, ops, note);
        response.instruction = request.instruction.clone();
        response
    }
    /// Type an instruction and let a frame hand it to the agent.
    fn instruct(
        s: &mut State,
        asked: &mut Asked,
        words: &str,
        at: f64,
    ) -> tacit::agent::worker::Request {
        s.direction_draft = TextEdit::new(words);
        s.submit_direction();
        poll_live(s, at, &mut Tx::default());
        asked.try_recv().unwrap()
    }
    fn grow(id: &str, name: &str) -> tacit::agent::Op {
        tacit::agent::Op::Branch {
            id: id.into(),
            name: name.into(),
            parent: None,
        }
    }
    fn file(thought: u64, branch: &str) -> tacit::agent::Op {
        tacit::agent::Op::Place {
            thought,
            branch: branch.into(),
            concept: Some(format!("Concept {thought}")),
            role: None,
        }
    }
    #[test]
    fn the_agent_files_thoughts_at_once_parks_and_steps_back_in_the_pause() {
        use tacit::agent::{Op, Tier};
        let mut s = State::empty();
        let (mut placing, mut stewarding, answer) = agent(&mut s);
        let mut tx = Tx::default();
        poll(&mut s, 0.0, &mut tx);
        assert!(placing.try_recv().is_err());
        for words in [
            "Meetings eat the whole morning",
            "Nobody knows who decides",
            "My bike needs a new chain",
        ] {
            s.draft = TextEdit::new(words);
            s.capture();
        }
        poll(&mut s, 1.0, &mut tx);
        let request = placing.try_recv().expect("asked the moment thoughts wait");
        assert_eq!(request.session, s.projection.session);
        assert!(s.busy && stewarding.try_recv().is_err());
        let park = Op::Park {
            thought: 3,
            kind: tacit::model::ParkKind::Later,
            topic: "Commute".into(),
            reason: "Nothing on the map is about getting to work yet".into(),
            concept: Some("Bike repair".into()),
        };
        let ops = vec![
            grow("time", "Time"),
            file(1, "time"),
            grow("decisions", "Decisions"),
            file(2, "decisions"),
            park,
        ];
        answer
            .send(turned(request.epoch, Tier::Place, ops, ""))
            .unwrap();
        poll(&mut s, 2.0, &mut tx);
        let session = &s.projection.session;
        assert_eq!(session.thoughts[0].category.as_deref(), Some("time"));
        assert_eq!((session.thoughts.len(), session.parked.len()), (2, 1));
        assert_eq!(s.organizer_status, "Filed 2 thoughts · parked 1 thought");
        assert!(!s.busy);
        // Filing keeps up with the speaker; only the three captures are steps to undo.
        assert_eq!(s.history.len(), 3);

        // The steward waits for the talk to pause.
        poll(&mut s, 3.0, &mut tx);
        assert!(stewarding.try_recv().is_err());
        poll(&mut s, 4.1, &mut tx);
        let request = stewarding.try_recv().expect("asked once the talk pauses");
        assert!(s.busy && placing.try_recv().is_err());
        let ops = vec![
            grow("commute", "Commute"),
            file(3, "commute"),
            Op::Narrative {
                narrative: "Mornings go to meetings, and nobody owns decisions.".into(),
            },
        ];
        answer
            .send(turned(
                request.epoch,
                Tier::Steward,
                ops,
                "Brought the commute back",
            ))
            .unwrap();
        poll(&mut s, 5.0, &mut tx);
        assert!(s.projection.session.parked.is_empty());
        assert_eq!(s.projection.session.thoughts.len(), 3);
        assert_eq!(s.organizer_status, "✦ Brought the commute back");
        assert!(!s.busy);

        // A restructure is one step to undo, and the steward then leaves the map alone.
        s.undo();
        assert_eq!(s.projection.session.parked.len(), 1);
        assert!(tacit::agent::brief::narrative(&s.projection.session).is_empty());
        poll(&mut s, 30.0, &mut tx);
        assert!(placing.try_recv().is_err() && stewarding.try_recv().is_err());
    }
    #[test]
    fn turns_from_before_a_user_edit_are_dropped_and_failures_keep_the_map() {
        use tacit::agent::Tier;
        let mut s = State::demo();
        let (mut placing, _stewarding, answer) = agent(&mut s);
        let mut tx = Tx::default();
        poll(&mut s, 0.0, &mut tx);
        let request = placing.try_recv().unwrap();
        // The user takes the map into their own hands while the turn is in flight.
        s.selected = Some(1);
        s.move_selected();
        let before = s.projection.session.clone();
        let ops = vec![grow("time", "Time"), file(2, "time")];
        answer
            .send(turned(request.epoch, Tier::Place, ops, ""))
            .unwrap();
        poll(&mut s, 1.0, &mut tx);
        assert_eq!(s.projection.session, before);
        // The map as it now stands is asked about instead.
        let request = placing.try_recv().unwrap();
        assert_eq!(request.session, before);
        assert_ne!(request.epoch, 0);
        answer
            .send(tacit::agent::worker::Response {
                epoch: request.epoch,
                tier: Tier::Place,
                instruction: None,
                result: Err("test timeout".into()),
            })
            .unwrap();
        poll(&mut s, 2.0, &mut tx);
        assert_eq!(s.projection.session, before);
        assert!(s.organizer_status.contains("test timeout"));
        assert!(!s.busy);
        // A failure waits before it asks again.
        poll(&mut s, 2.5, &mut tx);
        assert!(placing.try_recv().is_err());
        poll(&mut s, 4.1, &mut tx);
        assert!(placing.try_recv().is_ok());
    }
    #[test]
    fn a_capture_still_being_split_waits_for_its_thoughts() {
        use tacit::agent::Tier;
        let mut s = State::empty();
        let (mut placing, _stewarding, answer) = agent(&mut s);
        let mut tx = Tx::default();
        let words = "Meetings eat the morning. Budgets are frozen.";
        s.receive_voice(VoiceEvent::Transcript(words.into()), 0.0);
        s.pending_recordings.insert(1);
        poll(&mut s, 0.0, &mut tx);
        assert!(placing.try_recv().is_err());
        assert!(!s.busy);
        s.receive_recording(tacit::recording::Response {
            id: 1,
            text: words.into(),
            result: Ok(vec![
                "Meetings eat the morning. ".into(),
                "Budgets are frozen.".into(),
            ]),
        });
        poll(&mut s, 1.0, &mut tx);
        let request = placing.try_recv().unwrap();
        assert_eq!(request.session.thoughts.len(), 2);
        assert!(request.holds.is_empty());
        // The speaker goes on while the turn is out: its edits land on the newer map.
        s.draft = TextEdit::new("Hiring is paused");
        s.capture();
        let ops = vec![grow("time", "Time"), file(1, "time"), file(2, "time")];
        answer
            .send(turned(request.epoch, Tier::Place, ops, ""))
            .unwrap();
        poll(&mut s, 2.0, &mut tx);
        let filed = |s: &State| {
            let thoughts = s.projection.session.thoughts.iter();
            thoughts.filter(|t| t.category.is_some()).count()
        };
        assert_eq!((filed(&s), s.projection.session.thoughts.len()), (2, 3));
        assert_eq!(placing.try_recv().unwrap().session.thoughts.len(), 3);
    }
    #[test]
    fn the_parking_lot_is_the_users_to_fill_keep_and_empty() {
        use tacit::model::ParkKind;
        let mut rt = runtime((1280.0, 1000.0));
        click(&mut rt, "thought-1");
        assert!(rt.find("revive").is_none() && rt.find("parking-lot").is_none());
        click(&mut rt, "park");
        let session = &rt.state.projection.session;
        assert_eq!((session.thoughts.len(), session.parked.len()), (5, 1));
        assert!(session.parked[0].held);
        // It stays in view beside the map, with its way back.
        assert_eq!(rt.state.selected, Some(1));
        for key in ["revive", "remove", "parking-lot", "parked-1"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        for key in ["move", "park", "hold"] {
            assert!(rt.find(key).is_none(), "{key}");
        }
        click(&mut rt, "show-transcript");
        click(&mut rt, "thought-2");
        click(&mut rt, "park");
        // The lot opens with the room to read all of it, so there is nothing left to expand.
        assert!(rt.find("transcript-expand").is_none());
        click(&mut rt, "parked-1");
        assert_eq!(rt.state.selected, Some(1));
        // The lot opens from the transcript on whatever was parked last.
        click(&mut rt, "show-transcript");
        click(&mut rt, "parking-lot");
        assert_eq!(rt.state.selected, Some(2));
        click(&mut rt, "revive");
        let session = &rt.state.projection.session;
        let back = session.thoughts.last().unwrap();
        assert_eq!((back.id, back.kept, &back.category), (2, true, &None));
        assert_eq!(session.parked.len(), 1);
        assert!(rt.find("park").is_some() && rt.find("revive").is_none());
        // Parking and bringing back are steps to undo like any other edit.
        click(&mut rt, "undo");
        assert_eq!(rt.state.projection.session.parked.len(), 2);
        click(&mut rt, "parking-lot");
        click(&mut rt, "remove");
        let session = &rt.state.projection.session;
        assert_eq!((session.thoughts.len(), session.parked.len()), (4, 1));

        // What the agent parked "not yet" can be kept off the map for good.
        let park = tacit::agent::Op::Park {
            thought: 3,
            kind: ParkKind::Later,
            topic: "Commute".into(),
            reason: "No branch is about getting to work yet".into(),
            concept: Some("Bike repair".into()),
        };
        let parked = SessionEvent::Mapped {
            ops: vec![park],
            pass: "place".into(),
            note: String::new(),
        };
        rt.state.dispatch_session(parked).unwrap();
        rt.state.selected = Some(3);
        rt.invalidate();
        click(&mut rt, "hold");
        let kept = rt.state.projection.session.parked.last().unwrap();
        assert_eq!((kept.kind, kept.held), (ParkKind::OffTopic, true));
        assert!(rt.find("hold").is_none() && rt.find("revive").is_some());
    }
    #[test]
    fn the_parking_lot_opens_with_every_kind_in_view() {
        use tacit::model::ParkKind;
        let scene = State::mapped_demo();
        let session = &scene.projection.session;
        let roots = session.categories.iter().filter(|c| c.parent.is_none());
        assert_eq!((session.thoughts.len(), roots.count()), (8, 4));
        assert!(session.thoughts.iter().all(|t| t.category.is_some()));
        let kinds: Vec<_> = session.parked.iter().map(|p| p.kind).collect();
        let off = ParkKind::OffTopic;
        assert_eq!(kinds, [off, off, ParkKind::Later, ParkKind::Aside]);
        // What was said twice is kept with the thought it repeats, not drawn twice.
        let repeat = session.retired.last().unwrap();
        assert_eq!((repeat.thought.id, &repeat.replaced_by[..]), (11, &[4][..]));
        assert!(session.narrative().contains("[2][4]"));

        let env = Env {
            system_dark: false,
            scale: 1.0,
        };
        let mut rt = Runtime::new(State::parked_demo(), view, (1280.0, 860.0), env);
        rt.poll = Some(poll);
        rt.frame(0.0);
        rt.frame(1.0);
        assert_eq!(rt.state.selected, Some(13));
        let elements = rt.elements();
        let found = |key: &str| {
            let found = elements.iter().find(|e| e.key.as_deref() == Some(key));
            found.unwrap_or_else(|| panic!("missing {key}"))
        };
        let place = found("selected-place").text.as_deref();
        assert_eq!(place, Some("Parked · Aside · Recording check"));
        // A subject is named beside a thought only where the thought does not say it already.
        let room = found("transcript-scroll").rect;
        for (key, title) in [
            ("parked-12", "Hybrid days"),
            ("parked-10", "Hosting costs › Audit unused servers"),
            ("parked-9", "Hosting costs doubled"),
            ("parked-13", "Recording check"),
        ] {
            let pill = found(key);
            assert_eq!(pill.label.as_deref(), Some(title));
            let (below, floor) = (pill.rect[1] + pill.rect[3], room[1] + room[3]);
            assert!(below <= floor, "{key} is out of view");
        }
        // The room is the lot's for as long as it is open, and the transcript's own after.
        assert!(rt.find("transcript-expand").is_none());
        click(&mut rt, "show-transcript");
        assert!(rt.find("transcript-expand").is_some());
        let closed = rt.elements();
        let closed = closed
            .iter()
            .find(|e| e.key.as_deref() == Some("transcript-scroll"));
        assert!(closed.unwrap().rect[3] < room[3]);
    }
    #[test]
    fn the_agent_honours_what_the_user_keeps_parked_and_what_they_bring_back() {
        use tacit::agent::{Op, Tier};
        use tacit::model::ParkKind;
        let mut s = State::empty();
        let (mut placing, mut stewarding, answer) = agent(&mut s);
        let mut tx = Tx::default();
        poll(&mut s, 0.0, &mut tx);
        for words in [
            "Meetings eat the whole morning",
            "My bike needs a new chain",
        ] {
            s.draft = TextEdit::new(words);
            s.capture();
        }
        poll(&mut s, 1.0, &mut tx);
        let request = placing.try_recv().unwrap();
        let park = || Op::Park {
            thought: 2,
            kind: ParkKind::Later,
            topic: "Commute".into(),
            reason: "Nothing on the map is about getting to work yet".into(),
            concept: Some("Bike repair".into()),
        };
        let ops = vec![grow("time", "Time"), file(1, "time"), park()];
        answer
            .send(turned(request.epoch, Tier::Place, ops, ""))
            .unwrap();
        poll(&mut s, 2.0, &mut tx);
        assert_eq!(s.projection.session.parked[0].kind, ParkKind::Later);

        // Kept parked: the steward may no longer bring it back.
        s.selected = Some(2);
        s.hold_selected();
        let kept = &s.projection.session.parked[0];
        assert_eq!((kept.kind, kept.held), (ParkKind::OffTopic, true));
        poll(&mut s, 12.0, &mut tx);
        let request = stewarding.try_recv().expect("asked in the lull");
        let ops = vec![
            grow("commute", "Commute"),
            file(2, "commute"),
            Op::Narrative {
                narrative: "Mornings go to meetings.".into(),
            },
        ];
        answer
            .send(turned(request.epoch, Tier::Steward, ops, ""))
            .unwrap();
        poll(&mut s, 13.0, &mut tx);
        let session = &s.projection.session;
        assert_eq!((session.thoughts.len(), session.parked.len()), (1, 1));
        assert_eq!(session.categories.len(), 1);
        assert_eq!(
            tacit::agent::brief::narrative(session),
            "Mornings go to meetings."
        );

        // Brought back: it waits for a branch, and the agent may not park it again.
        s.revive_selected();
        poll(&mut s, 14.0, &mut tx);
        let request = placing
            .try_recv()
            .expect("asked about the returned thought");
        assert!(request.session.thoughts.iter().any(|t| t.id == 2 && t.kept));
        answer
            .send(turned(request.epoch, Tier::Place, vec![park()], ""))
            .unwrap();
        poll(&mut s, 15.0, &mut tx);
        assert!(s.projection.session.parked.is_empty());
        poll(&mut s, 17.1, &mut tx);
        let request = placing.try_recv().expect("asked again after a breath");
        let ops = vec![grow("commute", "Commute"), file(2, "commute")];
        answer
            .send(turned(request.epoch, Tier::Place, ops, ""))
            .unwrap();
        poll(&mut s, 18.0, &mut tx);
        let back = s.projection.session.thoughts.last().unwrap();
        assert_eq!((back.id, back.category.as_deref()), (2, Some("commute")));
    }
    #[test]
    fn artwork_is_composed_only_when_someone_asks() {
        let mut rt = runtime((820.0, 640.0));
        let title = |rt: &Runtime<State>, key: &str| {
            rt.elements()
                .iter()
                .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
                .and_then(|e| e.label.clone())
        };
        // With no way to compose and nothing composed, there is no artwork to offer.
        for key in ["art-compose", "art-progress", "art-style"] {
            assert!(rt.find(key).is_none(), "{key}");
        }
        let (send, requests) = std::sync::mpsc::channel();
        let (responses, receive) = std::sync::mpsc::channel();
        rt.state.gallery.worker = Some(generated_art::Worker { send, receive });
        rt.invalidate();
        // Hiding the artwork withdraws whatever was asked, which the worker hears as well.
        let sent = || requests.try_iter().flatten().next();
        let failure = |request: &generated_art::Request| generated_art::Response {
            id: request.id,
            result: Err("Mural generation failed; check image model, credits and key".into()),
        };
        // A conversation composes nothing by itself, however long it is left.
        rt.frame(1.0);
        rt.frame(120.0);
        assert!(sent().is_none());
        assert_eq!(title(&rt, "art-compose").as_deref(), Some("Create artwork"));
        assert!(rt.find("art-style").is_none());
        click(&mut rt, "art-compose");
        assert!(rt.find("art-progress").is_some() && rt.find("art-compose").is_none());
        rt.frame(1.0);
        let request = sent().unwrap();
        assert!(request.base.is_none());
        // One ask is one composition, however long it takes.
        rt.frame(120.0);
        assert!(sent().is_none());
        assert!(rt.find("art-progress").is_some());
        let image = rt.state.image_cache.rgba(1, 1, vec![100, 160, 200, 255]);
        image.wait().unwrap();
        responses
            .send(generated_art::Response {
                id: request.id,
                result: Ok(generated_art::Update {
                    artifact: generated_art::Artifact {
                        fingerprint: "first-scene".into(),
                        encoded: Some(std::sync::Arc::new(vec![0])),
                        mural: tacit::artist::Mural {
                            identity: request.identity.clone(),
                            content: request.evidence.content.clone(),
                            render_policy: request.render_policy.clone(),
                            artist_model: request.artist_model.clone(),
                            image_model: request.model.clone(),
                            parent: None,
                            ..mural_fixture()
                        },
                    },
                    image: Some(image),
                }),
            })
            .unwrap();
        rt.frame(1.0);
        // The scene shows everything said so far, so all that is left is to put it away.
        for key in ["art-compose", "art-progress"] {
            assert!(rt.find(key).is_none(), "{key}");
        }
        assert_eq!(title(&rt, "art-style").as_deref(), Some("Hide artwork"));
        click(&mut rt, "thought-input");
        rt.key(KeyEvent {
            key: Key::Text("A greenhouse on the roof".into()),
            mods: Mods::default(),
        });
        rt.key(KeyEvent {
            key: Key::Enter,
            mods: Mods::default(),
        });
        rt.frame(1.0);
        rt.frame(120.0);
        // The conversation has moved past the mural, which waits to be asked again.
        assert!(sent().is_none());
        assert_eq!(title(&rt, "art-compose").as_deref(), Some("Update artwork"));
        // Artwork that is out of sight has nothing to bring up to date.
        click(&mut rt, "art-style");
        assert_eq!(title(&rt, "art-style").as_deref(), Some("Show artwork"));
        assert!(rt.find("art-compose").is_none());
        click(&mut rt, "art-style");
        click(&mut rt, "art-compose");
        rt.frame(1.0);
        let request = sent().unwrap();
        assert_eq!(request.base.as_ref().unwrap().fingerprint, "first-scene");
        // A failure keeps the mural, says what happened, and leaves the next move to a person.
        responses.send(failure(&request)).unwrap();
        rt.frame(1.0);
        rt.frame(120.0);
        assert!(sent().is_none());
        assert!(rt.find("art-notice").is_some() && rt.find("art-progress").is_none());
        assert_eq!(title(&rt, "art-compose").as_deref(), Some("Update artwork"));
        click(&mut rt, "retry-art");
        assert!(rt.find("art-notice").is_none() && rt.find("art-progress").is_some());
        rt.frame(1.0);
        responses.send(failure(&sent().unwrap())).unwrap();
        rt.frame(1.0);
        click(&mut rt, "art-dismiss");
        assert!(rt.find("art-notice").is_none());
        rt.frame(120.0);
        assert!(sent().is_none());
        assert_eq!(rt.state.gallery.layers.len(), 1);
        // A session that cannot be changed is not offered new artwork either.
        rt.state.read_only = true;
        rt.invalidate();
        rt.frame(1.0);
        assert!(rt.find("art-compose").is_none() && rt.find("art-style").is_some());
    }
    #[test]
    fn ink_art_toggles_without_changing_the_map() {
        let mut rt = runtime((820.0, 640.0));
        let image = rt.state.image_cache.rgba(1, 1, vec![100, 160, 200, 255]);
        image.wait().unwrap();
        rt.state.gallery.layers.push(generated_art::Layer {
            mural: Some(mural_fixture()),
            encoded: None,
            key: generated_art::CENTRAL_KEY.into(),
            fingerprint: "fixture".into(),
            slot: 0,
            image,
            born: -10.0,
            retired: None,
        });
        rt.invalidate();
        let before = rt.state.projection.session.clone();
        let ids = rt
            .state
            .art
            .layers
            .iter()
            .map(|l| l.serial)
            .collect::<Vec<_>>();
        assert_eq!(rt.state.art_style, Style::Ink);
        for expected in [Style::Off, Style::Ink] {
            click(&mut rt, "art-style");
            assert!(rt.state.art_style == expected);
            assert_eq!(rt.state.projection.session, before);
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
        state.projection.session.add("A later typed thought");
        state.receive_recording(tacit::recording::Response {
            id,
            text: "Meetings help. Budgets hurt.".into(),
            result: Ok(vec!["Meetings help. ".into(), "Budgets hurt.".into()]),
        });
        assert_eq!(
            state
                .projection
                .session
                .thoughts
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            vec!["Meetings help.", "Budgets hurt.", "A later typed thought"]
        );
        state.projection.session.validate().unwrap();
        state.undo();
        assert_eq!(
            state.projection.session.thoughts[0].text,
            "Meetings help. Budgets hurt."
        );
        state.pending_recordings.insert(id);
        state.undo();
        let before = state.projection.session.clone();
        state.receive_recording(tacit::recording::Response {
            id,
            text: "Meetings help. Budgets hurt.".into(),
            result: Ok(vec!["Meetings help. ".into(), "Budgets hurt.".into()]),
        });
        assert_eq!(state.projection.session, before);
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
        assert_eq!(
            state.projection.session.thoughts[0].text,
            "Full original words"
        );
        state.pending_recordings.insert(id);
        state.remove_selected();
        state.receive_recording(tacit::recording::Response {
            id,
            text: "Full original words".into(),
            result: Ok(vec!["Full original words".into()]),
        });
        assert!(state.projection.session.thoughts.is_empty());
    }
    #[test]
    fn voice_result_preserves_typed_draft_and_saves_recording() {
        let mut state = State::demo();
        state.draft = TextEdit::new("My typed thought.");
        let before = state.projection.session.clone();
        state.receive_voice(VoiceEvent::Transcript("My spoken thought.".into()), 0.0);
        assert_eq!(state.draft.text, "My typed thought.");
        assert_eq!(
            state.projection.session.thoughts.len(),
            before.thoughts.len() + 1
        );
        assert_eq!(state.voice_state, VoiceState::Ready);
        state.capture();
        assert_eq!(
            state.projection.session.thoughts.len(),
            before.thoughts.len() + 2
        );
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
        click(&mut rt, "mode-direct");
        assert!(!rt.state.direction_mode, "Recording locks the input mode");
        assert!(
            rt.elements()
                .iter()
                .filter(|e| { matches!(e.key.as_deref(), Some("mode-direct" | "mode-brainstorm")) })
                .all(|e| e.inert && !e.focusable)
        );
        // A storage failure must never take away the microphone's stop control.
        rt.state.read_only = true;
        rt.invalidate();
        rt.frame(0.0);
        assert!(rt.find("thought-input").is_none());
        click(&mut rt, "voice-stop");
        assert_eq!(commands.try_recv().unwrap(), VoiceCommand::Finish);
        rt.state.read_only = false;
        events
            .send(VoiceEvent::Transcript("A thought".into()))
            .unwrap();
        rt.frame(0.0);
        click(&mut rt, "voice-start");
        let _ = commands.try_recv();
        events.send(VoiceEvent::Recording).unwrap();
        rt.frame(0.0);
        rt.state.voice_command(VoiceCommand::Cancel);
        assert_eq!(commands.try_recv().unwrap(), VoiceCommand::Finish);
        rt.state.voice_state = VoiceState::Recording;
        rt.state.direction_mode = true;
        click(&mut rt, "voice-cancel");
        assert_eq!(commands.try_recv().unwrap(), VoiceCommand::Cancel);
        events.send(VoiceEvent::Cancelled).unwrap();
        assert_eq!(rt.state.draft.text, "");
        assert!(
            rt.state
                .projection
                .session
                .thoughts
                .iter()
                .any(|t| t.text == "A thought")
        );
    }
    #[test]
    fn modern_canvas_pans_both_axes_and_zooms_without_editing_content() {
        let mut rt = runtime((1280.0, 860.0));
        // Rune's debug stale-view audit deliberately commits some live-only events.
        rt.check_live_only(false);
        rt.frame(1.0);
        assert!(rt.state.map_viewport[0] > 800.0);
        assert!(rt.find("inspector").is_none());
        let before = rt.state.projection.session.clone();
        let pan = rt.state.camera_pan.get();
        let (x, y) = rt.find("map").unwrap();
        rt.wheel_event(rune::WheelEvent::new(90.0, -60.0).at(x, y).precise(true));
        rt.frame(1.1);
        assert_eq!(rt.state.camera_pan.get(), [pan[0] + 90.0, pan[1] - 60.0]);
        let zoom = rt.state.camera_zoom.get();
        let commits = rt.stats.commits;
        rt.magnify_event(rune::MagnifyEvent::new(0.25).at(x, y));
        rt.frame(1.2);
        assert!((rt.state.camera_zoom.get() - zoom * 1.25).abs() < 0.001);
        assert_eq!(
            rt.stats.commits, commits,
            "pinching must not rebuild the view"
        );
        rt.wheel_event(rune::WheelEvent::new(0.0, 10.0).at(x, y).mods(Mods {
            cmd: true,
            ..Mods::default()
        }));
        rt.frame(1.0 / 60.0);
        assert_eq!(
            rt.stats.commits, commits,
            "wheel zoom must not rebuild the view"
        );
        assert_eq!(rt.state.projection.session, before);
        click(&mut rt, "zoom-fit");
        assert!(rt.state.camera().valid());
        rt.check_live_only(true);
        rt.magnify_event(rune::MagnifyEvent::new(-0.1).at(x, y));
        rt.frame(1.0);
        assert!(rt.live_only_warnings().is_empty());
    }
    #[test]
    fn zoomed_concepts_remain_selectable_without_rebuilding_the_view() {
        let mut rt = runtime((1280.0, 860.0));
        rt.check_live_only(false);
        rt.frame(1.0);
        let (x, y) = rt.find("thought-1").unwrap();
        let commits = rt.stats.commits;
        rt.magnify_event(rune::MagnifyEvent::new(0.15).at(x, y));
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.stats.commits, commits);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.selected, Some(1));
    }
    #[test]
    fn the_zoom_control_says_how_large_the_map_is_drawn() {
        let mut rt = runtime((820.0, 640.0));
        rt.check_live_only(false);
        // A small window takes one more pass to settle: the map is first laid out for a larger one.
        rt.frame(1.0);
        rt.frame(1.0);
        let shown = |rt: &Runtime<State>| rt.state.zoom_readout.get().to_string();
        let drawn = |rt: &Runtime<State>| format!("{:.0}%", rt.state.camera_zoom.get() * 100.0);
        // Fitted into a small window, the demo is drawn smaller than life and says so.
        assert_eq!(shown(&rt), drawn(&rt));
        assert_ne!(shown(&rt), "100%");
        // A pinch does not rebuild the view, and the control keeps up with it all the same.
        let (fitted, commits) = (shown(&rt), rt.stats.commits);
        let (x, y) = rt.find("map").unwrap();
        rt.magnify_event(rune::MagnifyEvent::new(0.25).at(x, y));
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.stats.commits, commits);
        assert_eq!(shown(&rt), drawn(&rt));
        assert_ne!(shown(&rt), fitted);
        let pinched = shown(&rt);
        click(&mut rt, "zoom-out");
        assert_eq!(shown(&rt), drawn(&rt));
        assert_ne!(shown(&rt), pinched);
        // The reading is also the way back to actual size.
        click(&mut rt, "zoom-reset");
        assert_eq!(rt.state.camera_zoom.get(), 1.0);
        assert_eq!(shown(&rt), "100%");
        click(&mut rt, "zoom-fit");
        assert_eq!(shown(&rt), fitted);
    }
    #[test]
    fn the_map_keeps_every_idea_in_view_until_someone_chooses_a_view() {
        // The ideas that are not wholly inside the map.
        let cut_off = |rt: &Runtime<State>| {
            let elements = rt.elements();
            let map = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("map"))
                .unwrap()
                .rect;
            elements
                .iter()
                .filter(|e| {
                    !e.exiting
                        && e.key
                            .as_deref()
                            .and_then(|key| key.strip_prefix("thought-"))
                            .is_some_and(|id| id.parse::<u64>().is_ok())
                })
                .filter(|e| {
                    e.rect[0] < map[0] - 0.5
                        || e.rect[1] < map[1] - 0.5
                        || e.rect[0] + e.rect[2] > map[0] + map[2] + 0.5
                        || e.rect[1] + e.rect[3] > map[1] + map[3] + 0.5
                })
                .filter_map(|e| e.key.clone())
                .collect::<Vec<_>>()
        };
        let say = |rt: &mut Runtime<State>, words: &str| {
            click(rt, "thought-input");
            rt.key(KeyEvent {
                key: Key::Text(words.into()),
                mods: Mods::default(),
            });
            rt.key(KeyEvent {
                key: Key::Enter,
                mods: Mods::default(),
            });
            rt.frame(1.0);
            rt.frame(1.0);
        };
        for size in [(820.0, 640.0), (1280.0, 860.0)] {
            let mut rt = runtime(size);
            // Rune's debug stale-view audit deliberately commits some live-only events.
            rt.check_live_only(false);
            rt.frame(1.0);
            assert_eq!(cut_off(&rt), Vec::<String>::new());
            // Left alone, the camera pulls back as the map grows.
            for words in [
                "Could the standup become a shared note instead?",
                "Nobody remembers what last week's review decided",
                "Protect two mornings a week for deep work",
            ] {
                say(&mut rt, words);
                assert_eq!(cut_off(&rt), Vec::<String>::new(), "after {words:?}");
            }
            // A view someone chose stays where they put it, whatever arrives.
            let (x, y) = rt.find("map").unwrap();
            rt.wheel_event(rune::WheelEvent::new(-260.0, -140.0).at(x, y).precise(true));
            rt.frame(1.0);
            assert_ne!(cut_off(&rt), Vec::<String>::new());
            let held = rt.state.camera();
            say(&mut rt, "What would we lose without the weekly sync?");
            assert!(rt.state.camera() == held);
            // Fit map shows everything again, and goes back to following.
            click(&mut rt, "zoom-fit");
            assert_eq!(cut_off(&rt), Vec::<String>::new());
            say(&mut rt, "Try one meeting-free Friday as an experiment");
            assert_eq!(cut_off(&rt), Vec::<String>::new());
            assert!(rt.state.camera() != held);
        }
    }
    #[test]
    fn the_fitted_map_sits_mid_canvas_and_clear_of_the_zoom_control() {
        // A short, wide canvas has room beside the zoom control. The map must not lean into it.
        for size in [
            (820.0, 640.0),
            (1280.0, 700.0),
            (1280.0, 860.0),
            (1680.0, 1000.0),
        ] {
            let mut rt = runtime(size);
            rt.check_live_only(false);
            rt.frame(1.0);
            rt.frame(1.0);
            let elements = rt.elements();
            let rect = |key: &str| {
                elements
                    .iter()
                    .find(|e| e.key.as_deref() == Some(key))
                    .unwrap()
                    .rect
            };
            let (map, hub, zoom) = (rect("map"), rect("central-idea"), rect("map-navigation"));
            assert!(
                (hub[0] + hub[2] / 2.0 - map[0] - map[2] / 2.0).abs() < 1.0,
                "{size:?}: {hub:?} is off the middle of {map:?}"
            );
            for idea in elements.iter().filter(|e| {
                e.key.as_deref().is_some_and(|key| {
                    key == "central-idea"
                        || key
                            .strip_prefix("thought-")
                            .is_some_and(|id| id.parse::<u64>().is_ok())
                })
            }) {
                let [x, y, w, h] = idea.rect;
                assert!(
                    x + w <= zoom[0] || y + h <= zoom[1],
                    "{size:?}: {:?} runs under the zoom control",
                    idea.key
                );
            }
        }
    }
    #[test]
    fn displayed_pixels_and_presentation_commit_once_and_replay_together() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let mut state = State::empty();
        state.journal = Some(tacit::timeline::Journal::open(&path, Session::default()).unwrap());
        state.journal.as_mut().unwrap().begin_pixels().unwrap();
        let before = state.journal.as_ref().unwrap().entries.len();
        state.draft = TextEdit::new("A draft recorded with its pixels");
        state.camera_zoom.set(1.25);
        poll(&mut state, 0.0, &mut Tx::default());
        assert_eq!(state.journal.as_ref().unwrap().entries.len(), before);
        let pixels = [255, 0, 0, 255];
        state.record_pixels((1, 1), 2.0, &pixels).unwrap();
        let journal = state.journal.as_ref().unwrap();
        assert_eq!(journal.entries.len(), before + 1);
        assert_eq!(state.projection, journal.projection);
        let expected = journal.projection.clone();
        assert_eq!(expected.replay_scene().unwrap().draft, state.draft.text);
        assert_eq!(
            expected
                .replay_scene()
                .unwrap()
                .workspace
                .unwrap()
                .camera
                .zoom,
            1.25
        );
        assert_eq!(expected.pixel_frame.as_ref().unwrap().scale, 2.0);
        let asset = expected.pixel_frame.as_ref().unwrap().asset.clone();
        // Reusing identical image bytes must still save a new presentation and timestamp.
        state.draft = TextEdit::new("A changed draft");
        state.art.now += 1.0;
        state.record_pixels((1, 1), 1.0, &pixels).unwrap();
        let journal = state.journal.as_ref().unwrap();
        assert_eq!(journal.entries.len(), before + 2);
        assert_eq!(
            journal.projection.pixel_frame.as_ref().unwrap().asset,
            asset
        );
        let loaded = tacit::timeline::Journal::read(&path).unwrap();
        assert_eq!(loaded.projection, state.projection);
        loaded.validate_replay().unwrap();
        assert_eq!(
            loaded.projection.replay_scene().unwrap().draft,
            "A changed draft"
        );
    }
    #[test]
    fn drawer_and_transcript_controls_work_at_both_window_sizes() {
        for size in [(820.0, 640.0), (1280.0, 860.0)] {
            let mut rt = runtime(size);
            rt.state.sessions_open = true;
            rt.frame(1.0);
            assert!(rt.find("session-drawer").is_some());
            assert!(rt.find("map").is_some());
            click(&mut rt, "sessions-back");
            assert!(!rt.state.sessions_open);
            click(&mut rt, "transcript-expand");
            assert!(rt.state.transcript_expanded);
            assert!(rt.find("transcript-follow").is_none());
            let (x, y) = rt.find("transcript-scroll").unwrap();
            rt.wheel_event(rune::WheelEvent::new(0.0, 80.0).at(x, y).precise(true));
            rt.frame(1.0);
            assert!(!rt.state.transcript_following);
            click(&mut rt, "transcript-follow");
            assert!(rt.state.transcript_following);
            assert!(rt.find("transcript-follow").is_none());
        }
    }

    #[test]
    fn sessions_float_over_a_narrow_window_and_sit_beside_a_wide_one() {
        let rect = |rt: &Runtime<State>, key: &str| {
            rt.elements()
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .map(|e| e.rect)
                .unwrap_or_else(|| panic!("missing {key}"))
        };
        let open = |rt: &mut Runtime<State>| {
            rt.state.sessions_open = true;
            rt.invalidate();
            rt.frame(1.0);
            rt.frame(1.0);
        };
        let mut rt = runtime((820.0, 640.0));
        rt.frame(1.0);
        let map = rect(&rt, "map");
        open(&mut rt);
        let drawer = rect(&rt, "session-drawer");
        assert!(
            drawer[0] >= 0.0 && drawer[0] + drawer[2] <= 820.0,
            "{drawer:?}"
        );
        assert!(
            drawer[1] >= 0.0 && drawer[1] + drawer[3] <= 640.0,
            "{drawer:?}"
        );
        let beneath = rect(&rt, "map");
        assert!(
            map.iter().zip(beneath).all(|(a, b)| (a - b).abs() < 0.5),
            "a floating drawer leaves the map alone: {map:?} -> {beneath:?}"
        );
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(1.0);
        assert!(!rt.state.sessions_open, "Escape puts the sessions away");
        open(&mut rt);
        let selected = rt.state.selected;
        let (x, y) = (drawer[0] + drawer[2] + 120.0, drawer[1] + drawer[3] / 2.0);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0);
        assert!(!rt.state.sessions_open, "so does a press beside them");
        assert_eq!(
            rt.state.selected, selected,
            "which reaches nothing underneath"
        );

        let mut rt = runtime((1280.0, 860.0));
        open(&mut rt);
        let (drawer, workspace) = (rect(&rt, "session-drawer"), rect(&rt, "workspace"));
        assert!(
            workspace[0] >= drawer[0] + drawer[2],
            "a wide window has room for both: {drawer:?} {workspace:?}"
        );
        assert!(rt.find("sessions-scrim").is_none());
    }

    #[test]
    fn a_new_session_name_is_offered_once_typed_and_kept_with_enter() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("session.json");
        let mut rt = runtime((820.0, 640.0));
        rt.state = State::open_session(base.clone()).unwrap();
        rt.state.library_base = Some(base);
        rt.state.refresh_sessions();
        rt.invalidate();
        click(&mut rt, "sessions");
        assert!(rt.state.sessions_open);
        assert!(
            rt.find("session-rename").is_none(),
            "the name it already has needs no confirming"
        );
        rt.state.session_name_edit = TextEdit::new("Quarterly retro");
        rt.invalidate();
        rt.frame(1.0);
        assert!(rt.find("session-rename").is_some());
        click(&mut rt, "session-name");
        rt.key(KeyEvent {
            key: Key::Enter,
            mods: Mods::default(),
        });
        rt.frame(1.0);
        assert_eq!(rt.state.session_name, "Quarterly retro");
        assert!(
            rt.state
                .session_entries
                .iter()
                .any(|e| e.name == "Quarterly retro")
        );
        assert!(
            rt.find("session-rename").is_none(),
            "the confirmation leaves once the name is kept"
        );
    }

    #[test]
    fn the_first_thought_opens_the_dock_without_moving_what_was_there() {
        let mut rt = Runtime::new(
            State::empty(),
            view,
            (1280.0, 860.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        rt.poll = Some(poll);
        rt.frame(0.0);
        rt.frame(1.0);
        let rect = |rt: &Runtime<State>, key: &str| {
            rt.elements()
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .map(|e| e.rect)
                .unwrap_or_else(|| panic!("missing {key}"))
        };
        let path = |rt: &Runtime<State>, key: &str| {
            rt.elements()
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .map(|e| e.path.clone())
                .unwrap_or_else(|| panic!("missing {key}"))
        };
        click(&mut rt, "thought-input");
        rt.key(KeyEvent {
            key: Key::Text("What if we met less?".into()),
            mods: Mods::default(),
        });
        rt.frame(1.0);
        rt.frame(1.0);
        assert!(rt.find("dock-detail").is_none());
        assert!(rt.find("undo").is_none());
        let (bar, bar_path) = (rect(&rt, "dock-bar"), path(&rt, "dock-bar"));
        let story = rect(&rt, "toggle-story");
        rt.key(KeyEvent {
            key: Key::Enter,
            mods: Mods::default(),
        });
        rt.frame(1.0);
        rt.frame(1.0);
        assert_eq!(rt.state.projection.session.thoughts.len(), 1);
        // The transcript arrives above the bar, which stays the element it was.
        assert!(rt.find("dock-detail").is_some());
        assert_eq!(path(&rt, "dock-bar"), bar_path);
        let after = rect(&rt, "dock-bar");
        assert!(
            bar.iter().zip(after).all(|(a, b)| (a - b).abs() < 0.5),
            "the composer bar keeps its place: {bar:?} -> {after:?}"
        );
        // Undo joins the header before the tools already there, so none of them move.
        let (undo, now) = (rect(&rt, "undo"), rect(&rt, "toggle-story"));
        assert!(undo[0] + undo[2] <= now[0], "{undo:?} {now:?}");
        assert!(
            story.iter().zip(now).all(|(a, b)| (a - b).abs() < 0.5),
            "{story:?} -> {now:?}"
        );
    }

    #[test]
    fn the_header_keeps_every_tool_whole_in_the_smallest_window() {
        const NAME: &str = "A conversation whose name is far too long to sit beside every tool";
        // The widths of the header's tools, left to right, which must follow one another inside
        // the window.
        let row = |rt: &Runtime<State>, keys: &[&str]| {
            let elements = rt.elements();
            let rect = |key: &str| {
                elements
                    .iter()
                    .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
                    .map(|e| e.rect)
                    .unwrap_or_else(|| panic!("missing {key}"))
            };
            let window = rect("workspace");
            let name = elements
                .iter()
                .find(|e| e.text.as_deref() == Some(NAME))
                .map(|e| e.rect)
                .unwrap();
            let sessions = rect("sessions");
            // The name gives way to the tools, and still says which conversation this is.
            assert!(name[0] >= sessions[0] + sessions[2] && name[2] >= 48.0);
            let mut edge = name[0] + name[2];
            keys.iter()
                .map(|key| {
                    let tool = rect(key);
                    assert!(tool[0] >= edge - 0.5, "{key} is over its neighbor");
                    edge = tool[0] + tool[2];
                    assert!(
                        edge <= window[0] + window[2] + 0.5,
                        "{key} leaves the window"
                    );
                    tool[2]
                })
                .collect::<Vec<_>>()
        };
        // Everything the header can hold at once: a thought the mural does not show and that can
        // be undone, artwork to hide, the other sessions, and a name too long to sit beside them.
        let widths = |size| {
            let mut rt = runtime(size);
            let (send, _requests) = std::sync::mpsc::channel();
            let (_responses, receive) = std::sync::mpsc::channel();
            rt.state.gallery.worker = Some(generated_art::Worker { send, receive });
            let image = rt.state.image_cache.rgba(1, 1, vec![100, 160, 200, 255]);
            image.wait().unwrap();
            rt.state.gallery.layers.push(generated_art::Layer {
                mural: Some(mural_fixture()),
                encoded: Some(std::sync::Arc::new(vec![0])),
                key: generated_art::CENTRAL_KEY.into(),
                fingerprint: "fixture".into(),
                slot: 0,
                image,
                born: -10.0,
                retired: None,
            });
            rt.state.library_base = Some(PathBuf::from("unused-library/session.json"));
            rt.state.session_name = NAME.into();
            rt.invalidate();
            click(&mut rt, "thought-input");
            rt.key(KeyEvent {
                key: Key::Text("One more idea".into()),
                mods: Mods::default(),
            });
            rt.key(KeyEvent {
                key: Key::Enter,
                mods: Mods::default(),
            });
            rt.frame(1.0);
            rt.frame(1.0);
            let mut all = row(&rt, &["art-compose", "art-style", "undo", "toggle-story"]);
            click(&mut rt, "art-compose");
            rt.frame(1.0);
            all.extend(row(
                &rt,
                &["art-progress", "art-style", "undo", "toggle-story"],
            ));
            all
        };
        // No tool is squeezed to make room in a small window.
        let (small, roomy) = (widths((820.0, 640.0)), widths((1280.0, 860.0)));
        assert!(
            small.iter().zip(&roomy).all(|(a, b)| (a - b).abs() < 0.5),
            "{small:?} {roomy:?}"
        );
    }

    #[test]
    fn unavailable_guidance_preserves_the_draft_and_explains_the_requirement() {
        let mut rt = runtime((820.0, 640.0));
        let before = rt.state.projection.session.clone();
        click(&mut rt, "mode-direct");
        click(&mut rt, "thought-input");
        rt.key(KeyEvent {
            key: Key::Text("Put decisions first".into()),
            mods: Mods::default(),
        });
        rt.frame(1.0);
        assert!(rt.find("voice-start").is_none());
        assert!(rt.find("voice-check").is_none());
        let elements = rt.elements();
        let submit = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("capture"))
            .unwrap();
        assert!(submit.inert && !submit.focusable);
        assert!(
            elements
                .iter()
                .any(|e| e.key.as_deref() == Some("voice-feedback")
                    && e.text.as_deref().is_some_and(|t| t.contains("OpenRouter")))
        );
        rt.key(KeyEvent {
            key: Key::Enter,
            mods: Mods::default(),
        });
        rt.frame(1.0);
        assert_eq!(rt.state.direction_draft.text, "Put decisions first");
        assert_eq!(rt.state.projection.session, before);
        assert!(rt.state.queued_direction.is_none());
        click(&mut rt, "mode-brainstorm");
        assert!(rt.state.draft.text.is_empty());
        assert_eq!(rt.state.direction_draft.text, "Put decisions first");
    }

    #[test]
    fn workspace_removes_inapplicable_actions_without_hiding_recovery() {
        let mut rt = runtime((820.0, 640.0));
        for key in [
            "capture",
            "save",
            "undo",
            "art-compose",
            "art-progress",
            "art-style",
            "art-notice",
            "retry-art",
            "art-dismiss",
            "voice-check",
        ] {
            assert!(
                rt.find(key).is_none(),
                "{key} has no action in the idle demo"
            );
        }
        click(&mut rt, "toggle-story");
        assert!(rt.find("map-navigation").is_none());
        assert!(rt.find("thought-input").is_some());
        click(&mut rt, "toggle-story");
        assert!(rt.find("zoom-fit").is_some());

        rt.state.path = Some(PathBuf::from("unused-recovery-path.json"));
        rt.state.status = "Not saved: disk full. Use Retry save.".into();
        rt.invalidate();
        rt.frame(1.0);
        assert!(rt.find("save").is_some());
        rt.state.read_only = true;
        rt.invalidate();
        rt.frame(1.0);
        assert!(rt.find("save").is_none());
        assert!(rt.find("thought-input").is_none());
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("session-status")
                    && e.text.as_deref().is_some_and(|t| t.contains("disk full")))
        );
    }
    #[test]
    fn live_voice_keeps_transcript_review_visible() {
        let mut rt = runtime((820.0, 640.0));
        rt.state.selected = None;
        rt.state.receive_speech_segment(
            "current",
            1,
            tacit::timeline::Cue {
                start_ms: 0,
                end_ms: 1000,
                text: "A fresh spoken idea".into(),
            },
        );
        rt.frame(1.0);
        assert_eq!(rt.state.selected, None);
        assert!(rt.find("live-segment-current-1").is_some());
        assert!(rt.find("selected-original").is_none());
    }

    #[test]
    #[ignore = "manual headless performance probe; no network or microphone"]
    fn performance_report() {
        use std::{hint::black_box, time::Instant};
        let mut state = State::themed_demo();
        for n in 0..154 {
            state.projection.session.add(&format!(
                "Idea {n}: {}",
                "Protect time for focused work and shared decisions. ".repeat(10)
            ));
        }
        let session = state.projection.session.clone();
        state.art = Garden::settled(&session);
        let mut rt = Runtime::new(
            state,
            view,
            (1280.0, 860.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        rt.frame(0.0);
        rt.frame(1.0);
        let (x, y) = rt.find("map").unwrap();
        let started = Instant::now();
        for n in 0..120 {
            rt.magnify_event(
                rune::MagnifyEvent::new(if n % 2 == 0 { 0.01 } else { -0.01 }).at(x, y),
            );
            rt.frame(1.0 / 60.0);
        }
        println!(
            "PERF zoom_ms_per_frame {:.3}",
            started.elapsed().as_secs_f64() * 1000.0 / 120.0
        );

        let (send, _requests) = std::sync::mpsc::channel();
        let (_responses, receive) = std::sync::mpsc::channel();
        let mut gallery = Gallery::default();
        gallery.worker = Some(generated_art::Worker { send, receive });
        gallery.update(&session, Style::Ink, false, 0.0, &mut Tx::default());
        let started = Instant::now();
        for _ in 0..1000 {
            black_box(gallery.update(&session, Style::Ink, false, 1.0, &mut Tx::default()));
        }
        println!(
            "PERF art_planning_ms_per_tick {:.3}",
            started.elapsed().as_secs_f64()
        );

        let dir = tempfile::tempdir().unwrap();
        rt.state.journal = Some(
            tacit::timeline::Journal::open(&dir.path().join("session.json"), session).unwrap(),
        );
        let mut pixels = [232, 227, 207, 255].repeat(1280 * 860);
        let started = Instant::now();
        for n in 0..40 {
            pixels[n * 4] = n as u8;
            poll(&mut rt.state, n as f64 / 60.0, &mut Tx::default());
            rt.state.record_pixels((1280, 860), 1.0, &pixels).unwrap();
        }
        println!(
            "PERF archive_ms_per_frame {:.3}",
            started.elapsed().as_secs_f64() * 1000.0 / 40.0
        );
        let journal = rt.state.journal.as_ref().unwrap();
        let at = journal.duration();
        let started = Instant::now();
        for _ in 0..100 {
            black_box(journal.replay_scene_at(at).unwrap());
        }
        println!(
            "PERF replay_ms_per_seek {:.3}",
            started.elapsed().as_secs_f64() * 1000.0 / 100.0
        );
    }
}
