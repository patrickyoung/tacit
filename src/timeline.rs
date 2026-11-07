//! Versioned, append-only session history. The reducer never calls a model.
use crate::{model::Session, session_event::SessionEvent};
// v1 compatibility only; new events use an opaque renderer-specific presentation track.
pub use crate::presentation::{Artwork, Scene};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const VERSION: u32 = 2;
pub const RENDERER: &str = concat!("tacit-", env!("CARGO_PKG_VERSION"), "-scene-1-rune-801e61a");
pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn root_for(path: &Path) -> PathBuf {
    path.with_extension("tacit")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Audio {
    pub id: String,
    pub asset: String,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub samples: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    MapRevised {
        fingerprint: String,
        plan: crate::evolution::Plan,
    },
    SpeechSegment {
        recording: String,
        segment: u64,
        cue: Cue,
        thought_id: u64,
    },
    RecordingDirected {
        id: String,
    },
    CaptureBoundary {
        recording: bool,
    },
    Pixels {
        frame: PixelFrame,
    },
    Frame {
        renderer: String,
        time: f64,
    },
    Transaction {
        events: Vec<Event>,
    },
    Opened {
        version: u32,
        renderer: String,
        origin_unix_ms: u64,
    },
    Scene {
        reason: String,
        scene: Box<Scene>,
    },
    SessionChanged {
        change: SessionEvent,
    },
    ArtworkAvailable {
        artwork: SessionArtwork,
    },
    Presentation {
        renderer: String,
        version: u32,
        state: serde_json::Value,
    },
    TranscriptLinked {
        recording: String,
        thought_id: u64,
    },
    ThoughtsSplit {
        source_id: u64,
        thought_ids: Vec<u64>,
        byte_ends: Vec<usize>,
    },
    RecordingStarted {
        id: String,
        start_ms: u64,
    },
    RecordingCancelled {
        id: String,
    },
    AudioSaved {
        audio: Audio,
    },
    Transcript {
        recording: String,
        cues: Vec<Cue>,
        text: String,
    },
    Failure {
        operation: String,
        message: String,
    },
}
impl Event {
    pub fn leaves(&self) -> Box<dyn Iterator<Item = &Event> + '_> {
        match self {
            Self::Transaction { events } => Box::new(events.iter().flat_map(Event::leaves)),
            _ => Box::new(std::iter::once(self)),
        }
    }
}
pub fn content_hash(session: &Session) -> io::Result<String> {
    Ok(
        blake3::hash(&serde_json::to_vec(session).map_err(io::Error::other)?)
            .to_hex()
            .to_string(),
    )
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PixelFrame {
    pub asset: String,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub content_hash: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub seq: u64,
    pub at_ms: u64,
    pub previous: String,
    pub event: Event,
    pub hash: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkScope {
    #[default]
    Theme,
    Map,
}
impl ArtworkScope {
    fn is_theme(&self) -> bool {
        *self == Self::Theme
    }
}
/// Content-addressed illustration with renderer-independent semantic scope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionArtwork {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mural: Option<crate::artist::Mural>,
    #[serde(default, skip_serializing_if = "ArtworkScope::is_theme")]
    pub scope: ArtworkScope,
    pub theme: String,
    pub fingerprint: String,
    pub asset: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Presentation {
    pub version: u32,
    pub state: serde_json::Value,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ThoughtSplit {
    pub source_id: u64,
    pub thought_ids: Vec<u64>,
    pub byte_ends: Vec<usize>,
}
/// Finalized speech window and the original thought created from it; splits retain provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpeechSegment {
    pub cue: Cue,
    pub thought_id: u64,
}
/// Renderer-independent, serializable view. Presentation tracks are intentionally excluded.
#[derive(Serialize)]
pub struct SessionView<'a> {
    pub sources: crate::evolution::Sources,
    pub session: &'a Session,
    pub artwork: &'a [SessionArtwork],
    pub audio: &'a [Audio],
    pub transcripts: &'a [(String, Vec<Cue>, String)],
    pub transcript_links: &'a std::collections::BTreeMap<String, u64>,
    pub splits: &'a [ThoughtSplit],
    pub direction_recordings: &'a std::collections::BTreeSet<String>,
    pub speech_segments:
        &'a std::collections::BTreeMap<String, std::collections::BTreeMap<u64, SpeechSegment>>,
}
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Projection {
    pub source_words: crate::evolution::Sources,
    pub session: Session,
    pub artwork: Vec<SessionArtwork>,
    pub presentations: std::collections::BTreeMap<String, Presentation>,
    pub pixel_frame: Option<PixelFrame>,
    pub pixel_capture_active: Option<bool>,
    pub frame_times: std::collections::BTreeMap<String, f64>,
    pub presented_sessions: std::collections::BTreeMap<String, Session>,
    pub audio: Vec<Audio>,
    pub transcripts: Vec<(String, Vec<Cue>, String)>,
    pub transcript_links: std::collections::BTreeMap<String, u64>,
    pub splits: Vec<ThoughtSplit>,
    pub direction_recordings: std::collections::BTreeSet<String>,
    pub speech_segments:
        std::collections::BTreeMap<String, std::collections::BTreeMap<u64, SpeechSegment>>,
}
impl Projection {
    pub fn content(&self) -> SessionView<'_> {
        SessionView {
            sources: self.evidence(),
            session: &self.session,
            artwork: &self.artwork,
            audio: &self.audio,
            transcripts: &self.transcripts,
            transcript_links: &self.transcript_links,
            splits: &self.splits,
            direction_recordings: &self.direction_recordings,
            speech_segments: &self.speech_segments,
        }
    }
    fn retain_artwork(&mut self, artwork: SessionArtwork) {
        if !self.artwork.contains(&artwork) {
            self.artwork.push(artwork);
        }
    }
    pub fn evidence(&self) -> crate::evolution::Sources {
        let mut sources = self.session.sources.clone();
        sources.extend(self.source_words.clone());
        for t in self
            .session
            .thoughts
            .iter()
            .chain(self.session.retired.iter().map(|r| &r.thought))
        {
            if t.evidence.is_empty() {
                sources
                    .entry(format!("thought-{}", t.id))
                    .or_insert_with(|| t.text.clone());
            }
        }
        for (id, _, text) in &self.transcripts {
            if !self.direction_recordings.contains(id) {
                sources.insert(format!("transcript-{id}"), text.clone());
            }
        }
        sources
    }
    fn remember_sources(&mut self) {
        for t in &self.session.thoughts {
            if t.evidence.is_empty() {
                self.source_words
                    .entry(format!("thought-{}", t.id))
                    .or_insert_with(|| t.text.clone());
            }
        }
    }
    pub fn apply(&mut self, event: &Event) -> io::Result<()> {
        if matches!(event, Event::Transaction { .. }) {
            let mut next = self.clone();
            next.apply_staged(event)?;
            *self = next;
            Ok(())
        } else {
            self.apply_staged(event)
        }
    }
    /// The caller owns an unpublished projection and discards it on failure.
    fn apply_staged(&mut self, event: &Event) -> io::Result<()> {
        match event {
            Event::MapRevised { fingerprint, plan } => {
                if content_hash(&self.session)? != *fingerprint {
                    return Err(io::Error::other("Map changed during editorial review"));
                }
                let next = plan
                    .apply(&self.session, &self.evidence())
                    .map_err(io::Error::other)?;
                self.remember_sources();
                self.session = next;
            }
            Event::SpeechSegment {
                recording,
                segment,
                cue,
                thought_id,
            } => {
                safe_asset(recording)?;
                let item = SpeechSegment {
                    cue: cue.clone(),
                    thought_id: *thought_id,
                };
                if let Some(existing) = self
                    .speech_segments
                    .get(recording)
                    .and_then(|s| s.get(segment))
                {
                    if existing == &item {
                        return Ok(());
                    }
                    return Err(io::Error::other("Conflicting finalized speech segment"));
                }
                if cue.start_ms >= cue.end_ms
                    || cue.text.trim().is_empty()
                    || cue.text.len() > 96000
                    || !self
                        .session
                        .thoughts
                        .iter()
                        .any(|t| t.id == *thought_id && t.text == cue.text.trim())
                    || self.speech_segments.get(recording).is_some_and(|s| {
                        s.values().any(|old| {
                            old.cue.start_ms < cue.end_ms && cue.start_ms < old.cue.end_ms
                        })
                    })
                {
                    return Err(io::Error::other("Invalid finalized speech segment"));
                }
                self.speech_segments
                    .entry(recording.clone())
                    .or_default()
                    .insert(*segment, item);
            }
            Event::RecordingDirected { id } => {
                self.direction_recordings.insert(id.clone());
            }
            Event::CaptureBoundary { recording } => {
                self.pixel_capture_active = Some(*recording);
                if *recording {
                    self.pixel_frame = None;
                }
            }
            Event::Pixels { frame } => {
                safe_asset(&frame.asset)?;
                if frame.width == 0
                    || frame.height == 0
                    || !frame.scale.is_finite()
                    || frame.scale <= 0.0
                    || frame.content_hash != content_hash(&self.session)?
                {
                    return Err(io::Error::other("Invalid original pixel frame"));
                }
                self.pixel_frame = Some(frame.clone());
            }
            Event::Frame { renderer, time } => {
                if !time.is_finite() || *time < 0.0 || !self.presentations.contains_key(renderer) {
                    return Err(io::Error::other("Invalid presentation frame"));
                }
                self.frame_times.insert(renderer.clone(), *time);
            }
            Event::Transaction { events } => {
                if events.is_empty()
                    || events
                        .iter()
                        .any(|e| matches!(e, Event::Transaction { .. } | Event::Opened { .. }))
                {
                    return Err(io::Error::other("Invalid event transaction"));
                }
                for event in events {
                    self.apply_staged(event)?;
                }
            }
            Event::Opened { version, .. } if ![1, VERSION].contains(version) => {
                return Err(io::Error::other("Unsupported timeline version"));
            }
            Event::Scene { scene, .. } => {
                scene.session.validate().map_err(io::Error::other)?;
                for art in &scene.artwork {
                    safe_asset(&art.asset)?;
                }
                self.remember_sources();
                self.session = scene.session.clone();
                self.remember_sources();
                for art in &scene.artwork {
                    self.retain_artwork(SessionArtwork {
                        mural: art.mural.clone(),
                        scope: if art.key == crate::generated_art::CENTRAL_KEY {
                            ArtworkScope::Map
                        } else {
                            ArtworkScope::Theme
                        },
                        theme: art.key.strip_prefix("theme-").unwrap_or(&art.key).into(),
                        fingerprint: art.fingerprint.clone(),
                        asset: art.asset.clone(),
                    });
                }
                self.presented_sessions
                    .insert(RENDERER.into(), self.session.clone());
                self.presentations.insert(
                    RENDERER.into(),
                    Presentation {
                        version: 1,
                        state: scene.presentation_value()?,
                    },
                );
            }
            Event::SessionChanged { change } => {
                let next = change.reduce(&self.session).map_err(io::Error::other)?;
                if let SessionEvent::Split {
                    source_id, chunks, ..
                } = change
                {
                    let index = next
                        .thoughts
                        .iter()
                        .position(|t| t.id == *source_id)
                        .unwrap();
                    let mut end = 0;
                    self.splits.push(ThoughtSplit {
                        source_id: *source_id,
                        thought_ids: next.thoughts[index..index + chunks.len()]
                            .iter()
                            .map(|t| t.id)
                            .collect(),
                        byte_ends: chunks
                            .iter()
                            .map(|c| {
                                end += c.len();
                                end
                            })
                            .collect(),
                    });
                }
                self.remember_sources();
                self.session = next;
                self.remember_sources();
            }
            Event::TranscriptLinked {
                recording,
                thought_id,
            } => {
                self.transcript_links.insert(recording.clone(), *thought_id);
            }
            Event::ThoughtsSplit {
                source_id,
                thought_ids,
                byte_ends,
            } => {
                let split = ThoughtSplit {
                    source_id: *source_id,
                    thought_ids: thought_ids.clone(),
                    byte_ends: byte_ends.clone(),
                };
                if !self.splits.contains(&split) {
                    self.splits.push(split);
                }
            }
            Event::ArtworkAvailable { artwork } => {
                safe_asset(&artwork.asset)?;
                self.retain_artwork(artwork.clone());
            }
            Event::Presentation {
                renderer,
                version,
                state,
            } => {
                if let Some(expected) = state.get("content_hash").and_then(|v| v.as_str())
                    && expected != content_hash(&self.session)?
                {
                    return Err(io::Error::other(
                        "Presentation content revision does not match",
                    ));
                }
                self.presented_sessions
                    .insert(renderer.clone(), self.session.clone());
                self.presentations.insert(
                    renderer.clone(),
                    Presentation {
                        version: *version,
                        state: state.clone(),
                    },
                );
            }
            Event::AudioSaved { audio } => {
                safe_asset(&audio.asset)?;
                safe_asset(&audio.id)?;
                if audio.sample_rate == 0
                    || audio.duration_ms != audio.samples * 1000 / audio.sample_rate as u64
                    || self.audio.iter().any(|a| a.id == audio.id)
                {
                    return Err(io::Error::other("Invalid audio event"));
                }
                self.audio.push(audio.clone());
            }
            Event::Transcript {
                recording,
                cues,
                text,
            } => {
                let audio = self
                    .audio
                    .iter()
                    .find(|a| &a.id == recording)
                    .ok_or_else(|| io::Error::other("Transcript has no audio"))?;
                validate_cues(cues, audio.duration_ms)?;
                if self.transcripts.iter().any(|(id, _, _)| id == recording) {
                    return Err(io::Error::other("Duplicate transcript"));
                }
                self.transcripts
                    .push((recording.clone(), cues.clone(), text.clone()));
            }
            _ => {}
        }
        Ok(())
    }
}
pub fn validate_cues(cues: &[Cue], duration: u64) -> io::Result<()> {
    let mut previous = 0;
    for c in cues {
        if c.start_ms < previous
            || c.end_ms <= c.start_ms
            || c.end_ms > duration
            || c.text.trim().is_empty()
        {
            return Err(io::Error::other("Invalid transcript timing"));
        }
        previous = c.start_ms;
    }
    Ok(())
}
fn safe_asset(name: &str) -> io::Result<()> {
    if name.is_empty()
        || Path::new(name).components().count() != 1
        || name.contains(['/', '\\'])
        || name.starts_with('.')
    {
        return Err(io::Error::other("Invalid asset name"));
    }
    Ok(())
}
fn hash(seq: u64, at_ms: u64, previous: &str, event: &Event) -> io::Result<String> {
    let bytes = serde_json::to_vec(&(seq, at_ms, previous, event)).map_err(io::Error::other)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}
