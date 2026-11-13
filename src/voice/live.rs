//! Feed the recorder's growing PCM WAV to Hex; the saved file remains authoritative.
use super::{Client, Event, agent, parse_transcript};
use crate::timeline::Cue;
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

type Transcript = Result<(String, Vec<Cue>), String>;
enum Signal {
    Finish(u64),
    Cancel,
}
pub(super) struct Live {
    send: Sender<Signal>,
    result: Receiver<Transcript>,
}
impl Live {
    pub(super) fn start(
        path: PathBuf,
        recording: String,
        events: Sender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (send, commands) = mpsc::channel();
        let (done, result) = mpsc::channel();
        std::thread::spawn(move || {
            let result = Client::discover()
                .and_then(|client| run(client, &path, &recording, &commands, &events, &wake));
            if let Err(message) = &result {
                let _ = events.send(Event::LiveStatus {
                    id: recording,
                    message: format!(
                        "Live transcription paused: {message}. Audio is still recording."
                    ),
                });
                wake();
            }
            let _ = done.send(result);
        });
        Self { send, result }
    }
    pub(super) fn finish(self, samples: u64) -> Transcript {
        let _ = self.send.send(Signal::Finish(samples));
        self.result
            .recv_timeout(Duration::from_secs(150))
            .map_err(|_| "Live transcription did not finish; audio is saved".to_owned())?
    }
    pub(super) fn cancel(self) {
        let _ = self.send.send(Signal::Cancel);
    }
}

