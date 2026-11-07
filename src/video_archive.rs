//! Original, lossless RGBA frames and timestamp-preserving FFV1/PCM video export.
use crate::timeline::{Event, Journal};
use std::{io, path::Path, process::Command};

pub fn png_bytes(width: u32, height: u32, pixels: &[u8]) -> io::Result<Vec<u8>> {
    if pixels.len() as u64 != u64::from(width) * u64::from(height) * 4 {
        return Err(io::Error::other("Invalid RGBA frame size"));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        encoder.set_filter(png::Filter::Sub);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = encoder.write_header().map_err(io::Error::other)?;
        writer.write_image_data(pixels).map_err(io::Error::other)?;
    }
    Ok(out)
}

/// Export original pixels, never reconstructed scenes. Padding handles window resizing without scaling.
/// The PNG masters retain their native sizes, exact pixels, and event timestamps.
pub fn export(journal: &Journal, output: &Path) -> io::Result<()> {
    journal.validate_assets()?;
    if journal.projection.pixel_capture_active == Some(true) {
        return Err(io::Error::other(
            "Original capture is still active or did not close cleanly; stop capture before exporting a complete recording",
        ));
    }
    if journal.entries.iter().flat_map(|e| e.event.leaves()).any(
        |e| matches!(e, Event::Failure { operation, .. } if operation == "original video capture"),
    ) {
        return Err(io::Error::other(
            "Original capture has a recorded interruption or failure; refusing to label an incomplete export as the original video",
        ));
    }
    if output.exists() {
        return Err(io::Error::other("Video output already exists"));
    }
    if output.extension().and_then(|x| x.to_str()) != Some("mkv") {
        return Err(io::Error::other("Use .mkv for lossless FFV1 video"));
    }
    let frames: Vec<_> = journal
        .entries
        .iter()
        .flat_map(|e| {
            e.event.leaves().filter_map(move |event| {
                if let Event::Pixels { frame } = event {
                    Some((e.at_ms, frame))
                } else {
                    None
                }
            })
        })
        .collect();
    let start = frames.first().ok_or_else(|| io::Error::other("This archive has no original pixels; re-rendered video is not an original recording"))?.0;
    let end = journal.duration().max(frames.last().unwrap().0 + 1);
    let width = frames.iter().map(|(_, f)| f.width).max().unwrap();
    let height = frames.iter().map(|(_, f)| f.height).max().unwrap();
    // The manifest uses only validated content hashes, never user-provided filenames.
    let temp = journal
        .root
        .join(format!("video-export-{}", std::process::id()));
    std::fs::create_dir(&temp)?;
    crate::timeline::private_dir(&temp)?;
    let result = (|| {
        let mut paths = std::collections::HashMap::new();
        for (_, frame) in &frames {
            if paths.contains_key(&frame.asset) {
                continue;
            }
            if frame.width == width && frame.height == height {
                paths.insert(frame.asset.clone(), format!("../media/{}", frame.asset));
            } else {
                let bytes = std::fs::read(journal.asset(&frame.asset)?)?;
                let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
                    .read_info()
                    .map_err(io::Error::other)?;
                let mut rgba = vec![
                    0;
                    reader.output_buffer_size().ok_or_else(|| io::Error::other(
                        "PNG dimensions overflow"
                    ))?
                ];
                let info = reader.next_frame(&mut rgba).map_err(io::Error::other)?;
                if info.color_type != png::ColorType::Rgba
                    || info.bit_depth != png::BitDepth::Eight
                    || (info.width, info.height) != (frame.width, frame.height)
                {
                    return Err(io::Error::other(
                        "Original pixel metadata does not match PNG",
                    ));
                }
                let mut padded = [0, 0, 0, 255].repeat(width as usize * height as usize);
                for row in 0..frame.height as usize {
                    padded[row * width as usize * 4
                        ..row * width as usize * 4 + frame.width as usize * 4]
                        .copy_from_slice(
                            &rgba[row * frame.width as usize * 4
                                ..(row + 1) * frame.width as usize * 4],
                        );
                }
                crate::timeline::atomic_write(
                    &temp.join(&frame.asset),
                    &png_bytes(width, height, &padded)?,
                )?;
                paths.insert(frame.asset.clone(), frame.asset.clone());
            }
        }
        let mut points: Vec<(u64, String)> = Vec::new();
        for entry in &journal.entries {
            for event in entry.event.leaves() {
                match event {
                    Event::Pixels { frame } => {
                        points.push((entry.at_ms, paths[&frame.asset].clone()))
                    }
                    Event::CaptureBoundary { recording: false }
                        if entry.at_ms > start && entry.at_ms < end =>
                    {
                        crate::timeline::atomic_write(
                            &temp.join("blank.png"),
                            &png_bytes(
                                width,
                                height,
                                &[0, 0, 0, 255].repeat(width as usize * height as usize),
                            )?,
                        )?;
                        points.push((entry.at_ms, "blank.png".into()));
                    }
                    _ => {}
                }
            }
        }
        let mut list = String::from("ffconcat version 1.0\n");
        for (i, (at, path)) in points.iter().enumerate() {
            let next = points.get(i + 1).map_or(end, |(at, _)| *at);
            if next <= *at {
                continue;
            }
            list.push_str(&format!(
                "file '{path}'\noption framerate 1000\nduration {:.3}\n",
                (next - at) as f64 / 1000.0
            ));
        }
        list.push_str(&format!(
            "file '{}'\noption framerate 1000\n",
            points.last().unwrap().1
        ));
        let manifest = temp.join("frames.ffconcat");
        crate::timeline::atomic_write(&manifest, list.as_bytes())?;
        let executable = std::env::var_os("TACIT_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
        let mut command = Command::new(executable);
        command
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-y",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
            ])
            .arg(&manifest);
        let audio: Vec<_> = journal
            .projection
            .audio
            .iter()
            .filter(|a| a.start_ms + a.duration_ms > start && a.start_ms < end)
            .collect();
        for a in &audio {
            command.arg("-i").arg(journal.asset(&a.asset)?);
        }
        let mut filters = format!("[0:v]pad={width}:{height}:0:0:color=black,format=bgra[v]");
        for (i, a) in audio.iter().enumerate() {
            let trim = start.saturating_sub(a.start_ms) as f64 / 1000.0;
            let delay = a.start_ms.saturating_sub(start);
            filters.push_str(&format!(
                ";[{}:a]atrim=start={trim:.3},asetpts=PTS-STARTPTS,adelay={delay}:all=1[a{i}]",
                i + 1
            ));
        }
        if !audio.is_empty() {
            filters.push(';');
            for i in 0..audio.len() {
                filters.push_str(&format!("[a{i}]"));
            }
            filters.push_str(&format!("amix=inputs={}:normalize=0,apad[a]", audio.len()));
        }
        command.args([
            "-filter_complex",
            &filters,
            "-map",
            "[v]",
            "-c:v",
            "ffv1",
            "-level",
            "3",
            "-pix_fmt",
            "bgra",
            "-fps_mode",
            "vfr",
        ]);
        // FFmpeg 6.1 can silently encode one-pixel-high FFV1 slices as empty frames.
        // Keep small images in one slice, within FFmpeg's single-slice size limit;
        // ordinary window captures retain the encoder's parallel slice selection.
        if width <= 352 && height <= 288 {
            command.args(["-slices", "1"]);
        }
        if !audio.is_empty() {
            command.args(["-map", "[a]", "-c:a", "pcm_s16le"]);
        }
        command.args(["-metadata", &format!("tacit_session_start_ms={start}")]);
        command
            .args(["-t", &format!("{:.3}", (end - start) as f64 / 1000.0)])
            .arg(output);
        let mut create = std::fs::OpenOptions::new();
        create.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            create.mode(0o600);
        }
        let reserved = create.open(output)?;
        drop(reserved);
        let result = command.output().map_err(|e| {
            let _ = std::fs::remove_file(output);
            io::Error::other(format!(
                "Cannot start lossless video encoder: {e}. Install ffmpeg or set TACIT_FFMPEG."
            ))
        })?;
        if !result.status.success() {
            let _ = std::fs::remove_file(output);
            return Err(io::Error::other(format!(
                "Video export failed: {}",
                String::from_utf8_lossy(&result.stderr)
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(output, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(temp);
    result
}
