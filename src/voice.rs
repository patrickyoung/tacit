//! Own the audio file first; Hex transcribes those exact samples locally.
use crate::timeline::{self, Cue};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Check,
    Start,
    Finish,
    Cancel,
    Retry(PathBuf),
}
#[derive(Clone, Debug)]
pub struct Recording {
    pub id: String,
    pub path: PathBuf,
    pub started: std::time::Instant,
    pub samples: u64,
    pub sample_rate: u32,
}
impl Recording {
    pub fn duration_ms(&self) -> u64 {
        self.samples * 1000 / self.sample_rate as u64
    }
}
#[derive(Debug)]
pub enum Event {
    Ready,
    Recording,
    Started {
        id: String,
        started: std::time::Instant,
    },
    AudioSaved(Recording),
    Transcript(String),
    TimedTranscript {
        id: String,
        text: String,
        cues: Vec<Cue>,
    },
    Cancelled,
    Error(String),
}
pub struct Worker {
    pub send: Sender<Command>,
    pub receive: Receiver<Event>,
}
impl Worker {
    pub fn start(root: PathBuf, wake: impl Fn() + Send + 'static) -> Self {
        let (send, commands) = mpsc::channel();
        let (events, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let mut recorder: Option<(rune::Recorder, String)> = None;
            let mut announced = false;
            let mut started = std::time::Instant::now();
            loop {
                let mut interrupted = false;
                if let Some((rec, id)) = &recorder {
                    match rec.state() {
                        rune::RecorderState::Recording if !announced => {
                            announced = true;
                            started = std::time::Instant::now()
                                .checked_sub(Duration::from_secs_f64(rec.elapsed().max(0.0)))
                                .unwrap_or_else(std::time::Instant::now);
                            let _ = events.send(Event::Started {
                                id: id.clone(),
                                started,
                            });
                            let _ = events.send(Event::Recording);
                            wake();
                        }
                        rune::RecorderState::Failed(message) => {
                            let _ = events.send(Event::Error(message));
                            recorder = None;
                            wake();
                        }
                        rune::RecorderState::Interrupted(_) => {
                            interrupted = true;
                        }
                        _ => {}
                    }
                }
                let command = if interrupted {
                    Command::Finish
                } else {
                    match commands.recv_timeout(Duration::from_millis(50)) {
                        Ok(c) => c,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    }
                };
                let result: Result<(), String> = (|| {
                    match command {
                        Command::Check => {
                            Client::discover()?.check()?;
                            let _ = events.send(Event::Ready);
                        }
                        Command::Start => {
                            if recorder.is_some() {
                                return Err("Already recording".into());
                            }
                            timeline::private_dir(&root)
                                .map_err(|_| "Cannot create audio folder")?;
                            let id = format!(
                                "recording-{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_nanos()
                            );
                            let rec = rune::Recorder::to_file(root.join(format!("{id}.wav")))
                                .sample_rate(16000)
                                .channels(1);
                            rec.start();
                            recorder = Some((rec, id));
                            announced = false;
                        }
                        Command::Finish => {
                            let (rec, id) = recorder.take().ok_or("No recording to stop")?;
                            let saved = rec.stop()?;
                            let (samples, rate) = wav_info(&saved.path)?;
                            let recording = Recording {
                                id: id.clone(),
                                path: saved.path.clone(),
                                started,
                                samples,
                                sample_rate: rate,
                            };
                            let _ = events.send(Event::AudioSaved(recording));
                            wake();
                            let (text, cues) = Client::discover()?.transcribe(&saved.path)?;
                            let _ = events.send(Event::TimedTranscript { id, text, cues });
                        }
                        Command::Cancel => {
                            if let Some((rec, _)) = recorder.take() {
                                rec.cancel();
                            }
                            let _ = events.send(Event::Cancelled);
                        }
                        Command::Retry(path) => {
                            let id = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .ok_or("Invalid recording path")?
                                .to_owned();
                            let (text, cues) = Client::discover()?.transcribe(&path)?;
                            let _ = events.send(Event::TimedTranscript { id, text, cues });
                        }
                    }
                    Ok(())
                })();
                if let Err(message) = result {
                    let _ = events.send(Event::Error(message));
                }
                wake();
            }
            // Recorder's last handle stops and retains unfinished audio on shutdown.
        });
        Self { send, receive }
    }
}
#[derive(Deserialize)]
struct Discovery {
    port: u16,
    token: String,
    #[serde(rename = "apiVersion")]
    api_version: String,
}
struct Client {
    base: String,
    token: String,
}
fn discovery_path() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("HEX_APPLICATION_SUPPORT_DIR") {
        return Ok(PathBuf::from(dir).join("local-api.json"));
    }
    #[cfg(target_os = "macos")]
    let dir = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?)
        .join("Library/Application Support/hex");
    #[cfg(target_os = "windows")]
    let dir = PathBuf::from(std::env::var_os("APPDATA").ok_or("APPDATA is not set")?).join("hex");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let dir = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("hex");
    Ok(dir.join("local-api.json"))
}
fn agent(seconds: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(seconds)))
        .build()
        .into()
}
impl Client {
    fn discover() -> Result<Self, String> {
        let d: Discovery = serde_json::from_slice(
            &std::fs::read(discovery_path()?).map_err(|_| "Open Hex, then reconnect")?,
        )
        .map_err(|_| "Invalid Hex discovery file")?;
        if d.port == 0 || d.token.is_empty() || d.api_version != "2" {
            return Err("Unsupported Hex API".into());
        }
        Ok(Self {
            base: format!("http://127.0.0.1:{}", d.port),
            token: d.token,
        })
    }
    fn request(
        &self,
        path: &str,
        body: Option<Value>,
        owner: Option<&str>,
    ) -> Result<Value, String> {
        let agent = agent(125);
        let url = format!("{}{path}", self.base);
        let auth = format!("Bearer {}", self.token);
        let response = if let Some(body) = body {
            let mut r = agent.post(&url).header("Authorization", &auth);
            if let Some(owner) = owner {
                r = r.header("X-Hex-Stream-Token", owner);
            }
            r.send_json(body)
        } else {
            agent.get(&url).header("Authorization", &auth).call()
        };
        let mut response = response.map_err(|_| "Hex did not respond; audio remains saved")?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(format!(
                "Hex transcription unavailable (HTTP {status}); audio remains saved"
            ));
        }
        if status == 204 {
            return Ok(Value::Null);
        }
        response
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_json()
            .map_err(|_| "Invalid Hex response".into())
    }
    fn check(&self) -> Result<(), String> {
        let c = self.request("/capabilities", None, None)?;
        if c.get("streams").and_then(Value::as_bool) != Some(true) {
            return Err("Update Hex: timed transcription streams are required".into());
        }
        Ok(())
    }
    fn transcribe(&self, path: &Path) -> Result<(String, Vec<Cue>), String> {
        let (samples, rate) = wav_info(path)?;
        if rate != 16000 {
            return Err("Transcription requires a 16 kHz recording".into());
        }
        let stream = self.request(
            "/streams",
            Some(json!({"sampleFormat":"s16le","sampleRate":16000})),
            None,
        )?;
        let id = stream
            .get("id")
            .and_then(Value::as_u64)
            .ok_or("Invalid Hex stream ID")?;
        let owner = stream
            .get("ownerToken")
            .and_then(Value::as_str)
            .ok_or("No Hex stream ownership token")?;
        let result = (|| {
            let mut wav = hound::WavReader::open(path).map_err(|_| "Cannot read saved audio")?;
            let mut input = wav.samples::<i16>();
            loop {
                let mut bytes = Vec::with_capacity(16000);
                for _ in 0..8000 {
                    match input.next() {
                        Some(Ok(sample)) => bytes.extend_from_slice(&sample.to_le_bytes()),
                        Some(Err(_)) => return Err("Invalid saved audio samples".into()),
                        None => break,
                    }
                }
                if bytes.is_empty() {
                    break;
                }
                let started = std::time::Instant::now();
                loop {
                    let response = agent(10)
                        .post(format!("{}/streams/{id}/audio", self.base))
                        .header("Authorization", format!("Bearer {}", self.token))
                        .header("X-Hex-Stream-Token", owner)
                        .header("Content-Type", "application/octet-stream")
                        .send(bytes.as_slice())
                        .map_err(|_| "Audio upload to Hex failed")?;
                    let status = response.status().as_u16();
                    if status == 204 {
                        break;
                    }
                    if status != 429 || started.elapsed() > Duration::from_secs(120) {
                        return Err(format!(
                            "Hex audio stream failed (HTTP {status}); audio remains saved"
                        ));
                    }
                    self.request(
                        &format!("/streams/{id}/heartbeat"),
                        Some(json!({})),
                        Some(owner),
                    )?;
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            let result = self.request(
                &format!("/streams/{id}/finish"),
                Some(json!({})),
                Some(owner),
            )?;
            parse_transcript(&result, samples * 1000 / rate as u64)
        })();
        if result.is_err() {
            let _ = self.request(
                &format!("/streams/{id}/cancel"),
                Some(json!({})),
                Some(owner),
            );
        }
        result
    }
}
pub fn wav_info(path: &Path) -> Result<(u64, u32), String> {
    let wav = hound::WavReader::open(path).map_err(|_| "Saved recording is not a valid WAV")?;
    let spec = wav.spec();
    if spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
        || spec.sample_rate == 0
    {
        return Err("Unsupported recording format".into());
    }
    Ok((wav.duration() as u64, spec.sample_rate))
}
fn parse_transcript(result: &Value, duration: u64) -> Result<(String, Vec<Cue>), String> {
    let text = result
        .get("text")
        .and_then(Value::as_str)
        .ok_or("Hex returned no transcript")?
        .to_owned();
    let mut cues = Vec::new();
    for segment in result
        .get("segments")
        .and_then(Value::as_array)
        .ok_or("Hex returned no segment timing")?
    {
        let text = segment
            .get("text")
            .and_then(Value::as_str)
            .ok_or("Invalid segment text")?;
        if text.trim().is_empty() {
            continue;
        }
        let start_ms = segment
            .get("startMs")
            .and_then(Value::as_u64)
            .ok_or("Invalid segment timing")?;
        let end_ms = segment
            .get("endMs")
            .and_then(Value::as_u64)
            .ok_or("Invalid segment timing")?;
        cues.push(Cue {
            start_ms,
            end_ms,
            text: text.into(),
        });
    }
    timeline::validate_cues(&cues, duration).map_err(|e| e.to_string())?;
    if !text.trim().is_empty() && cues.is_empty() {
        return Err("Hex returned text without timing; audio remains saved".into());
    }
    Ok((text, cues))
}
pub fn transcribe_file(path: &Path) -> Result<(String, Vec<Cue>), String> {
    Client::discover()?.transcribe(path)
}
pub fn check_installed() -> Result<(), String> {
    Client::discover()?.check()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uploads_exact_saved_pcm_and_uses_owned_stream_timestamps() {
        use std::io::{Read, Write};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio.wav");
        let mut wav = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..8000 {
            wav.write_sample(321i16).unwrap();
        }
        wav.finalize().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for step in 0..3 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut data = Vec::new();
                let mut buf = [0; 16384];
                loop {
                    let n = socket.read(&mut buf).unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&buf[..n]);
                    if let Some(end) = data.windows(4).position(|b| b == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
                        let len: usize = headers
                            .lines()
                            .find_map(|l| {
                                l.strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap())
                            })
                            .unwrap_or(0);
                        if data.len() < end + 4 + len {
                            continue;
                        }
                        assert!(headers.contains("authorization: bearer fixture"));
                        if step == 0 {
                            assert!(headers.starts_with("post /streams "));
                        } else {
                            assert!(headers.contains("x-hex-stream-token: owned"));
                        }
                        if step == 1 {
                            assert!(headers.starts_with("post /streams/7/audio "));
                            assert_eq!(len, 16000);
                            assert!(
                                data[end + 4..]
                                    .as_chunks::<2>()
                                    .0
                                    .iter()
                                    .all(|b| *b == 321i16.to_le_bytes())
                            );
                        }
                        break;
                    }
                }
                let body=match step{0=>json!({"id":7,"ownerToken":"owned"}).to_string(),1=>String::new(),_=>json!({"text":"Hello","durationMs":500,"segments":[{"startMs":0,"endMs":500,"text":"Hello"}]}).to_string()};
                let status = if step == 1 { 204 } else { 200 };
                write!(socket,"HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        let (text, cues) = Client {
            base,
            token: "fixture".into(),
        }
        .transcribe(&path)
        .unwrap();
        assert_eq!(text, "Hello");
        assert_eq!(cues[0].end_ms, 500);
        server.join().unwrap();
    }
    #[test]
    fn preserves_real_timing_and_rejects_invented_or_out_of_bounds_cues() {
        let v = json!({"text":"Hello. Another topic.","segments":[{"startMs":250,"endMs":1200,"text":"Hello."},{"startMs":1400,"endMs":2100,"text":"Another topic."}]});
        let (_, cues) = parse_transcript(&v, 2200).unwrap();
        assert_eq!(cues[1].start_ms, 1400);
        assert!(parse_transcript(&v, 2000).is_err());
        assert!(parse_transcript(&json!({"text":"Hello","segments":[]}), 1000).is_err());
    }
}