pub struct Journal {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub projection: Projection,
    pub origin_unix_ms: u64,
    file: File,
    clock_anchor: std::time::Instant,
    clock_base_ms: u64,
    checkpoints: Vec<(usize, Projection)>,
    capturing_pixels: bool,
}
impl Journal {
    pub fn open(path: &Path, legacy: Session) -> io::Result<Self> {
        Self::open_mode(path, legacy, true)
    }
    pub fn read(path: &Path) -> io::Result<Self> {
        Self::open_mode(path, Session::default(), false)
    }
    fn open_mode(path: &Path, legacy: Session, writable: bool) -> io::Result<Self> {
        let root = root_for(path);
        if writable {
            private_dir(&root)?;
            private_dir(&root.join("media"))?;
        }
        let log = root.join("events.jsonl");
        let bytes = match fs::read(&log) {
            Ok(b) => b,
            Err(e) if writable && e.kind() == io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(e),
        };
        let complete = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        let mut entries = Vec::<Entry>::new();
        let mut projection = Projection::default();
        let mut checkpoints = Vec::new();
        for line in bytes[..complete]
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
        {
            let e: Entry = serde_json::from_slice(line).map_err(io::Error::other)?;
            let previous = entries.last().map_or("", |e| e.hash.as_str());
            if e.seq != entries.len() as u64
                || e.previous != previous
                || e.hash != hash(e.seq, e.at_ms, previous, &e.event)?
                || entries.last().is_some_and(|p| p.at_ms > e.at_ms)
            {
                return Err(io::Error::other(
                    "Timeline sequence or integrity check failed",
                ));
            }
            if entries.is_empty() && !matches!(e.event, Event::Opened { .. }) {
                return Err(io::Error::other("Missing timeline header"));
            }
            projection.apply_staged(&e.event)?;
            entries.push(e);
            if entries.len().is_multiple_of(64) {
                checkpoints.push((entries.len(), projection.clone()));
            }
        }
        if writable && complete != bytes.len() {
            atomic_write(&root.join("interrupted-tail.json"), &bytes[complete..])?;
        }
        let file = if writable {
            let file = private_file(&log, true)?;
            file.set_len(complete as u64)?;
            file
        } else {
            File::open(&log)?
        };
        let origin_unix_ms = entries
            .first()
            .and_then(|e| match e.event {
                Event::Opened { origin_unix_ms, .. } => Some(origin_unix_ms),
                _ => None,
            })
            .unwrap_or_else(unix_ms);
        let clock_base_ms = unix_ms()
            .saturating_sub(origin_unix_ms)
            .max(entries.last().map_or(0, |e| e.at_ms));
        let mut journal = Self {
            clock_anchor: std::time::Instant::now(),
            clock_base_ms,
            checkpoints,
            capturing_pixels: false,
            root,
            entries,
            projection,
            origin_unix_ms,
            file,
        };
        if journal.entries.is_empty() {
            if !writable {
                return Err(io::Error::other("Empty timeline"));
            }
            journal.append(
                0,
                Event::Opened {
                    version: VERSION,
                    renderer: RENDERER.into(),
                    origin_unix_ms,
                },
            )?;
            journal.append(
                0,
                Event::SessionChanged {
                    change: SessionEvent::Restored {
                        session: legacy,
                        reason: "initial/import".into(),
                    },
                },
            )?;
        }
        if writable {
            if journal.projection.pixel_capture_active == Some(true) {
                journal.append(
                    journal.now_ms(),
                    Event::Failure {
                        operation: "original video capture".into(),
                        message:
                            "Previous capture ended unexpectedly; completeness cannot be guaranteed"
                                .into(),
                    },
                )?;
                journal.append(
                    journal.now_ms(),
                    Event::CaptureBoundary { recording: false },
                )?;
            }
            journal.recover_recordings()?;
        }
        journal.validate_assets()?;
        if writable {
            journal.export_vtt()?;
        }
        Ok(journal)
    }
    fn recover_recordings(&mut self) -> io::Result<()> {
        let started: Vec<_> = self
            .entries
            .iter()
            .flat_map(|e| e.event.leaves())
            .filter_map(|e| match e {
                Event::RecordingStarted { id, start_ms } => Some((id.clone(), *start_ms)),
                _ => None,
            })
            .collect();
        for (id, start_ms) in started {
            safe_asset(&id)?;
            if self.projection.audio.iter().any(|a| a.id == id)
                || self
                    .entries
                    .iter()
                    .flat_map(|e| e.event.leaves())
                    .any(|e| matches!(e,Event::RecordingCancelled{id:cancelled} if cancelled==&id))
            {
                continue;
            }
            let path = self.root.join("recordings").join(format!("{id}.wav"));
            if path.exists()
                && let Ok((samples, sample_rate)) = crate::voice::wav_info(&path)
            {
                let asset = self.import(&path, "wav")?;
                self.append(
                    self.now_ms(),
                    Event::AudioSaved {
                        audio: Audio {
                            id,
                            asset,
                            start_ms,
                            duration_ms: samples * 1000 / sample_rate as u64,
                            sample_rate,
                            samples,
                        },
                    },
                )?;
            }
        }
        Ok(())
    }
    pub fn validate_assets(&self) -> io::Result<()> {
        let mut names = std::collections::HashSet::new();
        for event in self.entries.iter().flat_map(|e| e.event.leaves()) {
            match event {
                Event::Scene { scene, .. } => {
                    for a in &scene.artwork {
                        names.insert(a.asset.as_str());
                    }
                }
                Event::AudioSaved { audio } => {
                    names.insert(audio.asset.as_str());
                }
                Event::ArtworkAvailable { artwork } => {
                    names.insert(artwork.asset.as_str());
                }
                Event::Pixels { frame } => {
                    names.insert(frame.asset.as_str());
                }
                _ => {}
            }
        }
        for name in names {
            let path = self.asset(name)?;
            let bytes = fs::read(&path)
                .map_err(|e| io::Error::other(format!("Missing replay asset {name}: {e}")))?;
            let expected = name.split('.').next().unwrap_or_default();
            if blake3::hash(&bytes).to_hex().as_str() != expected {
                return Err(io::Error::other(format!(
                    "Replay asset checksum failed: {name}"
                )));
            }
        }
        Ok(())
    }
    pub fn now_ms(&self) -> u64 {
        self.clock_base_ms
            .saturating_add(self.clock_anchor.elapsed().as_millis() as u64)
            .max(self.duration())
    }
    pub fn duration(&self) -> u64 {
        self.entries.last().map_or(0, |e| e.at_ms).max(
            self.projection
                .audio
                .iter()
                .map(|a| a.start_ms + a.duration_ms)
                .max()
                .unwrap_or(0),
        )
    }
    pub fn begin_pixels(&mut self) -> io::Result<()> {
        if !self.capturing_pixels {
            self.append(self.now_ms(), Event::CaptureBoundary { recording: true })?;
            self.capturing_pixels = true;
        }
        Ok(())
    }
    pub fn end_pixels(&mut self) -> io::Result<()> {
        if self.capturing_pixels {
            self.append(self.now_ms(), Event::CaptureBoundary { recording: false })?;
            self.capturing_pixels = false;
        }
        Ok(())
    }
    pub fn append(&mut self, at_ms: u64, event: Event) -> io::Result<()> {
        let mut next = self.projection.clone();
        next.apply_staged(&event)?;
        let seq = self.entries.len() as u64;
        let at_ms = at_ms.max(self.entries.last().map_or(0, |e| e.at_ms));
        let previous = self
            .entries
            .last()
            .map_or_else(String::new, |e| e.hash.clone());
        let hash = hash(seq, at_ms, &previous, &event)?;
        let e = Entry {
            seq,
            at_ms,
            previous,
            event,
            hash,
        };
        let mut line = serde_json::to_vec(&e).map_err(io::Error::other)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.sync_data()?;
        self.entries.push(e);
        self.projection = next;
        if self.entries.len().is_multiple_of(64) {
            self.checkpoints
                .push((self.entries.len(), self.projection.clone()));
        }
        Ok(())
    }
    pub fn at(&self, at_ms: u64) -> io::Result<Projection> {
        let end = self.entries.partition_point(|e| e.at_ms <= at_ms);
        if end == self.entries.len() {
            return Ok(self.projection.clone());
        }
        let checkpoint = self.checkpoints.partition_point(|(n, _)| *n <= end);
        let (start, mut result) = self.checkpoints[..checkpoint]
            .last()
            .cloned()
            .unwrap_or_default();
        for e in &self.entries[start..end] {
            result.apply_staged(&e.event)?;
        }
        Ok(result)
    }
    pub fn asset(&self, name: &str) -> io::Result<PathBuf> {
        safe_asset(name)?;
        Ok(self.root.join("media").join(name))
    }
    pub fn import(&self, source: &Path, extension: &str) -> io::Result<String> {
        let bytes = fs::read(source)?;
        self.import_bytes(&bytes, extension)
    }
    pub fn import_bytes(&self, bytes: &[u8], extension: &str) -> io::Result<String> {
        let name = format!("{}.{}", blake3::hash(bytes).to_hex(), extension);
        let target = self.asset(&name)?;
        if !target.exists() {
            atomic_write(&target, bytes)?;
        }
        Ok(name)
    }
    pub fn export_vtt(&self) -> io::Result<()> {
        atomic_write(
            &self.root.join("transcript.vtt"),
            vtt(&self.projection, None).as_bytes(),
        )?;
        for a in &self.projection.audio {
            atomic_write(
                &self.root.join("media").join(format!("{}.vtt", a.id)),
                vtt(&self.projection, Some(&a.id)).as_bytes(),
            )?;
        }
        Ok(())
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        let _ = self.end_pixels();
    }
}
pub fn timestamp(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn vtt(p: &Projection, recording: Option<&str>) -> String {
    let mut output = String::from(
        "WEBVTT\n\nNOTE Timings are Hex audio-window boundaries, not word alignment.\n\n",
    );
    let mut cues = Vec::new();
    for (id, segments, _) in &p.transcripts {
        if recording.is_some_and(|r| r != id) {
            continue;
        }
        if let Some(a) = p.audio.iter().find(|a| &a.id == id) {
            for (i, c) in segments.iter().enumerate() {
                let offset = if recording.is_some() { 0 } else { a.start_ms };
                cues.push((
                    c.start_ms + offset,
                    c.end_ms + offset,
                    format!("{id}-{}", i + 1),
                    escape(&c.text),
                ));
            }
        }
    }
    cues.sort_by_key(|c| c.0);
    for (start, end, id, text) in cues {
        output.push_str(&format!(
            "{id}\n{} --> {}\n{text}\n\n",
            timestamp(start),
            timestamp(end)
        ));
    }
    output
}
pub fn private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn private_file(path: &Path, append: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("tmp");
    let mut file = private_file(&temp, false)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_reduces_every_event_and_seeks_without_models() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let mut j = Journal::open(&path, Session::default()).unwrap();
        let mut scene = j.projection.scene();
        scene.session.add("First idea");
        scene.draft = "a draft".into();
        scene.style = crate::art::Style::Chalk;
        scene.settled_at = 0.1521678770492989; // Hashes must survive exact float decoding.
        j.scene(1000, "capture", scene.clone()).unwrap();
        let first = scene.clone();
        scene.session.add("Second idea");
        scene.selected = Some(2);
        scene.story = true;
        scene.map_scroll = [0.0, 42.0];
        j.scene(2000, "capture", scene.clone()).unwrap();
        assert_eq!(j.at(1500).unwrap().scene(), first);
        assert_eq!(j.at(2000).unwrap().scene(), scene);
        assert_eq!(j.scene_at(1500), first);
        let expected = j.projection.clone();
        drop(j);
        let loaded = Journal::read(&path).unwrap();
        assert_eq!(loaded.projection, expected);
        assert_eq!(loaded.at(2000).unwrap(), expected);
    }
    #[test]
    fn incomplete_tail_is_recovered_but_committed_corruption_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let j = Journal::open(&path, Session::demo()).unwrap();
        let log = j.root.join("events.jsonl");
        let expected = j.projection.clone();
        drop(j);
        OpenOptions::new()
            .append(true)
            .open(&log)
            .unwrap()
            .write_all(b"{\"seq\":2")
            .unwrap();
        let read = Journal::read(&path).unwrap();
        assert_eq!(read.projection, expected);
        drop(read);
        let j = Journal::open(&path, Session::default()).unwrap();
        assert_eq!(j.projection, expected);
        assert!(j.root.join("interrupted-tail.json").exists());
        drop(j);
        let bytes = fs::read_to_string(&log)
            .unwrap()
            .replace("Make room for meaningful work", "Tampered title");
        fs::write(&log, bytes).unwrap();
        assert!(Journal::read(&path).is_err());
    }
    #[test]
    fn audio_is_durable_vtt_is_escaped_and_timestamps_are_session_relative() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let mut j = Journal::open(&path, Session::default()).unwrap();
        let source = dir.path().join("original.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(&source, spec).unwrap();
        for _ in 0..32000 {
            wav.write_sample(0i16).unwrap();
        }
        wav.finalize().unwrap();
        let asset = j.import(&source, "wav").unwrap();
        j.append(
            3000,
            Event::AudioSaved {
                audio: Audio {
                    id: "recording-1".into(),
                    asset: asset.clone(),
                    start_ms: 1000,
                    duration_ms: 2000,
                    sample_rate: 16000,
                    samples: 32000,
                },
            },
        )
        .unwrap();
        j.append(
            4000,
            Event::Transcript {
                recording: "recording-1".into(),
                text: "<hello> & bye -->".into(),
                cues: vec![Cue {
                    start_ms: 250,
                    end_ms: 1750,
                    text: "<hello> & bye -->".into(),
                }],
            },
        )
        .unwrap();
        j.export_vtt().unwrap();
        let output = fs::read_to_string(j.root.join("transcript.vtt")).unwrap();
        assert!(output.contains("00:00:01.250 --> 00:00:02.750"));
        assert!(output.contains("&lt;hello&gt; &amp; bye --&gt;"));
        assert!(
            fs::read_to_string(j.root.join("media/recording-1.vtt"))
                .unwrap()
                .contains("00:00:00.250 --> 00:00:01.750")
        );
        fs::remove_file(source).unwrap();
        j.validate_assets().unwrap();
        fs::write(j.asset(&asset).unwrap(), b"broken").unwrap();
        assert!(j.validate_assets().is_err());
    }
    #[test]
    fn interrupted_recording_recovers_once_and_remains_available_for_transcription() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let mut j = Journal::open(&path, Session::default()).unwrap();
        j.append(
            1000,
            Event::Transaction {
                events: vec![
                    Event::RecordingStarted {
                        id: "recording-interrupted".into(),
                        start_ms: 1000,
                    },
                    Event::RecordingDirected {
                        id: "recording-interrupted".into(),
                    },
                ],
            },
        )
        .unwrap();
        let recordings = j.root.join("recordings");
        private_dir(&recordings).unwrap();
        let mut wav = hound::WavWriter::create(
            recordings.join("recording-interrupted.wav"),
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..8000 {
            wav.write_sample(0i16).unwrap();
        }
        wav.finalize().unwrap();
        drop(j);
        let j = Journal::open(&path, Session::default()).unwrap();
        assert_eq!(j.projection.audio.len(), 1);
        assert_eq!(j.projection.audio[0].duration_ms, 500);
        assert!(
            j.projection
                .direction_recordings
                .contains("recording-interrupted")
        );
        assert!(j.projection.transcripts.is_empty());
        let count = j.entries.len();
        drop(j);
        let j = Journal::open(&path, Session::default()).unwrap();
        assert_eq!(j.entries.len(), count);
        j.validate_assets().unwrap();
    }
    #[test]
    fn live_sources_survive_journal_reopen_and_reject_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let mut j = Journal::open(&path, Session::default()).unwrap();
        let id = j.projection.session.next_id;
        let cue = Cue {
            start_ms: 100,
            end_ms: 900,
            text: "Plant a garden".into(),
        };
        let source = Event::SpeechSegment {
            recording: "recording-live".into(),
            segment: 0,
            cue: cue.clone(),
            thought_id: id,
        };
        let capture = Event::SessionChanged {
            change: SessionEvent::capture(&j.projection.session, &cue.text),
        };
        j.append(
            900,
            Event::Transaction {
                events: vec![capture, source.clone()],
            },
        )
        .unwrap();
        drop(j);
        let mut j = Journal::open(&path, Session::default()).unwrap();
        assert_eq!(
            j.projection.speech_segments["recording-live"][&0].thought_id,
            id
        );
        assert_eq!(j.projection.session.thoughts.len(), 1);
        j.projection.apply(&source).unwrap();
        let conflict = Event::SpeechSegment {
            recording: "recording-live".into(),
            segment: 0,
            cue: Cue {
                text: "Different".into(),
                ..cue
            },
            thought_id: id,
        };
        assert!(j.projection.apply(&conflict).is_err());
    }

    #[test]
    fn invalid_timings_and_path_traversal_are_rejected() {
        assert!(
            validate_cues(
                &[Cue {
                    start_ms: 10,
                    end_ms: 9,
                    text: "no".into()
                }],
                100
            )
            .is_err()
        );
        assert!(safe_asset("../audio.wav").is_err());
        assert_eq!(timestamp(3_600_123), "01:00:00.123");
        let mut p = Projection::default();
        assert!(
            p.apply(&Event::Transcript {
                recording: "missing".into(),
                cues: vec![],
                text: String::new()
            })
            .is_err()
        );
    }
}
