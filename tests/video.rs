use std::process::Command;
use tacit::{
    model::Session,
    timeline::{Audio, Event, Journal, PixelFrame, content_hash},
    video_archive,
};

#[test]
fn original_video_roundtrips_pixels_and_preserves_frame_timestamps() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("ffmpeg unavailable; codec integration test skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&dir.path().join("session.json"), Session::default()).unwrap();
    let red = [255, 0, 0, 255].repeat(4);
    let blue = [0, 17, 255, 255].repeat(4);
    for (at, rgba) in [(100, &red), (140, &blue), (250, &red)] {
        let png = video_archive::png_bytes(2, 2, rgba).unwrap();
        let asset = j.import_bytes(&png, "png").unwrap();
        j.append(
            at,
            Event::Pixels {
                frame: PixelFrame {
                    asset,
                    width: 2,
                    height: 2,
                    scale: 1.0,
                    content_hash: content_hash(&j.projection.session).unwrap(),
                },
            },
        )
        .unwrap();
    }
    // Real PCM starts 20ms after the first video frame.
    let wav = dir.path().join("audio.wav");
    let mut writer = hound::WavWriter::create(
        &wav,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..1600 {
        writer.write_sample(1234i16).unwrap();
    }
    writer.finalize().unwrap();
    let asset = j.import(&wav, "wav").unwrap();
    j.append(
        300,
        Event::AudioSaved {
            audio: Audio {
                id: "audio".into(),
                asset,
                start_ms: 120,
                duration_ms: 100,
                sample_rate: 16000,
                samples: 1600,
            },
        },
    )
    .unwrap();
    let output = dir.path().join("original.mkv");
    video_archive::export(&j, &output).unwrap();
    assert!(video_archive::export(&j, &output).is_err()); // Never overwrite an export.
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&output)
        .args([
            "-map",
            "0:v",
            "-fps_mode",
            "passthrough",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    assert_eq!(decoded.stdout, [red.clone(), blue, red].concat());
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v",
            "-show_entries",
            "frame=pts_time",
            "-of",
            "csv=p=0",
        ])
        .arg(&output)
        .output()
        .unwrap();
    let times: Vec<f64> = String::from_utf8(probe.stdout)
        .unwrap()
        .lines()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(times, vec![0.0, 0.04, 0.15]);
    let sound = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&output)
        .args(["-map", "0:a", "-f", "s16le", "-"])
        .output()
        .unwrap();
    let samples: Vec<i16> = sound
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    assert!(samples[..320].iter().all(|s| *s == 0));
    assert!(samples[320..1920].iter().all(|s| *s == 1234));
}

#[test]
fn no_original_pixels_means_no_original_video_export() {
    let dir = tempfile::tempdir().unwrap();
    let j = Journal::open(&dir.path().join("session.json"), Session::demo()).unwrap();
    let output = dir.path().join("no-original.mkv");
    assert!(video_archive::export(&j, &output).is_err());
    assert!(!output.exists());
    assert!(video_archive::png_bytes(2, 2, &[0; 4]).is_err());
}

#[test]
fn resize_and_capture_gaps_do_not_scale_pixels_or_hold_a_closed_window() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut j = Journal::open(&dir.path().join("session.json"), Session::default()).unwrap();
    let red = [255, 0, 0, 255].repeat(4);
    let blue = [0, 0, 255, 255].repeat(8);
    for (at, active, pixels, width) in [
        (100, true, Some(&red), 2),
        (150, false, None, 0),
        (220, true, Some(&blue), 4),
        (300, false, None, 0),
    ] {
        j.append(at, Event::CaptureBoundary { recording: active })
            .unwrap();
        if let Some(pixels) = pixels {
            let png = video_archive::png_bytes(width, 2, pixels).unwrap();
            let asset = j.import_bytes(&png, "png").unwrap();
            j.append(
                at,
                Event::Pixels {
                    frame: PixelFrame {
                        asset,
                        width,
                        height: 2,
                        scale: 1.0,
                        content_hash: content_hash(&j.projection.session).unwrap(),
                    },
                },
            )
            .unwrap();
        }
    }
    let output = dir.path().join("resize.mkv");
    video_archive::export(&j, &output).unwrap();
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&output)
        .args([
            "-map",
            "0:v",
            "-fps_mode",
            "passthrough",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    let black = [0, 0, 0, 255].repeat(8);
    let mut padded = black.clone();
    padded[0..8].copy_from_slice(&red[0..8]);
    padded[16..24].copy_from_slice(&red[8..16]);
    assert_eq!(decoded.stdout, [padded, black, blue].concat());
}

#[test]
fn interrupted_capture_is_detected_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let mut j = Journal::open(&path, Session::default()).unwrap();
    // Simulate an uncleanly stopped process without the owner's closing guard.
    j.append(0, Event::CaptureBoundary { recording: true })
        .unwrap();
    assert!(
        video_archive::export(&j, &dir.path().join("active.mkv"))
            .unwrap_err()
            .to_string()
            .contains("still active")
    );
    drop(j);
    let j = Journal::open(&path, Session::default()).unwrap();
    assert_eq!(j.projection.pixel_capture_active, Some(false));
    assert!(j.entries.iter().any(|e| matches!(&e.event, Event::Failure { operation, .. } if operation == "original video capture")));
    assert!(video_archive::export(&j, &dir.path().join("incomplete.mkv")).is_err());
}
