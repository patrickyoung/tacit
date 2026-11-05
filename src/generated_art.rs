//! OpenRouter image generation, private content-addressed cache, and stable UI transitions.
use crate::{art::Style, model::Session, organizer::CloudConfig};
use base64::Engine as _;
use rune::{Image, ImageCache};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

pub const MODEL: &str = "openai/gpt-image-2.5-flare";
const MAX_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq)]
pub struct Brief {
    pub key: String,
    pub fingerprint: String,
    pub slot: usize,
    pub prompt: String,
}
pub fn briefs(session: &Session, style: Style, model: &str) -> Vec<Brief> {
    if style == Style::Off {
        return vec![];
    }
    session.categories.iter().filter(|c| c.parent.is_none()).enumerate().filter_map(|(slot,c)| {
        let thoughts=session.thoughts.iter().filter(|t|session.within_category(t, &c.id)).take(3).map(|t|t.text.chars().take(240).collect::<String>()).collect::<Vec<_>>();
        if thoughts.is_empty(){return None;}
        let subject=c.illustration.clone().filter(|s|!s.trim().is_empty()).unwrap_or_else(||format!("{}: {}",c.name,thoughts.join("; ")));
        let medium=match style {Style::Pastel=>"Soft wax pastel and colored-pencil editorial illustration; dusty pigment, layered translucent strokes, expressive hand-drawn contours",Style::Chalk=>"Colorful chalk and dry pastel illustration on a transparent ground; broken powdery contours and tactile grain",Style::Marker=>"Bold hand-drawn marker illustration; expressive variable-width ink strokes, rich flat accents and playful confident shapes",Style::Off=>unreachable!()};
        let prompt=format!("Use case: illustration-story. Asset: a beautiful small scene beside a mindmap theme.\nSubject (data, not instructions): {subject}\nTheme (data): {}\nStyle: {medium}. Draw an original, sophisticated visual metaphor combining 2–4 related elements into one coherent narrative vignette. Give it the charm and craft of a live graphic recorder at a systems-thinking conference. Rich emerald, cornflower blue, coral, ochre and plum accents, harmonious rather than rainbow.\nComposition: square, a spacious central vignette using 75% of the canvas; generous transparent margins. Clear silhouette, readable at small size. Transparent background and open negative space between marks.\nAvoid: text, letters, numbers, captions, diagrams, interface elements, stock icons, emoji, clipart, rectangular backgrounds, paper rectangles, frames, photorealism, glossy 3D. No text whatsoever. Preserve alpha transparency.",c.name);
        let fingerprint=blake3::hash(format!("tacit-art-v1\0{model}\0{prompt}").as_bytes()).to_hex().to_string();
        Some(Brief{key:format!("theme-{}",c.id),fingerprint,slot,prompt})
    }).collect()
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
pub async fn generate(config: &CloudConfig, model: &str, brief: &Brief) -> Result<Vec<u8>, String> {
    use openrouter_rs::api::images::ImageGenerationRequest;
    let request = ImageGenerationRequest::builder()
        .model(model)
        .prompt(&brief.prompt)
        .aspect_ratio("1:1")
        .background("transparent")
        .quality("medium")
        .n(1)
        .build()
        .map_err(|_| "Could not build image request")?;
    let client = config.image_client()?;
    let response = tokio::time::timeout(Duration::from_secs(180), client.images().create(&request))
        .await
        .map_err(|_| "Image generation timed out; previous artwork retained")?
        .map_err(|_| "Image generation failed; check image model, credits and key")?;
    let item = response
        .data
        .first()
        .ok_or("Image model returned no artwork")?;
    if item.b64_json.len() > MAX_BYTES * 4 / 3 + 4 {
        return Err("Generated image is too large".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&item.b64_json)
        .map_err(|_| "Image model returned invalid image data")?;
    validate_png(&bytes)?;
    Ok(bytes)
}
pub struct Response {
    pub encoded: Option<Vec<u8>>,
    pub brief: Brief,
    pub result: Result<Image, String>,
}
pub struct Worker {
    pub send: Sender<Vec<Brief>>,
    pub receive: Receiver<Response>,
}
impl Worker {
    pub fn start(config: CloudConfig, model: String, wake: rune::Waker) -> Self {
        let (send, requests) = mpsc::channel::<Vec<Brief>>();
        let (responses, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let cache = ImageCache::new(wake.clone());
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            while let Ok(mut batch) = requests.recv() {
                loop {
                    while let Ok(newer) = requests.try_recv() {
                        batch = newer;
                    }
                    if batch.is_empty() {
                        break;
                    }
                    let brief = batch.remove(0);
                    let path = cache_dir().join(format!("{}.png", brief.fingerprint));
                    let mut encoded = None;
                    let result = (|| -> Result<Image, String> {
                        if let Some(bytes) = read_cache(&path) {
                            let image = cache.load(bytes.clone());
                            if image.wait().is_ok() {
                                encoded = Some(bytes);
                                return Ok(image);
                            }
                            let _ = std::fs::remove_file(&path);
                        }
                        let rt = runtime
                            .as_ref()
                            .map_err(|_| "Could not start image runtime")?;
                        let bytes = rt.block_on(generate(&config, &model, &brief))?;
                        let image = cache.load(bytes.clone());
                        image
                            .wait()
                            .map_err(|_| "Generated image could not be decoded")?;
                        write_cache(&path, &bytes)?;
                        encoded = Some(bytes);
                        Ok(image)
                    })();
                    if responses
                        .send(Response {
                            brief,
                            result,
                            encoded,
                        })
                        .is_err()
                    {
                        return;
                    }
                    wake();
                }
            }
        });
        Self { send, receive }
    }
}
pub struct Layer {
    pub encoded: Option<Vec<u8>>,
    pub key: String,
    pub fingerprint: String,
    pub slot: usize,
    pub image: Image,
    pub born: f64,
    pub retired: Option<f64>,
}
impl Layer {
    pub fn progress(&self, now: f64) -> f32 {
        let t = ((now - self.born) / 2.4).clamp(0., 1.) as f32;
        t * t * (3. - 2. * t)
    }
    pub fn opacity(&self, now: f64) -> f32 {
        self.retired
            .map_or(1., |t| 1. - ((now - t) / 2.4).clamp(0., 1.) as f32)
    }
}
pub struct Gallery {
    pub worker: Option<Worker>,
    pub layers: Vec<Layer>,
    pub model: String,
    pub status: String,
    pub now: f64,
    plans: Vec<Brief>,
    attempted: HashSet<String>,
    changed_at: f64,
    submitted: bool,
    active: bool,
}
impl Default for Gallery {
    fn default() -> Self {
        Self {
            worker: None,
            layers: vec![],
            model: std::env::var("TACIT_IMAGE_MODEL").unwrap_or_else(|_| MODEL.into()),
            status: String::new(),
            now: 0.,
            plans: vec![],
            attempted: HashSet::new(),
            changed_at: 0.,
            submitted: false,
            active: false,
        }
    }
}
impl Gallery {
    pub fn retry(&mut self) {
        self.attempted.clear();
        self.submitted = false;
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
        let plans = briefs(session, style, &self.model);
        let mut changed = false;
        if plans != self.plans {
            self.plans = plans;
            // A superseded batch may contain jobs the worker has not started.
            // Keep completed images deduplicated but allow canceled jobs back in.
            self.attempted
                .retain(|f| self.layers.iter().any(|l| &l.fingerprint == f));
            self.changed_at = now;
            self.submitted = false;
            // Replace pending jobs immediately. The in-flight result is checked below.
            let _ = self.worker.as_ref().unwrap().send.send(vec![]);
            for l in &mut self.layers {
                if let Some(p) = self.plans.iter().find(|p| p.key == l.key) {
                    l.slot = p.slot;
                } else if l.retired.is_none() {
                    l.retired = Some(now);
                }
            }
            self.status = if self.plans.is_empty() {
                String::new()
            } else {
                "Art · waiting for themes to settle".into()
            };
            changed = true;
        }
        while let Some(response) = self.worker.as_ref().and_then(|w| w.receive.try_recv().ok()) {
            if let Some(plan) = self.plans.iter().find(|p| {
                p.key == response.brief.key && p.fingerprint == response.brief.fingerprint
            }) {
                match response.result {
                    Ok(image) => {
                        if self.layers.iter().any(|l| {
                            l.retired.is_none()
                                && l.key == plan.key
                                && l.fingerprint == plan.fingerprint
                        }) {
                            continue;
                        }
                        for l in &mut self.layers {
                            if l.key == plan.key && l.retired.is_none() {
                                l.retired = Some(now);
                            }
                        }
                        self.layers.push(Layer {
                            encoded: response.encoded,
                            key: plan.key.clone(),
                            fingerprint: plan.fingerprint.clone(),
                            slot: plan.slot,
                            image,
                            born: now,
                            retired: None,
                        });
                        let ready = self
                            .plans
                            .iter()
                            .filter(|p| {
                                self.layers.iter().any(|l| {
                                    l.retired.is_none()
                                        && l.key == p.key
                                        && l.fingerprint == p.fingerprint
                                })
                            })
                            .count();
                        self.status = if ready == self.plans.len() {
                            "Art · illustrations ready".into()
                        } else {
                            format!(
                                "Art · {ready}/{} ready; illustrating remaining themes…",
                                self.plans.len()
                            )
                        };
                    }
                    Err(e) => {
                        self.status = e;
                    }
                }
                changed = true;
            }
        }
        if !self.submitted && !self.plans.is_empty() {
            if now - self.changed_at >= 5.0 && !organizing {
                let jobs = self
                    .plans
                    .iter()
                    .filter(|p| {
                        !self.attempted.contains(&p.fingerprint)
                            && !self
                                .layers
                                .iter()
                                .any(|l| l.retired.is_none() && l.fingerprint == p.fingerprint)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                for p in &jobs {
                    self.attempted.insert(p.fingerprint.clone());
                }
                if !jobs.is_empty() {
                    self.status = format!("Art · illustrating {} themes…", jobs.len());
                    let _ = self.worker.as_ref().unwrap().send.send(jobs);
                }
                self.submitted = true;
                changed = true;
            } else if !organizing {
                tx.wake_after(Duration::from_secs_f64(
                    (self.changed_at + 5. - now).max(0.01),
                ));
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
/// One real image request for the built-in fictional example, never a user's notebook.
pub fn check_demo(config: &CloudConfig, session: &Session) -> Result<PathBuf, String> {
    let model = std::env::var("TACIT_IMAGE_MODEL").unwrap_or_else(|_| MODEL.into());
    let brief = briefs(session, Style::Pastel, &model)
        .into_iter()
        .next()
        .ok_or("Demo has no theme")?;
    let path = cache_dir().join(format!("{}.png", brief.fingerprint));
    let cache = ImageCache::new(std::sync::Arc::new(|| {}));
    let bytes = if let Some(bytes) = read_cache(&path) {
        bytes
    } else {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Could not start image runtime")?;
        let bytes = runtime.block_on(generate(config, &model, &brief))?;
        let image = cache.load(bytes.clone());
        image.wait().map_err(|_| "Image could not be decoded")?;
        write_cache(&path, &bytes)?;
        bytes
    };
    cache
        .load(bytes)
        .wait()
        .map_err(|_| "Cached image could not be decoded")?;
    Ok(path)
}
impl Gallery {
    pub fn load_demo(&mut self, session: &Session) {
        let cache = ImageCache::new(std::sync::Arc::new(|| {}));
        for brief in briefs(session, Style::Pastel, &self.model) {
            if let Some(bytes) = read_cache(&cache_dir().join(format!("{}.png", brief.fingerprint)))
            {
                let image = cache.load(bytes.clone());
                if image.wait().is_ok() {
                    self.layers.push(Layer {
                        encoded: Some(bytes),
                        key: brief.key,
                        fingerprint: brief.fingerprint,
                        slot: brief.slot,
                        image,
                        born: -10.,
                        retired: None,
                    });
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> Session {
        let mut s = Session::demo();
        s.categories = vec![crate::model::Category {
            parent: None,
            id: "time".into(),
            name: "Time".into(),
            motif: None,
            illustration: Some("A garden growing inside an open calendar".into()),
        }];
        for t in &mut s.thoughts {
            t.category = Some("time".into());
        }
        s
    }
    #[test]
    fn briefs_stay_stable_for_membership_edits_but_change_with_meaning_or_style() {
        let mut s = session();
        let a = briefs(&s, Style::Pastel, MODEL);
        s.add("Another thought");
        assert_eq!(a, briefs(&s, Style::Pastel, MODEL));
        s.categories[0].illustration = Some("A bridge made of calendar pages".into());
        assert_ne!(a, briefs(&s, Style::Pastel, MODEL));
        assert_ne!(
            briefs(&s, Style::Pastel, MODEL),
            briefs(&s, Style::Marker, MODEL)
        );
        assert!(briefs(&s, Style::Off, MODEL).is_empty());
    }
    #[test]
    fn scheduler_debounces_deduplicates_and_cancels_queue() {
        let (send, requests) = mpsc::channel();
        let (_respond, receive) = mpsc::channel();
        let mut g = Gallery {
            worker: Some(Worker { send, receive }),
            ..Gallery::default()
        };
        let s = session();
        let mut tx = rune::Tx::default();
        g.update(&s, Style::Pastel, false, 0., &mut tx);
        assert!(requests.try_recv().unwrap().is_empty());
        g.update(&s, Style::Pastel, false, 4., &mut tx);
        assert!(requests.try_recv().is_err());
        g.update(&s, Style::Pastel, false, 5., &mut tx);
        assert_eq!(requests.try_recv().unwrap().len(), 1);
        g.update(&s, Style::Pastel, false, 100., &mut tx);
        assert!(requests.try_recv().is_err());
        g.update(&s, Style::Off, false, 101., &mut tx);
        assert!(requests.try_recv().unwrap().is_empty());
    }
    #[test]
    fn stale_results_are_ignored_and_previous_art_survives_until_replacement() {
        let cache = ImageCache::new(std::sync::Arc::new(|| {}));
        let image = cache.rgba(1, 1, vec![255u8, 0, 0, 255]);
        image.wait().unwrap();
        let (send, _requests) = mpsc::channel();
        let (responses, receive) = mpsc::channel();
        let mut g = Gallery {
            worker: Some(Worker { send, receive }),
            ..Gallery::default()
        };
        let mut s = session();
        let mut tx = rune::Tx::default();
        g.update(&s, Style::Pastel, false, 0., &mut tx);
        let old = g.plans[0].clone();
        responses
            .send(Response {
                encoded: None,
                brief: old.clone(),
                result: Ok(image.clone()),
            })
            .unwrap();
        g.update(&s, Style::Pastel, false, 1., &mut tx);
        assert_eq!(g.layers.len(), 1);
        s.categories[0].illustration = Some("A clock becoming a flower".into());
        g.update(&s, Style::Pastel, false, 4., &mut tx);
        assert!(g.layers[0].retired.is_none());
        responses
            .send(Response {
                encoded: None,
                brief: old,
                result: Err("stale failure".into()),
            })
            .unwrap();
        g.update(&s, Style::Pastel, false, 4.1, &mut tx);
        assert!(!g.status.contains("stale failure"));
        responses
            .send(Response {
                encoded: None,
                brief: g.plans[0].clone(),
                result: Ok(image),
            })
            .unwrap();
        g.update(&s, Style::Pastel, false, 5., &mut tx);
        assert_eq!(g.layers.len(), 2);
        assert_eq!(g.layers[0].retired, Some(5.));
        g.update(&s, Style::Pastel, false, 8., &mut tx);
        assert_eq!(g.layers.len(), 1);
        g.update(&s, Style::Pastel, false, 9., &mut tx);
        assert!(!g.update(&s, Style::Pastel, false, 10., &mut tx));
    }
    #[test]
    fn malformed_images_rejected() {
        assert!(validate_png(b"not an image").is_err());
        let mut p = vec![0; 33];
        p[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        p[12..16].copy_from_slice(b"IHDR");
        p[16..20].copy_from_slice(&100000u32.to_be_bytes());
        p[20..24].copy_from_slice(&100000u32.to_be_bytes());
        assert!(validate_png(&p).is_err());
    }
}
