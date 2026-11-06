//! A second renderer: consume a saved session without Rune scene/layout knowledge.
//! cargo run --example render_text -- /path/to/session.tacit [milliseconds] [--json]
use tacit::timeline::Journal;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .ok_or("Pass a session.json or session.tacit path")?;
    let journal = Journal::read(std::path::Path::new(path))?;
    let at = args
        .get(1)
        .filter(|a| *a != "--json")
        .map(|a| a.parse::<u64>())
        .transpose()?
        .unwrap_or(journal.duration());
    let projection = journal.at(at)?;
    let content = projection.content();
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&content)?);
        return Ok(());
    }
    println!("{}\n", content.session.title);
    for thought in &content.session.thoughts {
        println!(
            "[{}] {}: {}",
            thought.id,
            content.session.placement_path(thought),
            thought.map_label()
        );
        if thought.concept.is_some() {
            println!("    Original: {}", thought.text);
        }
    }
    if let Some(interpretation) = &content.session.interpretation {
        println!("\n{}", interpretation.narrative);
        for link in &interpretation.relationships {
            println!("[{}] → [{}]: {}", link.from, link.to, link.label);
        }
    }
    for art in content.artwork {
        if art.scope == tacit::timeline::ArtworkScope::Map {
            println!("Whole-map illustration: {}", art.asset);
        } else {
            println!("Illustration for {}: {}", art.theme, art.asset);
        }
    }
    for audio in content.audio {
        println!(
            "Audio {}: {}ms–{}ms ({})",
            audio.id,
            audio.start_ms,
            audio.start_ms + audio.duration_ms,
            audio.asset
        );
    }
    Ok(())
}
