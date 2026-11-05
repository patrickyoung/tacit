//! Opt-in live check of concept labels and incremental upgrades; synthetic content only.
use tacit::{model::Session, organizer::CloudConfig};
fn main() -> Result<(), String> {
    let config = CloudConfig::from_env()?.ok_or("OpenRouter is not configured")?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        let mut session = Session::default();
        for words in [
            "We aren't planning to remove every meeting. We only want fewer routine status calls.",
            "People cannot tell who owns the release decision, so everything waits for another meeting.",
            "Perhaps a written update could replace the weekly status call.",
            "Giving one person ownership of release decisions might unblock us.",
        ] { session.add(words); }
        let originals: Vec<_> = session.thoughts.iter().map(|t| t.text.clone()).collect();
        config.organize(&session,true).await?.apply(&mut session)?;
        for (t, original) in session.thoughts.iter().zip(&originals) {
            if &t.text != original || t.concept.is_none() { return Err("Concept generation lost source text or omitted a phrase".into()); }
            tacit::model::validate_concept(t.map_label())?;
            println!("{}: {}",session.placement(t),t.map_label());
        }
        if session.thoughts[0].category != session.thoughts[2].category || session.thoughts[1].category != session.thoughts[3].category || session.thoughts[0].category == session.thoughts[1].category {
            return Err("Concepts were not clustered by their distinct meanings".into());
        }
        let retained = session.thoughts[0].concept.clone();
        session.thoughts[1].concept = None;
        session.thoughts[3].concept = None;
        config.organize(&session,false).await?.apply(&mut session)?;
        if session.thoughts.iter().any(|t|t.concept.is_none()) || session.thoughts[0].concept != retained || session.thoughts.iter().map(|t|t.text.clone()).collect::<Vec<_>>() != originals {
            return Err("Incremental concept upgrade failed to preserve existing content".into());
        }
        println!("PASS: concise concepts, contextual clusters, exact originals, incremental upgrade");
        Ok(())
    })
}
