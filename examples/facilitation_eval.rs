//! Opt-in live-model checks using synthetic discussion only. Requires OpenRouter.
use tacit::{
    direction,
    model::{Category, Session},
    organizer::CloudConfig,
};
fn main() -> Result<(), String> {
    let config = CloudConfig::from_env()?.ok_or("OpenRouter is not configured")?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        let cases = [
            ("qualified tradeoff", "It is cheaper, but only if we accept the reliability risk.", 1),
            ("claim with example", "Meetings take all day. For example yesterday we had six. Separately, hosting costs doubled.", 2),
            ("independent proposals", "We could send updates beforehand. Another option is to shorten meetings.", 2),
            ("topic return", "The hiring pipeline is slow. On another topic, hosting costs doubled. Back to hiring, we could introduce paid work trials.", 3),
        ];
        if !std::env::args().any(|a| a == "--directions-only") {
        for (name,text,expected) in cases {
            let chunks = tacit::recording::split(&config,text,&Session::default()).await?;
            if chunks.concat() != text || chunks.len() != expected { return Err(format!("Segmentation case failed: {name}: expected {expected}, got {}",chunks.len())); }
            println!("PASS segmentation: {name}");
        }
        let mut discussion = Session::default();
        for thought in ["New hires wait weeks for an experienced buddy.", "Our database bill doubled after moving to the new cluster.", "Pairing each newcomer with a mentor could reduce that waiting.", "We should remove unused replicas to reduce the bill."] { discussion.add(thought); }
        config.organize(&discussion,true).await?.apply(&mut discussion)?;
        let t = &discussion.thoughts;
        if t[0].category != t[2].category || t[1].category != t[3].category || t[0].category == t[1].category {
            return Err("Classification failed to connect contextual references across topic returns".into());
        }
        println!("PASS classification: proposals follow their conceptual threads across topic switches");
        }
        let mut s = Session::default();
        s.add("Our hosting costs doubled."); s.add("Who owns release decisions?");
        s.categories = vec![Category {id:"costs".into(),name:"Costs".into(),motif:None,illustration:None},Category {id:"decisions".into(),name:"Decisions".into(),motif:None,illustration:None}];
        s.thoughts[0].category = Some("costs".into()); s.thoughts[1].category = Some("decisions".into());
        let instruction = "Rename Costs to Infrastructure costs and put Decisions before it. Keep all thoughts in their current branches.";
        let p = direction::plan(&config,&s,instruction).await?;
        let edited = direction::apply(&s,instruction,&p.edits)?;
        if edited.categories[0].id != "decisions" || edited.categories[1].name != "Infrastructure costs" || edited.thoughts != s.thoughts { return Err("Direction case did not preserve content and apply the requested branch edits".into()); }
        println!("PASS direction: rename and order without changing thoughts");
        let p = direction::plan(&config,&s,"Place the Costs branch exactly at pixel x=170, y=400 and animate a spinning 3D cube.").await?;
        if !p.edits.is_empty() { return Err("Unsupported layout request should explain its limitation without edits".into()); }
        println!("PASS direction: unsupported layout does not fabricate edits");
        Ok(())
    })
}
