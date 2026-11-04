//! Hex desktop capture. Authentication stays in this process; no clipboard or global hotkeys.
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Check,
    Start,
    Finish,
    Cancel,
}
#[derive(Debug)]
pub enum Event {
    Ready,
    Recording,
    Transcript(String),
    Cancelled,
    Error(String),
}
pub struct Worker {
    pub send: Sender<Command>,
    pub receive: Receiver<Event>,
}
impl Worker {
    pub fn start(wake: impl Fn() + Send + 'static) -> Self {
        let (send, commands) = mpsc::channel();
        let (events, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let mut capture: Option<Capture> = None;
            loop {
                if capture
                    .as_ref()
                    .is_some_and(|c| c.failed.load(Ordering::Acquire))
                {
                    drop(capture.take());
                    if events.send(Event::Error("Lost connection to Hex; recording stopped or will expire within 10 seconds.".into())).is_err() { break; }
                    wake();
                }
                let command = match commands.recv_timeout(Duration::from_millis(200)) {
                    Ok(command) => command,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let result = match command {
                    Command::Check => Client::discover()
                        .and_then(|c| c.check())
                        .map(|()| Event::Ready),
                    Command::Start if capture.is_none() => {
                        Client::discover().and_then(|c| c.start()).map(|c| {
                            capture = Some(c);
                            Event::Recording
                        })
                    }
                    Command::Start => Err("Tacit is already recording.".into()),
                    Command::Finish => match capture.take() {
                        Some(mut c) => c.finish().map(Event::Transcript),
                        None => Err("There is no Tacit recording to finish.".into()),
                    },
                    Command::Cancel => match capture.take() {
                        Some(mut c) => c.cancel().map(|()| Event::Cancelled),
                        None => Ok(Event::Cancelled),
                    },
                };
                if events.send(result.unwrap_or_else(Event::Error)).is_err() {
                    break;
                }
                wake();
            }
            // Dropping an owned capture cancels only that capture. If the process dies,
            // Hex's 10-second lease provides the final backstop.
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
#[derive(Clone)]
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
    let dir = match std::env::var_os("XDG_DATA_HOME") {
        Some(d) => PathBuf::from(d).join("hex"),
        None => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?)
            .join(".local/share/hex"),
    };
    Ok(dir.join("local-api.json"))
}
impl Client {
    fn discover() -> Result<Self, String> {
        let bytes = std::fs::read(discovery_path()?).map_err(|_| {
            "Open Hex and finish its microphone/model setup, then click Reconnect Hex.".to_string()
        })?;
        let d: Discovery = serde_json::from_slice(&bytes)
            .map_err(|_| "Hex discovery data is invalid. Restart Hex.".to_string())?;
        if d.port == 0 || d.token.is_empty() || d.api_version != "2" {
            return Err("Unsupported Hex API. Update Hex and reconnect.".into());
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
        timeout: u64,
    ) -> Result<Value, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(timeout)))
            .build()
            .into();
        let auth = format!("Bearer {}", self.token);
        let result = if let Some(body) = body {
            let mut request = agent
                .post(format!("{}{path}", self.base))
                .header("Authorization", &auth);
            if let Some(owner) = owner {
                request = request.header("X-Hex-Dictation-Token", owner);
            }
            request.send_json(body)
        } else {
            agent
                .get(format!("{}{path}", self.base))
                .header("Authorization", &auth)
                .call()
        };
        let mut response = result.map_err(|_| {
            "Hex did not respond. Check that it is running and reconnect.".to_string()
        })?;
        let status = response.status().as_u16();
        if status == 204 {
            return Ok(Value::Null);
        }
        let value: Value = response
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_json()
            .unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            let code = value
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("request-failed");
            return Err(match code {
                "dictation-busy"|"service-capture-busy" => "Hex is busy recording elsewhere. Finish that recording first.".into(),
                "service-capture-unavailable" => "This Hex instance does not support desktop recording. Open the Hex desktop app.".into(),
                "microphone-lost" => "The microphone disconnected. Check it in Hex and try again.".into(),
                _ => format!("Hex request failed (HTTP {status}, {}).",code.chars().take(80).collect::<String>()),
            });
        }
        Ok(value)
    }
    fn check(&self) -> Result<(), String> {
        let c = self.request("/capabilities", None, None, 4)?;
        if c.get("serviceCapture").and_then(Value::as_bool) != Some(true) {
            return Err("Hex desktop capture is unavailable on this instance.".into());
        }
        Ok(())
    }
    fn start(self) -> Result<Capture, String> {
        self.check()?;
        let result = self.request("/dictations", Some(json!({"source":"Tacit"})), None, 12)?;
        let id = result
            .get("id")
            .and_then(Value::as_u64)
            .ok_or("Hex returned an invalid recording ID")?;
        let owner = result
            .get("ownerToken")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("Hex returned no recording ownership token")?
            .to_owned();
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (c, o, stop_flag, failed_flag) =
            (self.clone(), owner.clone(), stop.clone(), failed.clone());
        std::thread::spawn(move || {
            // Short sleeps keep cancellation responsive without blocking the UI.
            for tick in 1u64.. {
                std::thread::sleep(Duration::from_millis(100));
                if stop_flag.load(Ordering::Acquire) {
                    break;
                }
                if tick % 30 == 0
                    && c.request(
                        &format!("/dictations/{id}/heartbeat"),
                        Some(json!({})),
                        Some(&o),
                        4,
                    )
                    .is_err()
                {
                    failed_flag.store(true, Ordering::Release);
                    break;
                }
            }
        });
        Ok(Capture {
            client: self,
            id,
            owner,
            stop,
            failed,
            ended: false,
        })
    }
}
struct Capture {
    client: Client,
    id: u64,
    owner: String,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    ended: bool,
}
impl Capture {
    fn finish(&mut self) -> Result<String, String> {
        let result = self.client.request(
            &format!("/dictations/{}/finish", self.id),
            Some(json!({})),
            Some(&self.owner),
            125,
        )?;
        let text = result
            .get("transcript")
            .and_then(Value::as_str)
            .ok_or("Hex returned an invalid transcript")?
            .to_owned();
        self.ended = true;
        self.stop.store(true, Ordering::Release);
        Ok(text)
    }
    fn cancel(&mut self) -> Result<(), String> {
        let result = self.client.request(
            &format!("/dictations/{}/cancel", self.id),
            Some(json!({})),
            Some(&self.owner),
            5,
        );
        self.ended = result.is_ok();
        self.stop.store(true, Ordering::Release);
        result.map(|_| ())
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if !self.ended {
            let _ = self.cancel();
        }
    }
}

