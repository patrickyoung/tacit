//! One evolving mural. Art direction and image editing run off the UI thread.
use crate::{
    art::Style,
    artist::{self, Direction, Evidence, Mural},
    model::Session,
    organizer::CloudConfig,
};
use base64::Engine as _;
use rune::{Image, ImageCache};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};

pub const MODEL: &str = "openai/gpt-image-2.5-flare";
/// Kept stable for archived artwork and other renderers.
pub const CENTRAL_KEY: &str = "map-synthesis";
const IMAGE_SIZE: &str = "1536x1024";
const MAX_BYTES: usize = 16 * 1024 * 1024;
const INITIAL_DELAY: f64 = 8.0;
const UPDATE_INTERVAL: f64 = 30.0;

#[derive(Clone, Debug)]
pub struct Brief {
    pub fingerprint: String,
    pub prompt: String,
    pub model: String,
    pub previous: Option<Arc<Vec<u8>>>,
}
impl Brief {
    pub fn new(direction: &Direction, model: &str, previous: Option<Arc<Vec<u8>>>) -> Self {
        let operation = if previous.is_some() {
            "Edit the supplied mural IN PLACE using the art direction's change instructions. It is the actual previous artwork, not a loose style reference. Preserve the specified continuity anchors and unchanged areas. When the direction calls for re-staging an irrelevant older setting, replace its setting, props and actions as instructed while retaining useful character, perspective and composition continuity. Narrative accuracy takes priority over preserving obsolete scenery. Return the complete updated mural with alpha transparency."
        } else {
            "Draw the first scene of a mural that will evolve with a conversation. Make the specific story and visible interactions in the art direction immediately recognizable, with room for the scene to develop."
        };
        let direction = serde_json::to_string(direction).expect("serializable direction");
        let prompt = format!(
            "{operation}\nPurpose: {}\nArt direction: {direction}\nThe story and connections explain what the actions must convey; do not render them as text or separate illustrations.\nComposition: {}\nVisual language: {}",
            artist::NARRATIVE,
            artist::COMPOSITION,
            artist::STYLE,
        );
        let parent = previous
            .as_ref()
            .map(|b| blake3::hash(b).to_hex().to_string());
        let fingerprint = blake3::hash(
            format!(
                "tacit-mural-image-v1\0{model}\0low\0{IMAGE_SIZE}\0transparent\0png\0{}\0{prompt}",
                parent.as_deref().unwrap_or("initial")
            )
            .as_bytes(),
        )
        .to_hex()
        .to_string();
        Self {
            fingerprint,
            prompt,
            model: model.into(),
            previous,
        }
    }
}
fn data_url(bytes: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}
fn image_request(
    brief: &Brief,
) -> Result<openrouter_rs::api::images::ImageGenerationRequest, String> {
    use openrouter_rs::api::images::{ImageGenerationRequest, ImageInputReference};
    let mut builder = ImageGenerationRequest::builder();
    builder
        .model(&brief.model)
        .prompt(&brief.prompt)
        .aspect_ratio("3:2")
        .size(IMAGE_SIZE)
        .background("transparent")
        .output_format("png")
        .quality("low")
        .n(1);
    if let Some(bytes) = &brief.previous {
        validate_png(bytes)?;
        builder.input_references(vec![ImageInputReference::image_url(data_url(bytes))]);
    }
    builder
        .build()
        .map_err(|_| "Could not build mural image request".into())
}
pub async fn generate(config: &CloudConfig, brief: &Brief) -> Result<Vec<u8>, String> {
    let request = image_request(brief)?;
    let client = config.image_client()?;
    let response = tokio::time::timeout(Duration::from_secs(180), client.images().create(&request))
        .await
        .map_err(|_| "Mural generation timed out; previous artwork retained")?
        .map_err(|_| "Mural generation failed; check image model, credits and key")?;
    let item = response
        .data
        .first()
        .ok_or("Image model returned no artwork")?;
    if item.b64_json.len() > MAX_BYTES * 4 / 3 + 4 {
        return Err("Generated mural is too large".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&item.b64_json)
        .map_err(|_| "Image model returned invalid image data")?;
    validate_png(&bytes)?;
    Ok(bytes)
}

#[derive(Clone)]
pub struct Base {
    pub fingerprint: String,
    pub mural: Mural,
    pub encoded: Arc<Vec<u8>>,
}
#[derive(Clone)]
pub struct Request {
    pub cancelled: Arc<AtomicBool>,
    pub id: u64,
    pub evidence: Evidence,
    pub model: String,
    pub artist_model: String,
    pub base: Option<Base>,
}
pub struct Artifact {
    pub fingerprint: String,
    /// None means the artist elected to keep the existing pixels.
    pub encoded: Option<Arc<Vec<u8>>>,
    pub mural: Mural,
}
pub async fn paint(config: &CloudConfig, request: &Request) -> Result<Artifact, String> {
    let check_cancelled = || {
        if request.cancelled.load(Ordering::Acquire) {
            Err("Mural update was superseded".to_string())
        } else {
            Ok(())
        }
    };
    check_cancelled()?;
    let previous = request.base.as_ref();
    let url = previous.map(|b| data_url(&b.encoded));
    let decision = artist::direct(
        config,
        &request.artist_model,
        &request.evidence,
        previous.map(|b| &b.mural),
        url.as_deref(),
    )
    .await?;
    // Do not start an image call after art was disabled, replay began, or the
    // evidence was corrected while the artist was thinking.
    check_cancelled()?;
    if !decision.repaint
        && let Some(base) = previous
        && base.mural.image_model == request.model
        && base.mural.artist_model == request.artist_model
    {
        let mut mural = base.mural.clone();
        mural.evidence = request.evidence.fingerprint.clone();
        mural.direction = decision.direction;
        return Ok(Artifact {
            fingerprint: base.fingerprint.clone(),
            encoded: None,
            mural,
        });
    }
    let brief = Brief::new(
        &decision.direction,
        &request.model,
        previous.map(|b| b.encoded.clone()),
    );
    let path = cache_dir().join(format!("{}.png", brief.fingerprint));
    let bytes = if let Some(bytes) = read_cache(&path) {
        bytes
    } else {
        let bytes = generate(config, &brief).await?;
        write_cache(&path, &bytes)?;
        bytes
    };
    Ok(Artifact {
        fingerprint: brief.fingerprint,
        encoded: Some(Arc::new(bytes)),
        mural: Mural {
            direction: decision.direction,
            evidence: request.evidence.fingerprint.clone(),
            artist_model: request.artist_model.clone(),
            image_model: request.model.clone(),
            parent: previous.map(|b| b.fingerprint.clone()),
        },
    })
}
pub struct Update {
    pub artifact: Artifact,
    pub image: Option<Image>,
}
pub struct Response {
    pub id: u64,
    pub result: Result<Update, String>,
}
pub struct Worker {
    pub send: Sender<Option<Request>>,
    pub receive: Receiver<Response>,
}
impl Worker {
    pub fn start(config: CloudConfig, wake: rune::Waker) -> Self {
        let (send, requests) = mpsc::channel::<Option<Request>>();
        let (responses, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let cache = ImageCache::new(wake.clone());
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            while let Ok(mut latest) = requests.recv() {
                while let Ok(newer) = requests.try_recv() {
                    latest = newer;
                }
                let Some(request) = latest else {
                    continue;
                };
                let result = (|| {
                    let rt = runtime
                        .as_ref()
                        .map_err(|_| "Could not start mural runtime")?;
                    let artifact = rt.block_on(paint(&config, &request))?;
                    let image = if let Some(bytes) = &artifact.encoded {
                        let image = cache.load(bytes.as_ref().clone());
                        image
                            .wait()
                            .map_err(|_| "Mural could not be decoded; previous artwork retained")?;
                        Some(image)
                    } else {
                        None
                    };
                    Ok(Update { artifact, image })
                })();
                if responses
                    .send(Response {
                        id: request.id,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
                wake();
            }
        });
        Self { send, receive }
    }
}
pub struct Layer {
    pub encoded: Option<Arc<Vec<u8>>>,
    pub key: String,
    pub fingerprint: String,
    pub slot: usize,
    pub image: Image,
    pub born: f64,
    pub retired: Option<f64>,
    pub mural: Option<Mural>,
}
impl Layer {
    fn transition_seconds(&self) -> f64 {
        if self.key == CENTRAL_KEY { 6.0 } else { 2.4 }
    }
    pub fn progress(&self, now: f64) -> f32 {
        let t = ((now - self.born) / self.transition_seconds()).clamp(0., 1.) as f32;
        t * t * (3. - 2. * t)
    }
    pub fn opacity(&self, now: f64) -> f32 {
        self.retired.map_or(1., |t| {
            1. - ((now - t) / self.transition_seconds()).clamp(0., 1.) as f32
        })
    }
}
pub struct Gallery {
    pub worker: Option<Worker>,
    pub layers: Vec<Layer>,
    pub model: String,
    pub artist_model: String,
    pub status: String,
    pub now: f64,
    /// Historical scene replay can still use the original radial composition.
    pub mural_layout: bool,
    planned_for: Option<(Session, Style, String, String)>,
    input: Option<Evidence>,
    pending: Option<Request>,
    attempted: Option<(String, String, String)>,
    due: Option<f64>,
    last_request: Option<f64>,
    serial: u64,
    active: bool,
}
impl Default for Gallery {
    fn default() -> Self {
        Self {
            worker: None,
            layers: vec![],
            model: std::env::var("TACIT_IMAGE_MODEL").unwrap_or_else(|_| MODEL.into()),
            artist_model: std::env::var("TACIT_ARTIST_MODEL")
                .unwrap_or_else(|_| artist::MODEL.into()),
            status: String::new(),
            now: 0.,
            mural_layout: true,
            planned_for: None,
            input: None,
            pending: None,
            attempted: None,
            due: None,
            last_request: None,
            serial: 0,
            active: false,
        }
    }
}
impl Gallery {
    pub fn retry(&mut self) {
        self.attempted = None;
        self.due = Some(self.now);
    }
    /// Replay/off invalidate publication, including an already-running request.
    pub fn suspend(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancelled.store(true, Ordering::Release);
        }
        if let Some(worker) = &self.worker {
            let _ = worker.send.send(None);
        }
        self.attempted = None;
        self.due = None;
        self.planned_for = None;
    }
    fn current(&self) -> Option<&Layer> {
        self.layers
            .iter()
            .rev()
            .find(|l| l.key == CENTRAL_KEY && l.retired.is_none() && l.mural.is_some())
    }
    pub fn update(
        &mut self,
        session: &Session,
        style: Style,
        organizing: bool,
        now: f64,
        tx: &mut rune::Tx,
    ) -> bool {
        self.now = now;
        if self.worker.is_none() {
            return false;
        }
        let replan =
            self.planned_for
                .as_ref()
                .is_none_or(|(previous, old_style, model, artist)| {
                    previous != session
                        || *old_style != style
                        || model != &self.model
                        || artist != &self.artist_model
                });
        let mut changed = false;
        if replan {
            self.input = if style == Style::Off {
                None
            } else {
                Evidence::from_session(session)
            };
            if self.input.is_none()
                || self.pending.as_ref().is_some_and(|p| {
                    p.model != self.model
                        || p.artist_model != self.artist_model
                        || !self
                            .input
                            .as_ref()
                            .is_some_and(|i| p.evidence.still_supported_by(i))
                })
            {
                self.suspend();
            }
            self.planned_for = Some((
                session.clone(),
                style,
                self.model.clone(),
                self.artist_model.clone(),
            ));
            // Old topic images remain available in historical playback only.
            for layer in &mut self.layers {
                if layer.retired.is_none() && (layer.mural.is_none() || session.thoughts.is_empty())
                {
                    layer.retired = Some(now);
                }
            }
            if self.input.is_none() {
                self.status.clear();
            }
            changed = true;
        }
        while let Some(response) = self.worker.as_ref().and_then(|w| w.receive.try_recv().ok()) {
            if !self.pending.as_ref().is_some_and(|p| p.id == response.id) {
                continue;
            }
            let request = self.pending.take().unwrap();
            let same_parent = self.current().map(|l| &l.fingerprint)
                == request.base.as_ref().map(|b| &b.fingerprint);
            if !same_parent
                || request.model != self.model
                || request.artist_model != self.artist_model
                || !self
                    .input
                    .as_ref()
                    .is_some_and(|i| request.evidence.still_supported_by(i))
            {
                continue;
            }
            match response.result {
                Ok(Update {
                    artifact,
                    image: Some(image),
                }) => {
                    for l in &mut self.layers {
                        if l.retired.is_none() {
                            l.retired = Some(now);
                        }
                    }
                    self.layers.push(Layer {
                        encoded: artifact.encoded,
                        key: CENTRAL_KEY.into(),
                        fingerprint: artifact.fingerprint,
                        slot: 0,
                        image,
                        born: now,
                        retired: None,
                        mural: Some(artifact.mural),
                    });
                    self.status = "Art · mural evolving with the conversation".into();
                }
                Ok(Update {
                    artifact,
                    image: None,
                }) => {
                    if let Some(layer) = self
                        .layers
                        .iter_mut()
                        .rev()
                        .find(|l| l.retired.is_none() && l.fingerprint == artifact.fingerprint)
                    {
                        layer.mural = Some(artifact.mural);
                    }
                    self.status = "Art · mural up to date".into();
                }
                Err(error) => self.status = error,
            }
            changed = true;
        }
        if let Some(input) = &self.input {
            let attempted = self
                .attempted
                .as_ref()
                .is_some_and(|(evidence, model, artist)| {
                    evidence == &input.fingerprint
                        && model == &self.model
                        && artist == &self.artist_model
                });
            let done = self
                .current()
                .and_then(|l| l.mural.as_ref())
                .is_some_and(|m| {
                    m.evidence == input.fingerprint
                        && m.image_model == self.model
                        && m.artist_model == self.artist_model
                });
            if done || attempted {
                self.due = None;
            } else if self.pending.is_none() {
                let due = *self.due.get_or_insert_with(|| {
                    (now + INITIAL_DELAY)
                        .max(self.last_request.map_or(now, |t| t + UPDATE_INTERVAL))
                });
                if !organizing && now >= due {
                    let base = self
                        .current()
                        .map(|l| {
                            l.encoded
                                .clone()
                                .map(|encoded| Base {
                                    encoded,
                                    fingerprint: l.fingerprint.clone(),
                                    mural: l.mural.clone().unwrap(),
                                })
                                .ok_or("Previous mural pixels unavailable; artwork retained")
                        })
                        .transpose();
                    self.attempted = Some((
                        input.fingerprint.clone(),
                        self.model.clone(),
                        self.artist_model.clone(),
                    ));
                    self.due = None;
                    match base {
                        Ok(base) => {
                            self.serial += 1;
                            let request = Request {
                                cancelled: Arc::new(AtomicBool::new(false)),
                                id: self.serial,
                                evidence: input.clone(),
                                model: self.model.clone(),
                                artist_model: self.artist_model.clone(),
                                base,
                            };
                            if self
                                .worker
                                .as_ref()
                                .unwrap()
                                .send
                                .send(Some(request.clone()))
                                .is_ok()
                            {
                                self.pending = Some(request);
                                self.last_request = Some(now);
                                self.status = "Art · composing the next scene…".into();
                            } else {
                                self.status = "Mural worker unavailable".into();
                            }
                        }
                        Err(error) => self.status = error.into(),
                    }
                    changed = true;
                } else if !organizing {
                    tx.wake_after(Duration::from_secs_f64((due - now).max(0.01)));
                }
            }
        }
        let animating = self
            .layers
            .iter()
            .any(|l| l.progress(now) < 1. || l.retired.is_some());
        changed |= animating || self.active;
        self.active = animating;
        self.layers.retain(|l| l.opacity(now) > 0.);
        if animating {
            tx.wake_after(Duration::from_millis(33));
        }
        changed
    }
    pub fn coverage(&self, key: &str) -> f32 {
        self.layers
            .iter()
            .filter(|l| l.key == key)
            .map(|l| l.progress(self.now) * l.opacity(self.now))
            .fold(0., f32::max)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Demo {
    fingerprint: String,
    mural: Mural,
}
/// Live diagnostic uses only the built-in fictional example. No user's notebook.
pub fn check_demo(config: &CloudConfig, session: &Session) -> Result<PathBuf, String> {
    let gallery = Gallery::default();
    let request = Request {
        cancelled: Arc::new(AtomicBool::new(false)),
        id: 0,
        evidence: Evidence::from_session(session).ok_or("Empty demo")?,
        model: gallery.model,
        artist_model: gallery.artist_model,
        base: None,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Could not start mural runtime")?;
    let artifact = runtime.block_on(paint(config, &request))?;
    let demo = Demo {
        fingerprint: artifact.fingerprint,
        mural: artifact.mural,
    };
    // This manifest contains only the public fictional fixture, never private prompts.
    write_cache(
        &cache_dir().join("demo-mural.json"),
        &serde_json::to_vec(&demo).map_err(|_| "Could not save demo")?,
    )?;
    Ok(cache_dir().join(format!("{}.png", demo.fingerprint)))
}
impl Gallery {
    pub fn load_demo(&mut self, session: &Session) {
        let Some(input) = Evidence::from_session(session) else {
            return;
        };
        let demo = std::fs::read(cache_dir().join("demo-mural.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Demo>(&b).ok());
        let Some(demo) = demo.filter(|d| {
            d.mural.evidence == input.fingerprint
                && d.mural.image_model == self.model
                && d.mural.artist_model == self.artist_model
        }) else {
            return;
        };
        if let Some(bytes) = read_cache(&cache_dir().join(format!("{}.png", demo.fingerprint))) {
            let cache = ImageCache::new(Arc::new(|| {}));
            let image = cache.load(bytes.clone());
            if image.wait().is_ok() {
                self.layers.push(Layer {
                    encoded: Some(Arc::new(bytes)),
                    key: CENTRAL_KEY.into(),
                    fingerprint: demo.fingerprint,
                    slot: 0,
                    image,
                    born: -10.,
                    retired: None,
                    mural: Some(demo.mural),
                });
            }
        }
    }
}
pub fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("tacit/art")
}
fn validate_png(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_BYTES
        || bytes.len() < 33
        || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
        || &bytes[12..16] != b"IHDR"
    {
        return Err("Image model did not return a supported PNG".into());
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Err("Generated image dimensions are out of range".into());
    }
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .map_err(|_| "Generated PNG could not be decoded")?;
    let size = reader
        .output_buffer_size()
        .filter(|s| *s <= 64 * 1024 * 1024)
        .ok_or("Decoded mural is too large")?;
    let mut pixels = vec![0; size];
    reader
        .next_frame(&mut pixels)
        .map_err(|_| "Generated PNG is incomplete or corrupt")?;
    Ok(())
}
fn read_cache(path: &Path) -> Option<Vec<u8>> {
    if std::fs::metadata(path).ok()?.len() > MAX_BYTES as u64 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    validate_png(&bytes).ok()?;
    Some(bytes)
}
fn write_cache(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let dir = path.parent().ok_or("Missing art cache directory")?;
    std::fs::create_dir_all(dir).map_err(|_| "Could not create art cache")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Could not secure art cache")?;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .map_err(|_| "Could not write art cache")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Could not save generated art")?;
    std::fs::rename(temp, path).map_err(|_| "Could not finish art cache write")?;
    // Bound cache growth; images already on screen retain their decoded handles.
    let mut files = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| {
            let p = e.path();
            if p.extension()?.to_str()? != "png" {
                return None;
            }
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), p))
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|f| f.0);
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    let count = files.len();
    for (i, (_, length, p)) in files.into_iter().enumerate() {
        if total <= 256 * 1024 * 1024 && count - i <= 128 {
            break;
        }
        if p != path && std::fs::remove_file(p).is_ok() {
            total = total.saturating_sub(length);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    fn bytes() -> Arc<Vec<u8>> {
        Arc::new(
            base64::engine::general_purpose::STANDARD
                .decode(PNG)
                .unwrap(),
        )
    }
    fn direction() -> Direction {
        Direction {
            story: "A team struggles to decide who owns a decision".into(),
            connections: vec![artist::VisualConnection {
                thought_ids: vec![2],
                depiction: "Colleagues hesitate over an unclaimed work item".into(),
            }],
            scene: "Colleagues pause over an unclaimed decision in a modern workspace".into(),
            anchors: vec!["The shared project display".into()],
            change: "Show the unresolved handoff, keeping the decision visibly pending".into(),
        }
    }
    fn setup() -> (Gallery, Receiver<Option<Request>>, Sender<Response>) {
        let (send, requests) = mpsc::channel();
        let (responses, receive) = mpsc::channel();
        let gallery = Gallery {
            worker: Some(Worker { send, receive }),
            ..Gallery::default()
        };
        (gallery, requests, responses)
    }
    fn tick(gallery: &mut Gallery, session: &Session, now: f64) {
        gallery.update(session, Style::Ink, false, now, &mut rune::Tx::default());
    }
    fn success(request: &Request, fingerprint: &str) -> Response {
        let cache = ImageCache::new(Arc::new(|| {}));
        let encoded = bytes();
        let image = cache.load(encoded.as_ref().clone());
        image.wait().unwrap();
        Response {
            id: request.id,
            result: Ok(Update {
                artifact: Artifact {
                    fingerprint: fingerprint.into(),
                    encoded: Some(encoded),
                    mural: Mural {
                        direction: direction(),
                        evidence: request.evidence.fingerprint.clone(),
                        artist_model: request.artist_model.clone(),
                        image_model: request.model.clone(),
                        parent: request.base.as_ref().map(|b| b.fingerprint.clone()),
                    },
                },
                image: Some(image),
            }),
        }
    }
    fn initial(g: &mut Gallery, s: &Session, q: &Receiver<Option<Request>>, r: &Sender<Response>) {
        tick(g, s, 0.);
        tick(g, s, 8.);
        let request = q.try_recv().unwrap().unwrap();
        assert!(request.base.is_none());
        r.send(success(&request, "first-scene")).unwrap();
        tick(g, s, 9.);
    }
    #[test]
    fn one_mural_without_themes_coalesces_and_deduplicates() {
        let (mut g, q, _) = setup();
        let mut s = Session::default();
        s.add("We need space to think");
        tick(&mut g, &s, 0.);
        s.add("And a garden to meet in");
        tick(&mut g, &s, 7.);
        assert!(q.try_recv().is_err());
        g.update(&s, Style::Ink, true, 8., &mut rune::Tx::default());
        assert!(q.try_recv().is_err());
        tick(&mut g, &s, 9.);
        let request = q.try_recv().unwrap().unwrap();
        assert_eq!(request.evidence, Evidence::from_session(&s).unwrap());
        tick(&mut g, &s, 100.);
        assert!(q.try_recv().is_err());
    }
    #[test]
    fn edits_use_actual_previous_pixels_and_direction() {
        let (mut g, q, r) = setup();
        let mut s = Session::demo();
        initial(&mut g, &s, &q, &r);
        tick(&mut g, &s, 15.);
        assert!(q.try_recv().is_err());
        s.add("A new garden");
        tick(&mut g, &s, 16.);
        tick(&mut g, &s, 37.);
        assert!(q.try_recv().is_err());
        tick(&mut g, &s, 38.);
        let request = q.try_recv().unwrap().unwrap();
        let base = request.base.as_ref().unwrap();
        assert_eq!(base.fingerprint, "first-scene");
        assert_eq!(base.encoded, bytes());
        assert_eq!(base.mural.direction, direction());
        r.send(success(&request, "second-scene")).unwrap();
        tick(&mut g, &s, 40.);
        assert_eq!(
            g.current()
                .unwrap()
                .mural
                .as_ref()
                .unwrap()
                .parent
                .as_deref(),
            Some("first-scene")
        );
        tick(&mut g, &s, 500.);
        assert_eq!(g.layers.len(), 1);
        assert!(q.try_recv().is_err());
    }
    #[test]
    fn arriving_messages_do_not_starve_the_mural() {
        let (mut g, q, r) = setup();
        let mut s = Session::demo();
        tick(&mut g, &s, 0.);
        tick(&mut g, &s, 8.);
        let first = q.try_recv().unwrap().unwrap();
        s.add("Another idea during generation");
        tick(&mut g, &s, 10.);
        r.send(success(&first, "first-scene")).unwrap();
        tick(&mut g, &s, 12.);
        assert!(g.current().is_some());
        tick(&mut g, &s, 38.);
        let next = q.try_recv().unwrap().unwrap();
        assert_eq!(next.evidence, Evidence::from_session(&s).unwrap());
        assert_eq!(next.base.unwrap().fingerprint, "first-scene");
    }
    #[test]
    fn corrections_and_deletions_reject_stale_results() {
        for removal in [false, true] {
            let (mut g, q, r) = setup();
            let mut s = Session::demo();
            initial(&mut g, &s, &q, &r);
            s.add("A greenhouse");
            tick(&mut g, &s, 16.);
            tick(&mut g, &s, 38.);
            let request = q.try_recv().unwrap().unwrap();
            if removal {
                s.thoughts.pop();
            } else {
                s.thoughts.last_mut().unwrap().text = "Actually, a mountain".into();
            }
            r.send(success(&request, "obsolete")).unwrap();
            tick(&mut g, &s, 40.);
            assert_eq!(g.current().unwrap().fingerprint, "first-scene");
        }
    }
    #[test]
    fn off_replay_and_model_changes_invalidate_pending_publication() {
        for mode in 0..3 {
            let (mut g, q, r) = setup();
            let s = Session::demo();
            tick(&mut g, &s, 0.);
            tick(&mut g, &s, 8.);
            let request = q.try_recv().unwrap().unwrap();
            match mode {
                0 => {
                    g.update(&s, Style::Off, false, 9., &mut rune::Tx::default());
                }
                1 => g.suspend(),
                _ => g.model = "different-model".into(),
            }
            r.send(success(&request, "obsolete")).unwrap();
            tick(&mut g, &s, 10.);
            assert!(request.cancelled.load(Ordering::Acquire));
            assert!(g.current().is_none());
        }
    }
    #[test]
    fn failure_keeps_pixels_and_does_not_loop_until_retry() {
        let (mut g, q, r) = setup();
        let mut s = Session::demo();
        initial(&mut g, &s, &q, &r);
        s.add("A new garden");
        tick(&mut g, &s, 16.);
        tick(&mut g, &s, 38.);
        let request = q.try_recv().unwrap().unwrap();
        r.send(Response {
            id: request.id,
            result: Err("Unavailable".into()),
        })
        .unwrap();
        tick(&mut g, &s, 40.);
        tick(&mut g, &s, 500.);
        assert_eq!(g.current().unwrap().fingerprint, "first-scene");
        assert!(q.try_recv().is_err());
        g.retry();
        tick(&mut g, &s, 501.);
        assert!(q.try_recv().unwrap().unwrap().base.is_some());
    }
    #[test]
    fn artist_can_acknowledge_a_change_without_repainting() {
        let (mut g, q, r) = setup();
        let mut s = Session::demo();
        initial(&mut g, &s, &q, &r);
        let born = g.current().unwrap().born;
        s.title = "A refined title".into();
        tick(&mut g, &s, 16.);
        tick(&mut g, &s, 38.);
        let request = q.try_recv().unwrap().unwrap();
        let mut mural = request.base.unwrap().mural;
        mural.evidence = request.evidence.fingerprint;
        r.send(Response {
            id: request.id,
            result: Ok(Update {
                image: None,
                artifact: Artifact {
                    fingerprint: "first-scene".into(),
                    encoded: None,
                    mural,
                },
            }),
        })
        .unwrap();
        tick(&mut g, &s, 40.);
        tick(&mut g, &s, 500.);
        assert_eq!(g.current().unwrap().born, born);
        assert!(q.try_recv().is_err());
    }
    #[test]
    fn missing_archived_pixels_never_silently_restarts_the_scene() {
        let (mut g, q, r) = setup();
        let mut s = Session::demo();
        initial(&mut g, &s, &q, &r);
        g.layers[0].encoded = None;
        s.add("A new garden");
        tick(&mut g, &s, 16.);
        tick(&mut g, &s, 38.);
        assert!(g.status.contains("pixels unavailable"));
        assert!(q.try_recv().is_err());
        assert_eq!(g.current().unwrap().fingerprint, "first-scene");
    }
    #[test]
    fn initial_and_edit_requests_use_low_quality_landscape_and_alpha() {
        for previous in [None, Some(bytes())] {
            let brief = Brief::new(&direction(), MODEL, previous.clone());
            let request = serde_json::to_value(image_request(&brief).unwrap()).unwrap();
            assert_eq!(request["model"], MODEL);
            assert_eq!(request["quality"], "low");
            assert_eq!(request["background"], "transparent");
            assert_eq!(request["size"], IMAGE_SIZE);
            assert_eq!(request["aspect_ratio"], "3:2");
            assert_eq!(request["output_format"], "png");
            if let Some(bytes) = previous {
                assert_eq!(
                    request["input_references"][0]["image_url"]["url"],
                    data_url(&bytes)
                );
                assert!(brief.prompt.contains("IN PLACE"));
            } else {
                assert!(request.get("input_references").is_none());
            }
        }
    }
    #[test]
    fn cache_identity_tracks_parent_pixels_and_output_model() {
        let a = Brief::new(&direction(), MODEL, None);
        let b = Brief::new(&direction(), MODEL, Some(bytes()));
        assert_ne!(a.fingerprint, b.fingerprint);
        assert_ne!(
            a.fingerprint,
            Brief::new(&direction(), "other", None).fingerprint
        );
        let mut different = bytes().as_ref().clone();
        different.push(0);
        assert_ne!(
            b.fingerprint,
            Brief::new(&direction(), MODEL, Some(Arc::new(different))).fingerprint
        );
        assert!(validate_png(b"not a PNG").is_err());
        assert!(validate_png(&bytes()).is_ok());
    }
}