// WAV headers can contain JUNK/LIST chunks and unfinished RIFF/data lengths. Never
// assume a 44-byte header. Only complete, physically present PCM16 samples are read.
fn wav_data(path: &Path) -> Result<Option<(u64, u64)>, String> {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot read growing recording".into()),
    };
    let len = f.metadata().map_err(|_| "Cannot inspect recording")?.len();
    if len < 12 {
        return Ok(None);
    }
    let mut header = [0; 12];
    f.read_exact(&mut header)
        .map_err(|_| "Cannot read WAV header")?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Err("Live recording is not a PCM WAV".into());
    }
    let mut offset = 12;
    let mut format = false;
    while offset + 8 <= len {
        if offset > 16 * 1024 * 1024 {
            return Err("WAV metadata too large".into());
        }
        f.seek(SeekFrom::Start(offset))
            .map_err(|_| "Cannot seek recording")?;
        let mut chunk = [0; 8];
        f.read_exact(&mut chunk)
            .map_err(|_| "Cannot read WAV chunk")?;
        let size = u32::from_le_bytes(chunk[4..].try_into().unwrap()) as u64;
        let start = offset + 8;
        if &chunk[..4] == b"data" {
            if !format {
                return Err("WAV data precedes its format".into());
            }
            let physical = len - start;
            let available = if size > 0 && size != u32::MAX as u64 {
                physical.min(size)
            } else {
                physical
            };
            return Ok(Some((start, available / 2 * 2)));
        }
        if start + size > len {
            return Ok(None);
        }
        if &chunk[..4] == b"fmt " {
            if !(16..=4096).contains(&size) {
                return Err("Invalid PCM format header".into());
            }
            let mut fmt = vec![0; size as usize];
            f.read_exact(&mut fmt)
                .map_err(|_| "Cannot read PCM format")?;
            let encoding = u16::from_le_bytes(fmt[..2].try_into().unwrap());
            let pcm = encoding == 1
                || (encoding == 0xfffe
                    && fmt.len() >= 40
                    && fmt[24..40]
                        == [
                            1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
                        ]);
            if !pcm
                || u16::from_le_bytes(fmt[2..4].try_into().unwrap()) != 1
                || u32::from_le_bytes(fmt[4..8].try_into().unwrap()) != 16000
                || u16::from_le_bytes(fmt[12..14].try_into().unwrap()) != 2
                || u16::from_le_bytes(fmt[14..16].try_into().unwrap()) != 16
            {
                return Err("Live transcription requires 16 kHz mono PCM16".into());
            }
            format = true;
        }
        offset = start + size + size % 2;
    }
    Ok(None)
}
fn run(
    client: Client,
    path: &Path,
    recording: &str,
    commands: &Receiver<Signal>,
    events: &Sender<Event>,
    wake: &Arc<dyn Fn() + Send + Sync>,
) -> Transcript {
    let stream = client.request(
        "/streams",
        Some(json!({"sampleFormat":"s16le","sampleRate":16000})),
        None,
    )?;
    let id = stream
        .get("id")
        .and_then(Value::as_u64)
        .ok_or("Invalid stream ID")?;
    let owner = stream
        .get("ownerToken")
        .and_then(Value::as_str)
        .ok_or("Missing stream ownership")?
        .to_owned();
    let stop = Arc::new(AtomicBool::new(false));
    let reader_stop = stop.clone();
    let reader_client = client.clone();
    let reader_owner = owner.clone();
    let reader_events = events.clone();
    let reader_recording = recording.to_owned();
    let reader_wake = wake.clone();
    std::thread::spawn(move || {
        read_events(
            reader_client,
            id,
            reader_owner,
            reader_recording,
            reader_events,
            reader_stop,
            reader_wake,
        )
    });
    let result = (|| {
        let mut sent = 0u64;
        let mut hash = blake3::Hasher::new();
        let mut final_samples = None;
        let mut heartbeat = Instant::now();
        let mut backlog = None;
        loop {
            match commands.try_recv() {
                Ok(Signal::Cancel) | Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("Recording stopped".into());
                }
                Ok(Signal::Finish(samples)) => final_samples = Some(samples),
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if final_samples.is_some_and(|n| sent > n * 2) {
                return Err("Live audio exceeded saved recording".into());
            }
            if final_samples.is_some_and(|n| sent == n * 2) {
                // Detect a backend that rewrote/preallocated bytes while recording.
                let mut wav =
                    hound::WavReader::open(path).map_err(|_| "Cannot verify saved audio")?;
                let mut actual = blake3::Hasher::new();
                for sample in wav.samples::<i16>() {
                    actual.update(&sample.map_err(|_| "Invalid saved audio")?.to_le_bytes());
                }
                if actual.finalize() != hash.finalize() {
                    return Err("Live samples differ from the saved audio".into());
                }
                return parse_transcript(
                    &client.request(
                        &format!("/streams/{id}/finish"),
                        Some(json!({})),
                        Some(&owner),
                    )?,
                    final_samples.unwrap() * 1000 / 16000,
                );
            }
            if let Some((offset, available)) = wav_data(path)? {
                let available = available.min(final_samples.map_or(u64::MAX, |n| n * 2));
                if available > sent {
                    let count = (available - sent).min(16000) as usize;
                    let mut data = vec![0; count];
                    let mut file = File::open(path).map_err(|_| "Cannot read live audio")?;
                    file.seek(SeekFrom::Start(offset + sent))
                        .and_then(|_| file.read_exact(&mut data))
                        .map_err(|_| "Cannot read live samples")?;
                    let response = agent(5)
                        .post(format!("{}/streams/{id}/audio", client.base))
                        .header("Authorization", format!("Bearer {}", client.token))
                        .header("X-Hex-Stream-Token", &owner)
                        .header("Content-Type", "application/octet-stream")
                        .send(data.as_slice())
                        .map_err(|_| "Hex audio connection failed")?;
                    match response.status().as_u16() {
                        204 => {
                            hash.update(&data);
                            sent += count as u64;
                            heartbeat = Instant::now();
                            backlog = None;
                            continue;
                        }
                        429 => {
                            let since = backlog.get_or_insert_with(Instant::now);
                            if since.elapsed() > Duration::from_secs(120) {
                                return Err("Hex is too far behind".into());
                            }
                        }
                        _ => return Err("Hex rejected live audio".into()),
                    }
                } else if final_samples.is_some() {
                    return Err("Saved audio is shorter than expected".into());
                }
            } else if final_samples.is_some() {
                return Err("Saved audio header is incomplete".into());
            }
            if heartbeat.elapsed() >= Duration::from_secs(3) {
                client.request(
                    &format!("/streams/{id}/heartbeat"),
                    Some(json!({})),
                    Some(&owner),
                )?;
                heartbeat = Instant::now();
            }
            match commands.recv_timeout(Duration::from_millis(100)) {
                Ok(Signal::Finish(samples)) => final_samples = Some(samples),
                Ok(Signal::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Recording stopped".into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    })();
    stop.store(true, Ordering::Relaxed);
    if result.is_err() {
        let _ = client.request(
            &format!("/streams/{id}/cancel"),
            Some(json!({})),
            Some(&owner),
        );
    }
    result
}
fn decode_event(value: Value, recording: &str) -> Result<Option<Event>, String> {
    match value.get("type").and_then(Value::as_str) {
        Some("partial") => {
            let stable = value
                .get("stable")
                .and_then(Value::as_str)
                .ok_or("Invalid partial")?;
            let tentative = value
                .get("tentative")
                .and_then(Value::as_str)
                .ok_or("Invalid partial")?;
            if stable.len() + tentative.len() > 96000 {
                return Err("Partial text too large".into());
            }
            Ok(Some(Event::Partial {
                id: recording.into(),
                text: format!("{stable}{tentative}"),
            }))
        }
        Some("segment") => {
            let segment = value
                .get("segment")
                .and_then(Value::as_u64)
                .ok_or("Invalid segment ID")?;
            let cue = Cue {
                start_ms: value
                    .get("startMs")
                    .and_then(Value::as_u64)
                    .ok_or("Invalid segment start")?,
                end_ms: value
                    .get("endMs")
                    .and_then(Value::as_u64)
                    .ok_or("Invalid segment end")?,
                text: value
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("Invalid segment text")?
                    .into(),
            };
            if cue.text.trim().is_empty() {
                return Ok(None);
            }
            if cue.start_ms >= cue.end_ms || cue.text.len() > 96000 {
                return Err("Invalid live segment".into());
            }
            Ok(Some(Event::Segment {
                id: recording.into(),
                segment,
                cue,
            }))
        }
        Some("error") => Err("Hex ended live transcription; saved audio can be retried".into()),
        _ => Ok(None),
    }
}
fn read_events(
    client: Client,
    id: u64,
    owner: String,
    recording: String,
    events: Sender<Event>,
    stop: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    while !stop.load(Ordering::Relaxed) {
        let result = (|| -> Result<bool, String> {
            let mut response = agent(30)
                .get(format!("{}/streams/{id}/events", client.base))
                .header("Authorization", format!("Bearer {}", client.token))
                .header("X-Hex-Stream-Token", &owner)
                .call()
                .map_err(|_| "Live event connection interrupted")?;
            if response.status().as_u16() != 200 {
                return Err("Live events unavailable".into());
            }
            let mut reader = BufReader::new(response.body_mut().as_reader());
            let mut line = String::new();
            while !stop.load(Ordering::Relaxed) {
                line.clear();
                if reader
                    .read_line(&mut line)
                    .map_err(|_| "Live event connection interrupted")?
                    == 0
                {
                    return Ok(false);
                }
                if line.len() > 192000 {
                    return Err("Live event too large".into());
                }
                if let Some(data) = line.strip_prefix("data:") {
                    let value: Value =
                        serde_json::from_str(data.trim()).map_err(|_| "Invalid live event")?;
                    if matches!(
                        value.get("type").and_then(Value::as_str),
                        Some("final" | "cancelled")
                    ) {
                        return Ok(true);
                    }
                    if let Some(event) = decode_event(value, &recording)? {
                        if events.send(event).is_err() {
                            return Ok(true);
                        }
                        wake();
                    }
                }
            }
            Ok(true)
        })();
        if matches!(result, Ok(true)) {
            return;
        }
        // Reconnect: Hex replays completed segments, deduplicated by the event reducer.
        if !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growing_wav_handles_metadata_unfinished_lengths_and_sample_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("growing.wav");
        assert_eq!(wav_data(&path).unwrap(), None);
        let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
        bytes.extend_from_slice(b"JUNK\x03\0\0\0abc\0");
        bytes.extend_from_slice(b"fmt \x10\0\0\0");
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&32000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data\0\0\0\0");
        let offset = bytes.len() as u64;
        bytes.extend_from_slice(&[1, 0, 2]);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(wav_data(&path).unwrap(), Some((offset, 2)));
        bytes.push(0);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(wav_data(&path).unwrap(), Some((offset, 4)));
        bytes[offset as usize - 4..offset as usize].copy_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"LIST\x04\0\0\0tail");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(wav_data(&path).unwrap(), Some((offset, 4)));
    }

    #[test]
    fn uploads_and_delivers_segments_before_finish() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.wav");
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
        let (uploaded, upload_received) = mpsc::channel();
        let server = std::thread::spawn(move || {
            // Served until the stream is finished: a slow machine sends heartbeats meanwhile.
            let mut served = vec![];
            while served.last() != Some(&"finish") {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(socket.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    headers.push_str(&line.to_lowercase());
                }
                assert!(headers.contains("authorization: bearer fixture"));
                let len = headers
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length:")
                            .map(|n| n.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let (status, response) = if first.starts_with("POST /streams ") {
                    served.push("stream");
                    (200, json!({"id":7,"ownerToken":"owned"}).to_string())
                } else {
                    assert!(headers.contains("x-hex-stream-token: owned"));
                    if first.contains("/audio ") {
                        assert_eq!(body.len(), 16000);
                        assert!(
                            body.as_chunks::<2>()
                                .0
                                .iter()
                                .all(|b| *b == 321i16.to_le_bytes())
                        );
                        uploaded.send(()).unwrap();
                        served.push("audio");
                        (204, String::new())
                    } else if first.contains("/heartbeat ") {
                        (204, String::new())
                    } else if first.contains("/events ") {
                        served.push("events");
                        (200, "data: {\"type\":\"partial\",\"stable\":\"Hello\",\"tentative\":\"\"}\n\ndata: {\"type\":\"segment\",\"segment\":0,\"startMs\":0,\"endMs\":500,\"text\":\"Hello\"}\n\ndata: {\"type\":\"final\"}\n\n".into())
                    } else {
                        assert!(first.contains("/finish "), "{first}");
                        served.push("finish");
                        (200, json!({"text":"Hello","segments":[{"startMs":0,"endMs":500,"text":"Hello"}]}).to_string())
                    }
                };
                write!(socket, "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
            }
            served
        });
        let (commands, receive_commands) = mpsc::channel();
        let (events, receive_events) = mpsc::channel();
        let task = std::thread::spawn(move || {
            run(
                Client {
                    base,
                    token: "fixture".into(),
                },
                &path,
                "recording",
                &receive_commands,
                &events,
                &(Arc::new(|| {}) as Arc<dyn Fn() + Send + Sync>),
            )
        });
        upload_received
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(
            receive_events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Partial { .. }
        ));
        assert!(matches!(
            receive_events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Segment { .. }
        ));
        commands.send(Signal::Finish(8000)).unwrap();
        let (text, cues) = task.join().unwrap().unwrap();
        assert_eq!(text, "Hello");
        assert_eq!(cues[0].end_ms, 500);
        let mut served = server.join().unwrap();
        served.sort_unstable();
        assert_eq!(served, ["audio", "events", "finish", "stream"]);
    }

    #[test]
    fn live_events_distinguish_provisional_text_from_timed_sources() {
        let partial = decode_event(
            json!({"type":"partial","stable":"A ","tentative":"garden"}),
            "r",
        )
        .unwrap();
        assert!(matches!(partial, Some(Event::Partial { text, .. }) if text == "A garden"));
        let segment = decode_event(
            json!({"type":"segment","segment":2,"startMs":100,"endMs":900,"text":"A garden"}),
            "r",
        )
        .unwrap();
        assert!(
            matches!(segment, Some(Event::Segment { segment: 2, cue, .. }) if cue.start_ms == 100 && cue.end_ms == 900)
        );
        assert!(
            decode_event(
                json!({"type":"segment","segment":2,"startMs":900,"endMs":100,"text":"invalid"}),
                "r"
            )
            .is_err()
        );
    }
}