/// Read-only health check for diagnostics; never opens the microphone.
pub fn check_installed() -> Result<(), String> {
    Client::discover()?.check()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    fn server(replies: Vec<(&'static str, u16, Value)>) -> (Client, std::thread::JoinHandle<()>) {
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            for (expected, status, body) in replies {
                let (mut socket, _) = server.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0; 2048];
                loop {
                    let n = socket.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|l| {
                                l.strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            assert!(headers.lines().next().unwrap().contains(expected));
                            assert!(headers.contains("authorization: bearer test-auth"));
                            if expected.contains("/7/") {
                                assert!(headers.contains("x-hex-dictation-token: test-owner"));
                            }
                            break;
                        }
                    }
                }
                let body = if status == 204 {
                    String::new()
                } else {
                    body.to_string()
                };
                write!(socket,"HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        (
            Client {
                base,
                token: "test-auth".into(),
            },
            thread,
        )
    }
    fn opening() -> Vec<(&'static str, u16, Value)> {
        vec![
            ("get /capabilities", 200, json!({"serviceCapture":true})),
            (
                "post /dictations ",
                201,
                json!({"id":7,"ownerToken":"test-owner","sampleRate":16000}),
            ),
        ]
    }
    #[test]
    fn capture_returns_raw_text_and_does_not_cancel_a_finished_recording() {
        let mut replies = opening();
        replies.push((
            "/7/finish",
            200,
            json!({"transcript":"What if we listened?","durationMs":1000}),
        ));
        let (client, thread) = server(replies);
        let mut capture = client.start().unwrap();
        assert_eq!(capture.finish().unwrap(), "What if we listened?");
        assert!(capture.stop.load(Ordering::Acquire));
        drop(capture);
        thread.join().unwrap();
    }
    #[test]
    fn dropping_a_recording_cancels_only_its_owned_session() {
        let mut replies = opening();
        replies.push(("/7/cancel", 204, Value::Null));
        let (client, thread) = server(replies);
        drop(client.start().unwrap());
        thread.join().unwrap();
    }
    #[test]
    fn busy_recording_is_preserved_and_error_is_actionable() {
        let mut replies = opening();
        replies[1] = ("post /dictations ", 409, json!({"code":"dictation-busy"}));
        let (client, thread) = server(replies);
        assert!(matches!(client.start(),Err(message) if message.contains("recording elsewhere")));
        thread.join().unwrap();
    }
    #[test]
    fn unsupported_capture_is_reported_before_any_recording_request() {
        let (client, thread) = server(vec![(
            "get /capabilities",
            200,
            json!({"serviceCapture":false}),
        )]);
        assert!(client.start().is_err());
        thread.join().unwrap();
    }
}
