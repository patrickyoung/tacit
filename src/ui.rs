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
    replay_audio: Option<(String, rune::Player)>,
    replay_images: std::collections::HashMap<String, rune::Image>,
    image_cache: rune::ImageCache,
    art_assets: std::collections::HashMap<rune::ImageId, String>,
    map_scroll: rune::Live<[f32; 2]>,
    story_scroll: rune::Live<[f32; 2]>,
    inspector_scroll: rune::Live<[f32; 2]>,
    viewport: [f32; 2],
    map_viewport: [f32; 2],
    reduced_motion: bool,
    active_recording: Option<String>,
    art: Garden,
    gallery: Gallery,
    art_style: Style,
    art_active: bool,
    pub draft: TextEdit,
    direction_mode: bool,
    direction_draft: TextEdit,
    director: Option<tacit::direction::Worker>,
    directing: bool,
    direction_status: String,
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
            || self.video_export.is_some()
            || self.directing
        {
            self.status =
                "Finish recording, transcription, or video export before switching sessions".into();
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
        match tacit::sessions::create(base, &self.session_name_edit.text) {
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
            last_pixels: None,
            replay_audio: None,
            replay_images: Default::default(),
            art_assets: Default::default(),
            image_cache: rune::ImageCache::new(std::sync::Arc::new(|| {})),
            map_scroll: rune::Live::new([0.0; 2]),
            story_scroll: rune::Live::new([0.0; 2]),
            inspector_scroll: rune::Live::new([0.0; 2]),
            viewport: [1280.0, 860.0],
            map_viewport: [0.0; 2],
            reduced_motion: false,
            active_recording: None,
            art: Garden::default(),
            gallery: Gallery::default(),
            art_style: Style::default(),
            art_active: false,
            draft: TextEdit::new(""),
            direction_mode: false,
            direction_draft: TextEdit::new(""),
            director: None,
            directing: false,
            direction_status: "Give instructions to reshape this map".into(),
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
        s.projection.session.categories = [
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
        if self.read_only && !matches!(command, VoiceCommand::Finish | VoiceCommand::Cancel) {
            return;
        }
        let allowed = match command {
            VoiceCommand::Check => matches!(
                self.voice_state,
                VoiceState::Ready | VoiceState::Unavailable
            ),
            VoiceCommand::Start => self.voice_state == VoiceState::Ready && !self.directing,
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
            VoiceEvent::Started { id, started } => {
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
    fn submit_direction(&mut self) {
        if self.read_only || self.replay.is_some() || self.directing {
            return;
        }
        let instruction = self.direction_draft.text.trim().to_owned();
        if instruction.is_empty() {
            return;
        }
        if !self.pending_recordings.is_empty() {
            self.direction_status =
                "Wait for recorded thoughts to finish splitting, then apply this direction".into();
            return;
        }
        let request = tacit::direction::Request {
            session: self.projection.session.clone(),
            instruction,
        };
        if self
            .director
            .as_ref()
            .is_some_and(|w| w.send.send(request).is_ok())
        {
            self.directing = true;
            self.direction_status = "Considering your direction…".into();
        } else {
            self.direction_status =
                "Map direction needs OpenRouter; your instruction is kept here".into();
        }
    }
    fn receive_direction(&mut self, response: tacit::direction::Response, now: f64) {
        self.directing = false;
        if self.read_only || self.replay.is_some() {
            return;
        }
        if tacit::timeline::content_hash(&self.projection.session)
            .ok()
            .as_ref()
            != Some(&response.fingerprint)
        {
            self.direction_status = "The map changed while planning. Apply the instruction again to use the latest map.".into();
            return;
        }
        match response.result {
            Ok(plan) => {
                if !plan.edits.is_empty() {
                    let change = SessionEvent::Directed {
                        instruction: response.instruction.clone(),
                        edits: plan.edits,
                    };
                    if let Err(error) = change.reduce(&self.projection.session) {
                        self.direction_status = error;
                        return;
                    }
                    self.checkpoint();
                    if let Err(error) = self.dispatch_session(change) {
                        self.direction_status = error;
                        return;
                    }
                    // Explicit edits remain stable until new brainstorming content arrives.
                    self.requested = Some(self.revision);
                    self.review_requested = Some(self.revision);
                    self.review_applied = Some(self.revision);
                    self.busy = false;
                    self.settled_at = now;
                    self.settling = 0.0;
                    if self.direction_draft.text.trim() == response.instruction {
                        self.direction_draft = TextEdit::new("");
                    }
                    self.save();
                }
                self.direction_status = plan.explanation;
            }
            Err(error) => self.direction_status = error,
        }
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
        if self.sessions_open || self.replay.is_some() || self.read_only || self.journal.is_none() {
            return;
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
                session: self.projection.session.clone(),
                render_time: Some(self.art.now),
                session_title: self.session_name.clone(),
                direction_mode: self.direction_mode,
                direction_draft: self.direction_draft.text.clone(),
                direction_status: self.direction_status.clone(),
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
            journal.present_scene(journal.now_ms(), scene)?;
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
        }
    }
    fn restore_scene(&mut self, scene: tacit::timeline::Scene, now: f64) {
        self.session_name = scene.session_title.clone();
        self.direction_mode = scene.direction_mode;
        self.direction_draft = TextEdit::new(&scene.direction_draft);
        self.direction_status = scene.direction_status.clone();
        let render_now = if self.replay.is_some() {
            scene.render_time.unwrap_or(now)
        } else {
            now
        };
        if let Some(journal) = &self.journal {
            self.projection = if self.replay.is_some() {
                journal
                    .at((now * 1000.0).round() as u64)
                    .expect("validated timeline")
            } else {
                journal.projection.clone()
            };
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
        self.art_style = scene.style;
        self.story = scene.story;
        self.art = scene.garden;
        self.art.now = render_now;
        self.gallery.now = render_now;
        self.reduced_motion = scene.reduced_motion;
        self.busy = scene.busy;
        self.settled_at = scene.settled_at;
        self.settling = ((render_now - self.settled_at) / 0.85).clamp(0.0, 1.0) as f32;
        self.pulse = ((render_now * 3.0).sin() as f32 + 1.0) / 2.0;
        self.organizer_status = scene.organizer_status;
        self.voice_status = scene.voice_status;
        self.gallery.status = scene.art_status;
        self.gallery.layers = scene
            .artwork
            .into_iter()
            .filter_map(|a| {
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
                    encoded: None,
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
    fn export_video(&mut self) {
        if self.video_export.is_some() {
            return;
        }
        let Some(journal) = &self.journal else {
            return;
        };
        let source = journal.root.clone();
        let output = source.join(format!("original-{}.mkv", tacit::timeline::unix_ms()));
        let (send, receive) = std::sync::mpsc::channel();
        self.video_export = Some(receive);
        self.status = "Exporting original pixels and audio as lossless video…".into();
        std::thread::spawn(move || {
            let result = tacit::timeline::Journal::read(&source)
                .and_then(|journal| tacit::video_archive::export(&journal, &output))
                .map(|()| output)
                .map_err(|e| e.to_string());
            let _ = send.send(result);
        });
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
        self.journal
            .as_mut()
            .ok_or("No archive for original pixels")?
            .begin_pixels()
            .map_err(|e| e.to_string())?;
        self.record_presentation("displayed frame");
        if self.read_only {
            return Err(self.status.clone());
        }
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
        self.dispatch(tacit::timeline::Event::Pixels {
            frame: frame.clone(),
        })?;
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
        if self.archive_only {
            return;
        }
        if matches!(
            self.voice_state,
            VoiceState::Recording | VoiceState::Starting | VoiceState::Finishing
        ) || !self.pending_recordings.is_empty()
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
            if let Some(worker) = &self.gallery.worker {
                let _ = worker.send.send(vec![]);
            }
            self.replay = Some(Replay {
                at_ms: self.journal.as_ref().unwrap().first_frame_ms().unwrap(),
                playing: false,
                last_tick: None,
                last_projected: None,
            });
            self.read_only = true;
        }
    }
    fn seek_replay(&mut self, delta: i64) {
        let end = self.journal.as_ref().map_or(0, |j| j.duration());
        if let Some(replay) = &mut self.replay {
            replay.at_ms = replay.at_ms.saturating_add_signed(delta).min(end).max(
                self.journal
                    .as_ref()
                    .and_then(|j| j.first_frame_ms())
                    .unwrap_or(0),
            );
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
            if j.projection.direction_recordings.contains(recording) || text.trim().is_empty() || j.entries.iter().any(|e|matches!(&e.event,tacit::timeline::Event::TranscriptLinked{recording:id,..} if id==recording)){return None;}
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
                    matches!(e, tacit::timeline::Event::SessionChanged { change: SessionEvent::Directed { instruction, .. } } if instruction.trim() == text.trim())
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
                Ok(()) => "Saved on this device".into(),
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
            self.save();
        }
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
            self.save();
        }
    }
    fn undo(&mut self) {
        if self.read_only {
            return;
        }
        self.pending_recordings.clear();
        if let Some(previous) = self.history.pop() {
            self.revision += 1;
            if self
                .dispatch_session(SessionEvent::Restored {
                    session: previous,
                    reason: "undo".into(),
                })
                .is_err()
            {
                return;
            }
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
    let width = if s.replay.is_some() && s.map_viewport[0] > 0.0 {
        s.map_viewport[0]
    } else {
        cx.layout_size("map").map_or(920.0, |size| size[0])
    }
    .max(700.0);
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
    let counts: Vec<usize> = (0..groups.len())
        .map(|i| {
            s.projection
                .session
                .thoughts
                .iter()
                .filter(|t| matches(t, i))
                .count()
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
            .projection
            .session
            .thoughts
            .iter()
            .filter(|t| matches(t, group))
            .map(|t| (Some(t.id), t.map_label().to_owned()))
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
                                if s.projection
                                    .session
                                    .thoughts
                                    .iter()
                                    .any(|t| t.id == id && t.pinned)
                                {
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
                node = node.on_press(move |s: &mut State| {
                    if s.replay.is_none() {
                        s.selected = Some(id);
                    }
                });
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
                .opacity(opacity * (1.0 - s.gallery.coverage(&layer.concept.key))),
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
            ),
    );
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
fn replay_controls(s: &State) -> El {
    let replay = s.replay.as_ref().unwrap();
    col()
        .gap(8.0)
        .child(
            row()
                .gap(8.0)
                .child(button("−10s", "replay-back", |s| s.seek_replay(-10000)))
                .child(button(
                    if replay.playing { "Pause" } else { "Play" },
                    "replay-play",
                    |s| {
                        let end = s.journal.as_ref().map_or(0, |j| j.duration());
                        if let Some(r) = &mut s.replay {
                            if r.at_ms >= end {
                                r.at_ms = s
                                    .journal
                                    .as_ref()
                                    .and_then(|j| j.first_frame_ms())
                                    .unwrap_or(0);
                            }
                            r.playing = !r.playing;
                            r.last_tick = None;
                        }
                    },
                ))
                .child(button("+10s", "replay-forward", |s| s.seek_replay(10000)))
                .child(button(
                    "Next event",
                    "replay-next",
                    State::next_replay_event,
                ))
                .child(label(
                    format!(
                        "{} / {}",
                        tacit::timeline::timestamp(replay.at_ms),
                        tacit::timeline::timestamp(s.journal.as_ref().map_or(0, |j| j.duration()))
                    ),
                    12.0,
                    INK,
                )),
        )
        .child(label(s.replay_caption(), 11.0, MUTED))
        .child(
            row()
                .gap(8.0)
                .child(button(
                    if s.pixel_replay {
                        "Scene replay"
                    } else {
                        "Original pixels"
                    },
                    "replay-renderer",
                    |s| {
                        s.pixel_replay = !s.pixel_replay;
                        if let Some(replay) = &mut s.replay {
                            replay.last_projected = None;
                        }
                    },
                ))
                .child(button(
                    "Export lossless video",
                    "export-video",
                    State::export_video,
                ))
                .child(if s.archive_only {
                    label("Saved archive", 11.0, MUTED)
                } else {
                    button("Back to live", "back-live", State::toggle_replay)
                }),
        )
}
fn sessions_button(s: &State) -> El {
    if s.library_base.is_none() {
        return el();
    }
    button("Sessions", "sessions", |s| {
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
pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    if s.sessions_open {
        return sessions_view(s);
    }
    if s.replay.is_some() && s.pixel_replay {
        let picture = if let Some((_, image)) = &s.pixel_image {
            el().pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
                .w_full()
                .h(0.0)
                .grow(1.0)
                .min_h(0.0)
        } else {
            label("No original pixels were captured at this time.", 18.0, INK).grow(1.0)
        };
        return col()
            .w_full()
            .h_full()
            .fill(PAPER)
            .child(picture)
            .child(replay_controls(s).padding(12.0))
            .child(sessions_button(s))
            .child(label(s.status.clone(), 11.0, MUTED));
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
                s.art_style = s.art_style.next();
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
            .child(button("Cancel", "voice-cancel", |s| {
                s.voice_command(VoiceCommand::Cancel)
            }))
            .child(
                button(
                    if s.direction_mode {
                        "Stop & direct"
                    } else {
                        "Stop & transcribe"
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
                            if s.directing {
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
                button("Retry art", "retry-art", |s| s.gallery.retry())
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
            .child(label(s.projection.session.placement(t), 12.0, MUTED))
            .child(if s.read_only {
                el()
            } else {
                button("Move to next branch", "move", State::move_selected)
            })
            .child(if s.read_only {
                el()
            } else {
                button("Remove thought", "remove", State::remove_selected)
            });
    } else {
        panel = panel.child(label(
            "Select a thought to inspect its original words or change where it belongs.",
            14.0,
            MUTED,
        ));
    }
    let selected_theme = s
        .projection
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
    let mut r = row().gap(10.0).center_y();
    if let Some(t) = s
        .projection
        .session
        .thoughts
        .iter()
        .find(|t| Some(t.id) == s.selected)
    {
        r = r
            .child(
                label(
                    format!(
                        "Selected {:02} · {}",
                        t.id,
                        s.projection.session.placement(t)
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
pub fn on_key(s: &mut State, event: &KeyEvent, _: &mut Tx) -> bool {
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
            let direction_wake = wake.clone();
            s.director = Some(tacit::direction::Worker::start(config.clone(), move || {
                direction_wake()
            }));
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
pub fn poll(s: &mut State, frame_now: f64, tx: &mut Tx) -> bool {
    let mut export_changed = false;
    if let Some(receiver) = &s.video_export {
        match receiver.try_recv() {
            Ok(result) => {
                s.status = result.map_or_else(
                    |e| format!("Video export failed: {e}"),
                    |p| format!("Original video saved: {}", p.display()),
                );
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
        let scene = if changed {
            s.journal
                .as_ref()
                .map(|j| j.replay_scene_at(at).expect("validated presentation"))
        } else {
            None
        };
        if let Some(scene) = scene {
            let scroll = scene.map_scroll;
            let story_scroll = scene.story_scroll;
            let inspector_scroll = scene.inspector_scroll;
            s.restore_scene(scene, at as f64 / 1000.0);
            tx.scroll_to_instant("map", scroll[0], scroll[1]);
            tx.scroll_to_instant("story", story_scroll[0], story_scroll[1]);
            tx.scroll_to_instant("inspector", inspector_scroll[0], inspector_scroll[1]);
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
    let changed = poll_live(s, now, tx);
    s.record_presentation("scene changed");
    changed || export_changed
}
fn poll_live(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    let art_deadline = s.art.update(&s.projection.session, now);
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
    while let Some(response) = s.director.as_ref().and_then(|w| w.receive.try_recv().ok()) {
        s.receive_direction(response, now);
        changed = true;
    }
    if s.directing {
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
            match response.result.and_then(|interpretation| {
                s.dispatch_session(SessionEvent::Organized {
                    interpretation,
                    pass: "review".into(),
                })
            }) {
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
            match response.result.and_then(|interpretation| {
                s.dispatch_session(SessionEvent::Organized {
                    interpretation,
                    pass: "fast".into(),
                })
            }) {
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
    if s.requested != Some(s.revision) && !s.projection.session.thoughts.is_empty() {
        if now >= s.due {
            let request = tacit::organizer::Request {
                revision: s.revision,
                session: s.projection.session.clone(),
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
        && s.pending_recordings.is_empty()
        && s.review_requested != Some(s.revision)
        && s.fast_finished == Some(s.revision)
        && !s.projection.session.thoughts.is_empty()
    {
        if now >= s.review_due {
            let request = tacit::organizer::Request {
                revision: s.revision,
                session: s.projection.session.clone(),
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
        assert!(!s.directing); // No provider: retain instruction, never fall back to capture.
        s.voice_state = VoiceState::Recording;
        s.toggle_direction();
        assert!(s.direction_mode);
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
    fn directions_are_replayable_undoable_and_stale_plans_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = State::open_session(dir.path().join("session.json")).unwrap();
        s.draft = TextEdit::new("A real idea");
        s.capture();
        let before = s.projection.session.clone();
        let fingerprint = tacit::timeline::content_hash(&before).unwrap();
        let response = || tacit::direction::Response {
            fingerprint: fingerprint.clone(),
            instruction: "Call this map Research".into(),
            result: Ok(tacit::direction::Plan {
                explanation: "Renamed the central theme to Research".into(),
                edits: vec![tacit::direction::Edit::SetTitle {
                    title: "Research".into(),
                }],
            }),
        };
        s.receive_direction(response(), 2.0);
        assert_eq!(s.projection.session.title, "Research");
        assert_eq!(s.projection.session.thoughts, before.thoughts);
        let j = s.journal.as_ref().unwrap();
        assert_eq!(j.at(j.duration()).unwrap().session, s.projection.session);
        let changed = s.projection.session.clone();
        s.receive_direction(response(), 3.0);
        assert_eq!(s.projection.session, changed);
        assert!(s.direction_status.contains("map changed"));
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
        let dir = tempfile::tempdir().unwrap();
        let mut state = State::empty();
        state.journal = Some(
            tacit::timeline::Journal::open(&dir.path().join("session.json"), Session::default())
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
                key: "theme-one".into(),
                fingerprint: "same-prompt".into(),
                slot: 0,
                image,
                encoded: Some(bytes.clone()),
                born: 0.0,
                retired: None,
            }];
            state.record_presentation("art arrived");
            assert!(!state.read_only);
            let j = state.journal.as_ref().unwrap();
            let asset = j.projection.scene().artwork[0].asset.clone();
            assert_eq!(std::fs::read(j.asset(&asset).unwrap()).unwrap(), bytes);
            assets.push(asset);
        }
        assert_ne!(assets[0], assets[1]);
        state.journal.as_ref().unwrap().validate_assets().unwrap();
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
        let before = s.projection.session.clone();
        review_response
            .send(tacit::organizer::Response {
                revision: 1,
                result: Err("stale review".into()),
            })
            .unwrap();
        poll(&mut s, 3.2, &mut tx);
        assert_eq!(s.projection.session, before);
        assert!(!s.organizer_status.contains("stale review"));
        poll(&mut s, 3.4, &mut tx);
        assert_eq!(fast_requests.try_recv().unwrap().revision, 2);
    }
    #[test]
    fn background_art_style_cycles_without_changing_the_map() {
        let mut rt = runtime((820.0, 640.0));
        let before = rt.state.projection.session.clone();
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
                .projection
                .session
                .thoughts
                .iter()
                .any(|t| t.text == "A thought")
        );
    }
}
