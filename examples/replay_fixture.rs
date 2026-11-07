//! Generate an isolated, synthetic session for offline replay QA. Never opens a microphone.
use tacit::{
    art::Garden,
    model::Session,
    timeline::{Audio, Cue, Event, Journal, Scene},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("Pass a new output directory")?,
    );
    std::fs::create_dir(&root)?;
    let mut j = Journal::open(&root.join("session.json"), Session::default())?;
    let wav_path = root.join("fixture.wav");
    let mut wav = hound::WavWriter::create(
        &wav_path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for _ in 0..32000 {
        wav.write_sample(0i16)?;
    }
    wav.finalize()?;
    let asset = j.import(&wav_path, "wav")?;
    j.append(
        3000,
        Event::AudioSaved {
            audio: Audio {
                id: "fixture".into(),
                asset,
                start_ms: 1000,
                duration_ms: 2000,
                sample_rate: 16000,
                samples: 32000,
            },
        },
    )?;
    j.append(
        3500,
        Event::Transcript {
            recording: "fixture".into(),
            text: "Synthetic caption for replay testing.".into(),
            cues: vec![Cue {
                start_ms: 250,
                end_ms: 1750,
                text: "Synthetic caption for replay testing.".into(),
            }],
        },
    )?;
    let session = Session::demo();
    let mut scene = Scene {
        garden: Garden::settled(&session),
        session,
        organizer_status: "Synthetic replay fixture · no model calls".into(),
        ..Scene::default()
    };
    if let Some(path) = std::env::args().nth(2) {
        let asset = j.import(std::path::Path::new(&path), "png")?;
        scene.artwork.push(tacit::timeline::Artwork {
            mural: None,
            key: "lens-0".into(),
            fingerprint: "fixture-art".into(),
            slot: 0,
            asset,
            born: 4.0,
            retired: None,
        });
    }
    j.scene(5000, "demo thoughts", scene.clone())?;
    j.scene(
        10000,
        "selection",
        Scene {
            selected: Some(2),
            ..scene
        },
    )?;
    j.export_vtt()?;
    j.validate_assets()?;
    assert_eq!(j.at(10000)?, j.projection);
    println!("{}", j.root.display());
    Ok(())
}
