//! What the user already knows, kept beside the notebook as small linked notes and shown to the
//! map agent a level at a time.
//!
//! A file dropped into the knowledge base is kept whole, read into text, and filed by the
//! [`librarian`] as notes that each hold one idea: a zettelkasten. The notes are markdown files
//! in `notes/`, linked as `[[id|Title]]`, so the folder can be read and edited with any notes
//! tool, and a note written by hand is taken in like the rest.
//!
//! No turn reads all of it. The levels, each loaded only when the one before calls for it:
//! the index of topics ([`Library::index`]), the cards of the notes nearest a map
//! ([`Library::nearest`], [`Library::card`]), and whole notes with the cards of what they link
//! to ([`Library::open`]), which the steward and a directed turn ask for by name.
//!
//! Knowledge tells the agent how a subject is organised. It is never put on a map.
use crate::timeline;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{fs, io};

pub mod librarian;

/// Notes one consultation opens in full, and the cards it shows of one topic.
const OPENED: usize = 6;
const TOPIC_CARDS: usize = 15;
/// Characters of a note's text shown when it is opened: about four hundred words.
const BODY: usize = 2_400;
/// Topics the index lists by name.
const TOPICS: usize = 24;
/// A note file larger than this was not written as a note.
const MAX_NOTE: u64 = 256 * 1024;

/// One idea, written to stand alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Note {
    /// Its file name in `notes/`, and what links and the map agent call it.
    pub id: String,
    pub title: String,
    /// The subject area it belongs to, shared with other notes.
    pub topic: String,
    pub keywords: Vec<String>,
    /// The whole idea in one sentence: all a card shows of the note.
    pub gist: String,
    pub body: String,
    /// The ids of the notes this one points to.
    pub links: Vec<String>,
    /// The name of the file it was filed from, and that file as kept in `sources/`.
    pub source: String,
    pub asset: String,
    /// When it was filed, in seconds since 1970.
    pub created: u64,
}

/// The knowledge base beside a notebook: `session.knowledge` beside `session.json`.
/// `TACIT_KNOWLEDGE` names another folder, or turns the knowledge base `off`.
pub fn root(base: &Path) -> Option<PathBuf> {
    match std::env::var_os("TACIT_KNOWLEDGE") {
        Some(off) if off.eq_ignore_ascii_case("off") => None,
        Some(path) if !path.is_empty() => Some(path.into()),
        _ => Some(base.with_extension("knowledge")),
    }
}

/// "1 note", "3 notes".
fn count(n: usize, of: &str) -> String {
    format!("{n} {of}{}", if n == 1 { "" } else { "s" })
}
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn clip(text: &str, max: usize) -> String {
    let flat = flat(text);
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max).collect();
    cut.push('…');
    cut
}
/// A note's id from whatever names it: `[[k3fa2c|Title]]`, `[k3fa2c]`, a file's name.
pub fn id_of(text: &str) -> String {
    let named = text.trim().trim_start_matches('[');
    let named = named.split(['|', ']', '#']).next().unwrap_or_default();
    let mut out = String::new();
    for c in named.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let end = out.char_indices().nth(48).map_or(out.len(), |(at, _)| at);
    out.truncate(end);
    out.trim_end_matches('-').to_owned()
}
fn links_in(text: &str, out: &mut Vec<String>) {
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let id = id_of(&after[..end]);
        if !id.is_empty() && !out.contains(&id) {
            out.push(id);
        }
        rest = &after[end + 2..];
    }
}

impl Note {
    /// The note as its markdown file. `titled` names the notes it links to.
    fn page(&self, titled: &dyn Fn(&str) -> Option<String>) -> String {
        let mut out = format!(
            "---\nid: {}\ntitle: {}\ntopic: {}\nkeywords: {}\nsource: {}\nasset: {}\ncreated: {}\n---\n\n# {}\n\n> {}\n\n{}\n",
            self.id,
            flat(&self.title),
            flat(&self.topic),
            self.keywords.join(", "),
            flat(&self.source),
            self.asset,
            self.created,
            flat(&self.title),
            flat(&self.gist),
            self.body.trim()
        );
        if !self.links.is_empty() {
            let links: Vec<String> = self
                .links
                .iter()
                .map(|id| match titled(id) {
                    // A `|` or `]` in a title would end the link early.
                    Some(title) => format!("[[{id}|{}]]", title.replace(['|', '[', ']'], " ")),
                    None => format!("[[{id}]]"),
                })
                .collect();
            let _ = writeln!(out, "\nSee also: {}", links.join(" · "));
        }
        out
    }
    /// A note from its file, however much of the layout a hand has changed. `stem` is the
    /// file's name, which names the note when nothing inside does.
    fn parse(stem: &str, raw: &str) -> Self {
        let raw = raw.replace("\r\n", "\n");
        let mut note = Note {
            id: id_of(stem),
            ..Default::default()
        };
        let mut rest = raw.as_str();
        let head = rest
            .strip_prefix("---\n")
            .and_then(|r| r.split_once("\n---"));
        if let Some((head, after)) = head {
            for line in head.lines() {
                let Some((key, value)) = line.split_once(':') else {
                    continue;
                };
                let value = value.trim();
                match key.trim() {
                    "id" if !id_of(value).is_empty() => note.id = id_of(value),
                    "title" => note.title = value.into(),
                    "topic" => note.topic = value.into(),
                    "keywords" | "tags" => {
                        let words = value.trim_matches(['[', ']']).split(',');
                        note.keywords = words.map(flat).filter(|w| !w.is_empty()).collect();
                    }
                    "source" => note.source = value.into(),
                    "asset" => note.asset = value.into(),
                    "created" => note.created = value.parse().unwrap_or_default(),
                    _ => {}
                }
            }
            rest = after.split_once('\n').map_or("", |(_, body)| body);
        }
        let (mut body, mut gist, mut started) = (vec![], vec![], false);
        for line in rest.lines() {
            let text = line.trim();
            if !started && text.is_empty() {
                continue;
            }
            let heading = !started && body.is_empty() && gist.is_empty();
            if heading && let Some(title) = text.strip_prefix("# ") {
                if note.title.is_empty() {
                    note.title = flat(title);
                }
                continue;
            }
            if !started && let Some(quoted) = text.strip_prefix('>') {
                gist.push(quoted.trim());
                continue;
            }
            started = true;
            if let Some(links) = text.strip_prefix("See also:") {
                links_in(links, &mut note.links);
                continue;
            }
            body.push(line);
        }
        note.body = body.join("\n").trim().to_owned();
        links_in(&note.body, &mut note.links);
        note.gist = flat(&gist.join(" "));
        if note.title.is_empty() {
            note.title = stem.trim().to_owned();
        }
        if note.gist.is_empty() {
            // The first sentence, as near as punctuation tells.
            let flat = flat(&note.body);
            let end = flat.find(". ").map_or(flat.len(), |at| at + 1);
            note.gist = clip(&flat[..end], 220);
        }
        let own = note.id.clone();
        note.links.retain(|id| *id != own);
        note
    }
}

/// Words that say nothing about what a note or a map is about.
const STOP: &[&str] = &[
    "about",
    "above",
    "after",
    "again",
    "all",
    "also",
    "and",
    "any",
    "are",
    "because",
    "been",
    "before",
    "being",
    "below",
    "between",
    "both",
    "but",
    "can",
    "cannot",
    "could",
    "did",
    "does",
    "doing",
    "done",
    "down",
    "during",
    "each",
    "either",
    "else",
    "even",
    "every",
    "few",
    "for",
    "from",
    "further",
    "get",
    "gets",
    "got",
    "had",
    "has",
    "have",
    "having",
    "her",
    "here",
    "hers",
    "him",
    "his",
    "how",
    "however",
    "into",
    "its",
    "just",
    "less",
    "like",
    "made",
    "make",
    "makes",
    "many",
    "may",
    "maybe",
    "might",
    "more",
    "most",
    "much",
    "must",
    "need",
    "needs",
    "nor",
    "not",
    "now",
    "off",
    "often",
    "once",
    "one",
    "only",
    "onto",
    "other",
    "others",
    "our",
    "ours",
    "out",
    "over",
    "own",
    "per",
    "really",
    "same",
    "say",
    "says",
    "said",
    "she",
    "should",
    "since",
    "some",
    "something",
    "still",
    "such",
    "than",
    "that",
    "the",
    "their",
    "theirs",
    "them",
    "then",
    "there",
    "these",
    "they",
    "thing",
    "things",
    "think",
    "this",
    "those",
    "though",
    "through",
    "too",
    "two",
    "under",
    "until",
    "upon",
    "use",
    "used",
    "uses",
    "using",
    "very",
    "want",
    "wants",
    "was",
    "way",
    "ways",
    "well",
    "were",
    "what",
    "when",
    "where",
    "whether",
    "which",
    "while",
    "who",
    "whom",
    "whose",
    "why",
    "will",
    "with",
    "within",
    "without",
    "would",
    "yet",
    "you",
    "your",
    "yours",
];
/// A word as it is matched: "prices", "priced" and "pricing" all meet "price". Crude, and
/// only for English endings; other words are matched as written.
fn stem(mut word: String) -> String {
    if !word.is_ascii() || word.len() < 4 {
        return word;
    }
    if word.ends_with("ies") {
        word.truncate(word.len() - 3);
        word.push('y');
    } else if word.ends_with("sses") {
        word.truncate(word.len() - 2);
    } else if word.ends_with('s') && !["ss", "us", "is"].iter().any(|end| word.ends_with(end)) {
        word.pop();
    }
    if word.len() >= 6 && word.ends_with("ing") {
        word.truncate(word.len() - 3);
    } else if word.len() >= 5 && word.ends_with("ed") {
        word.truncate(word.len() - 2);
    }
    if word.len() >= 4 && word.ends_with('e') {
        word.pop();
    }
    word
}
/// The words of a text that say what it is about, as they are matched.
fn terms(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| {
            word.len() >= 3 || word.bytes().all(|b| b.is_ascii_digit()) && word.len() == 2
        })
        .map(str::to_lowercase)
        .filter(|word| !STOP.contains(&word.as_str()))
        .map(stem)
}

/// Where a word stands in one note: how much it weighs there, and whether it is in what the
/// note is called (title, keywords or topic) and not only in what it says.
#[derive(Clone, Debug)]
struct Posting {
    note: usize,
    weight: f32,
    key: bool,
}

/// Every note, with what is needed to find the ones near a text. Cheap to share: a turn is
/// given a snapshot, and filing replaces it.
#[derive(Clone, Debug, Default)]
pub struct Library {
    notes: Vec<Note>,
    by_id: BTreeMap<String, usize>,
    words: BTreeMap<String, Vec<Posting>>,
    /// Each note's length in weighted words, and their mean.
    lengths: Vec<f32>,
    mean: f32,
}
impl Library {
    /// The notes in a folder. One with nothing in it, or that is not there, is an empty
    /// knowledge base.
    pub fn load(root: &Path) -> Self {
        let mut notes = vec![];
        for entry in fs::read_dir(root.join("notes"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            let stem = path.file_stem().and_then(|stem| stem.to_str());
            let markdown = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"));
            let small = entry
                .metadata()
                .is_ok_and(|m| m.is_file() && m.len() <= MAX_NOTE);
            let (Some(stem), true, true) = (stem, markdown, small) else {
                continue;
            };
            let Ok(raw) = fs::read_to_string(&path) else {
                continue;
            };
            let note = Note::parse(stem, &raw);
            if !note.id.is_empty() && !(note.body.is_empty() && note.gist.is_empty()) {
                notes.push(note);
            }
        }
        Self::of(notes)
    }
    /// A knowledge base of these notes, oldest first. Of two with one id the first is kept,
    /// and links that lead nowhere are dropped.
    pub fn of(mut notes: Vec<Note>) -> Self {
        notes.sort_by(|a, b| (a.created, &a.id).cmp(&(b.created, &b.id)));
        let mut by_id = BTreeMap::new();
        notes.retain(|note| {
            let new = !by_id.contains_key(&note.id);
            if new {
                by_id.insert(note.id.clone(), by_id.len());
            }
            new
        });
        let mut library = Self {
            by_id,
            ..Default::default()
        };
        for (at, note) in notes.iter_mut().enumerate() {
            note.links.retain(|id| library.by_id.contains_key(id));
            let fields = [
                (note.title.clone(), 3.0, true),
                (note.keywords.join(" "), 3.0, true),
                (note.topic.clone(), 2.0, true),
                (note.gist.clone(), 2.0, false),
                (note.body.clone(), 1.0, false),
            ];
            let mut held: BTreeMap<String, (f32, bool)> = BTreeMap::new();
            for (text, weight, key) in fields {
                for term in terms(&text) {
                    let entry = held.entry(term).or_default();
                    entry.0 += weight;
                    entry.1 |= key;
                }
            }
            library.lengths.push(held.values().map(|(w, _)| w).sum());
            for (term, (weight, key)) in held {
                let posting = Posting {
                    note: at,
                    weight,
                    key,
                };
                library.words.entry(term).or_default().push(posting);
            }
        }
        let count = notes.len().max(1) as f32;
        library.mean = (library.lengths.iter().sum::<f32>() / count).max(1.0);
        library.notes = notes;
        library
    }
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }
    pub fn len(&self) -> usize {
        self.notes.len()
    }
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }
    pub fn get(&self, id: &str) -> Option<&Note> {
        self.by_id.get(id).map(|&at| &self.notes[at])
    }
    /// A file with these contents was filed before.
    pub fn has_source(&self, asset: &str) -> bool {
        self.notes.iter().any(|note| note.asset == asset)
    }
    /// The topics, largest first, each with its notes.
    pub fn topics(&self) -> Vec<(&str, Vec<&Note>)> {
        let mut topics: Vec<(&str, Vec<&Note>)> = vec![];
        for note in &self.notes {
            let same = |(name, _): &(&str, Vec<&Note>)| name.eq_ignore_ascii_case(&note.topic);
            match topics.iter().position(same) {
                Some(at) => topics[at].1.push(note),
                None => topics.push((&note.topic, vec![note])),
            }
        }
        topics.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
        topics
    }
    /// A topic's name as the knowledge base spells it.
    pub fn topic(&self, name: &str) -> Option<&str> {
        let same = |note: &&Note| note.topic.eq_ignore_ascii_case(name.trim());
        self.notes.iter().find(same).map(|note| note.topic.as_str())
    }

    /// Write `notes` into the folder and take them in.
    pub fn add(&mut self, root: &Path, notes: Vec<Note>) -> io::Result<()> {
        timeline::private_dir(root)?;
        let folder = root.join("notes");
        timeline::private_dir(&folder)?;
        let titled = |id: &str| {
            let new = notes.iter().find(|note| note.id == id);
            new.or_else(|| self.get(id)).map(|note| note.title.clone())
        };
        for note in &notes {
            let path = folder.join(format!("{}.md", note.id));
            timeline::atomic_write(&path, note.page(&titled).as_bytes())?;
        }
        let mut all = std::mem::take(&mut self.notes);
        all.extend(notes);
        *self = Self::of(all);
        Ok(())
    }

    /// The notes nearest a text, nearest first: at most `limit`, and none that only happen to
    /// share a word with it. `query` is the text in parts, each with how much its words count.
    ///
    /// Ranked by BM25 over each note's title, keywords, topic, gist and text, the first three
    /// counting for more.
    pub fn nearest(&self, query: &[(f32, &str)], limit: usize) -> Vec<&Note> {
        let mut asked: BTreeMap<String, f32> = BTreeMap::new();
        for (weight, text) in query {
            for term in terms(text) {
                let held = asked.entry(term).or_default();
                *held = held.max(*weight);
            }
        }
        let count = self.notes.len() as f32;
        // Per note: its score, the query words it holds, and those it is called by.
        let mut found: BTreeMap<usize, (f32, usize, usize)> = BTreeMap::new();
        for (term, weight) in &asked {
            let Some(postings) = self.words.get(term) else {
                continue;
            };
            let held = postings.len() as f32;
            let rarity = (1.0 + (count - held + 0.5) / (held + 0.5)).ln();
            for posting in postings {
                let length = self.lengths[posting.note] / self.mean;
                let often = posting.weight * 2.2 / (posting.weight + 1.2 * (0.25 + 0.75 * length));
                let entry = found.entry(posting.note).or_default();
                entry.0 += weight * rarity * often;
                entry.1 += 1;
                entry.2 += usize::from(posting.key);
            }
        }
        // One shared word is chance. A note is near when the text uses two of the words it
        // is called by, or one of those and more of what it says; a text of a word or two is
        // a name being looked up, and is matched whole.
        let brief = asked.len() < 3;
        let mut near: Vec<(usize, f32)> = found
            .into_iter()
            .filter(|(_, (_, held, key))| match brief {
                true => *held == asked.len(),
                false => *key >= 2 || (*key >= 1 && *held >= 3),
            })
            .map(|(at, (score, ..))| (at, score))
            .collect();
        near.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let best = near.first().map_or(0.0, |(_, score)| *score);
        near.retain(|(_, score)| *score >= best * 0.25);
        near.truncate(limit);
        near.into_iter().map(|(at, _)| &self.notes[at]).collect()
    }
    /// The notes that point to this one.
    pub fn linked_from(&self, id: &str) -> Vec<&Note> {
        let points = |note: &&Note| note.links.iter().any(|link| link == id);
        self.notes.iter().filter(points).collect()
    }

    /// The first level: what the knowledge base holds, by topic.
    pub fn index(&self) -> String {
        let topics = self.topics();
        let held = (count(self.len(), "note"), count(topics.len(), "topic"));
        let mut out = format!("{} in {}:\n", held.0, held.1);
        for (name, notes) in topics.iter().take(TOPICS) {
            // The keywords most of its notes share say what the topic covers.
            let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
            for word in notes.iter().flat_map(|note| &note.keywords) {
                *counted.entry(word).or_default() += 1;
            }
            let mut words: Vec<(&str, usize)> = counted.into_iter().collect();
            words.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let words: Vec<&str> = words.iter().take(5).map(|(word, _)| *word).collect();
            let _ = writeln!(out, "- {name} ({}): {}", notes.len(), words.join(", "));
        }
        if topics.len() > TOPICS {
            let _ = writeln!(out, "- and {} smaller topics", topics.len() - TOPICS);
        }
        out
    }
    /// The second level: one line of a note, enough to tell whether it bears on a map.
    pub fn card(&self, note: &Note) -> String {
        let mut out = format!("[{}] {}", note.id, flat(&note.title));
        if !note.topic.is_empty() {
            let _ = write!(out, " · {}", flat(&note.topic));
        }
        let _ = write!(out, ": {}", clip(&note.gist, 220));
        if !note.links.is_empty() {
            let links: Vec<&str> = note.links.iter().take(4).map(String::as_str).collect();
            let _ = write!(out, " · see {}", links.join(", "));
        }
        out
    }
    /// The third level: the whole note, and the cards of the notes it is linked with.
    pub fn full(&self, note: &Note) -> String {
        let mut out = format!("[{}] {}\n", note.id, flat(&note.title));
        let _ = write!(out, "Topic: {}", flat(&note.topic));
        if !note.source.is_empty() {
            let _ = write!(out, " · from {}", flat(&note.source));
        }
        let _ = writeln!(out, "\n{}", clip(&note.body, BODY));
        let to = note.links.iter().filter_map(|id| self.get(id));
        let from = self.linked_from(&note.id);
        for (heading, notes) in [("Links to", to.collect()), ("Linked from", from)] {
            let notes: Vec<&Note> = notes;
            if !notes.is_empty() {
                let _ = writeln!(out, "{heading}:");
                for linked in notes.iter().take(6) {
                    let _ = writeln!(out, "  {}", self.card(linked));
                }
            }
        }
        out
    }
    /// What a turn asked to read: notes by id or title, in full, and topics by name, as the
    /// cards of their notes. `seen` holds what the turn was shown before, and takes what it
    /// is shown now. Returns the text and the ids of the notes opened.
    pub fn open(&self, wanted: &[String], seen: &mut BTreeSet<String>) -> (String, Vec<String>) {
        let (mut out, mut opened) = (String::new(), vec![]);
        for name in wanted
            .iter()
            .map(|name| flat(name))
            .filter(|n| !n.is_empty())
        {
            let titled = |note: &&Note| note.title.eq_ignore_ascii_case(&name);
            // A topic is asked for by its name, which may also be the id of no note.
            let note = match self.topic(&name) {
                Some(_) if self.get(&id_of(&name)).is_none() => None,
                _ => self
                    .get(&id_of(&name))
                    .or_else(|| self.notes.iter().find(titled)),
            };
            if let Some(note) = note {
                if !seen.insert(note.id.clone()) {
                    let _ = writeln!(out, "[{}] is open above.\n", note.id);
                } else if opened.len() >= OPENED {
                    seen.remove(&note.id);
                    let _ = writeln!(out, "[{}] was not opened: {OPENED} at a time.\n", note.id);
                } else {
                    let _ = writeln!(out, "{}", self.full(note));
                    opened.push(note.id.clone());
                }
            } else if let Some(topic) = self.topic(&name) {
                let notes = self
                    .notes
                    .iter()
                    .filter(|n| n.topic.eq_ignore_ascii_case(topic));
                let notes: Vec<&Note> = notes.collect();
                let _ = writeln!(out, "TOPIC {topic} ({})", count(notes.len(), "note"));
                for note in notes.iter().take(TOPIC_CARDS) {
                    let _ = writeln!(out, "{}", self.card(note));
                }
                if notes.len() > TOPIC_CARDS {
                    let _ = writeln!(out, "and {} more", notes.len() - TOPIC_CARDS);
                }
                out.push('\n');
            } else {
                let _ = writeln!(
                    out,
                    "The knowledge base holds no note or topic called {name:?}.\n"
                );
            }
        }
        (out, opened)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: &str, title: &str, topic: &str, keywords: &[&str], body: &str) -> Note {
        Note {
            id: id.into(),
            title: title.into(),
            topic: topic.into(),
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            gist: format!("{title}."),
            body: body.into(),
            ..Default::default()
        }
    }
    fn library() -> Library {
        let mut flow = note(
            "k00001",
            "Small batches shorten feedback loops",
            "Delivery flow",
            &["batch size", "feedback", "lead time"],
            "Work released in small batches reaches users sooner, so problems show while they are cheap to fix.",
        );
        flow.links = vec!["k00002".into(), "gone".into()];
        let limits = note(
            "k00002",
            "Limits on work in progress expose bottlenecks",
            "Delivery flow",
            &["wip limit", "bottleneck", "kanban"],
            "A cap on work in progress makes a queue visible where a team is overloaded.",
        );
        let price = note(
            "k00003",
            "Anchoring sets what a price is compared with",
            "Pricing",
            &["anchoring", "price", "tiers"],
            "The first price a buyer sees becomes the reference for every later one.",
        );
        let sourdough = note(
            "k00004",
            "A sourdough starter needs feeding every day",
            "Baking",
            &["sourdough", "starter", "flour"],
            "Fed with flour and water each day, the starter stays active. Time and warmth do the rest.",
        );
        Library::of(vec![flow, limits, price, sourdough])
    }
    fn ids(notes: &[&Note]) -> Vec<String> {
        notes.iter().map(|note| note.id.clone()).collect()
    }

    #[test]
    fn a_note_survives_its_file() {
        let mut written = note(
            "k3fa2c",
            "Small batches",
            "Delivery flow",
            &["batch size", "flow"],
            "First paragraph.\n\nSecond, with a [[k00002|Limits]] link.",
        );
        written.gist = "Smaller batches mean faster feedback.".into();
        written.links = vec!["k00002".into(), "k00009".into()];
        written.source = "lean notes.pdf".into();
        written.asset = "abc.pdf".into();
        written.created = 1_794_000_000;
        let page = written.page(&|id| (id == "k00002").then(|| "Limits | on [work]".into()));
        assert!(
            page.starts_with("---\nid: k3fa2c\ntitle: Small batches\n"),
            "{page}"
        );
        assert!(page.contains("\n# Small batches\n\n> Smaller batches mean faster feedback.\n"));
        assert!(
            page.ends_with("\nSee also: [[k00002|Limits   on  work ]] · [[k00009]]\n"),
            "{page}"
        );
        assert_eq!(Note::parse("k3fa2c", &page), written);
        // Windows line endings, as an editor there would save it.
        assert_eq!(Note::parse("k3fa2c", &page.replace('\n', "\r\n")), written);
    }
    #[test]
    fn a_note_written_by_hand_is_taken_as_it_is() {
        let by_hand = Note::parse(
            "Team Topologies",
            "# Stream-aligned teams\n\nA team aligned to one flow of work. It links to [[Platform teams]] and [[k00001|batches]].\nIt owns what it builds.\n",
        );
        assert_eq!(by_hand.id, "team-topologies");
        assert_eq!(by_hand.title, "Stream-aligned teams");
        assert_eq!(by_hand.gist, "A team aligned to one flow of work.");
        assert_eq!(by_hand.links, ["platform-teams", "k00001"]);
        let bare = Note::parse("idea", "just a line");
        assert_eq!(
            (bare.title.as_str(), bare.body.as_str()),
            ("idea", "just a line")
        );
        let tagged = Note::parse(
            "x",
            "---\ntags: [flow, lean thinking]\ntopic: Flow\n---\nBody.",
        );
        assert_eq!(tagged.keywords, ["flow", "lean thinking"]);
        assert_eq!(tagged.topic, "Flow");
        assert_eq!(id_of("[[Über uns|x]]"), "über-uns");
    }
    #[test]
    fn notes_are_kept_in_a_folder_and_found_again() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("session.knowledge");
        assert!(
            Library::load(&root).is_empty(),
            "no folder is an empty knowledge base"
        );
        let mut kept = Library::default();
        kept.add(&root, library().notes().to_vec()).unwrap();
        assert_eq!(kept.len(), 4);
        let page = fs::read_to_string(root.join("notes/k00001.md")).unwrap();
        assert!(
            page.contains("See also: [[k00002|Limits on work in progress expose bottlenecks]]"),
            "{page}"
        );
        fs::write(
            root.join("notes/mine.md"),
            "# My own note\n\nKanban boards, see [[k00002]].",
        )
        .unwrap();
        fs::write(root.join("notes/readme.txt"), "not a note").unwrap();
        fs::write(root.join("notes/empty.md"), "").unwrap();
        let found = Library::load(&root);
        assert_eq!(found.len(), 5);
        assert_eq!(found.get("k00001").unwrap(), kept.get("k00001").unwrap());
        assert_eq!(
            found.get("k00001").unwrap().links,
            ["k00002"],
            "a link to nothing is dropped"
        );
        assert_eq!(ids(&found.linked_from("k00002")), ["k00001", "mine"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&root.join("notes")), 0o700);
            assert_eq!(mode(&root.join("notes/k00001.md")), 0o600);
        }
    }
    #[test]
    fn words_meet_in_their_stems() {
        let stems = |text: &str| terms(text).collect::<Vec<_>>();
        assert_eq!(stems("Prices, priced and pricing the price"), ["pric"; 4]);
        assert_eq!(
            stems("batches of meetings in 3 classes"),
            ["batch", "meet", "class"]
        );
        assert_eq!(
            stems("WIP limits; Q3 2026 is 40% of it"),
            ["wip", "limit", "2026", "40"]
        );
        assert_eq!(stems("Über Größe"), ["über", "größe"]);
    }
    #[test]
    fn the_nearest_notes_are_those_a_text_is_about() {
        let library = library();
        let near = |text: &str| ids(&library.nearest(&[(1.0, text)], 5));
        assert_eq!(
            near("Our batches are too big and feedback from users takes weeks"),
            ["k00001"]
        );
        assert_eq!(
            near(
                "We keep hitting a bottleneck; a WIP limit on the kanban board might show the queue"
            ),
            ["k00002"]
        );
        assert_eq!(
            near("pricing tiers"),
            ["k00003"],
            "a name looked up is matched whole"
        );
        assert_eq!(near("pricing sourdough"), [] as [&str; 0]);
        assert_eq!(
            near("It takes time to do the work each day"),
            [] as [&str; 0],
            "words shared by chance bring no note"
        );
        assert_eq!(near(""), [] as [&str; 0]);
        // What a part counts for decides the order.
        let both = [
            (1.0, "batch size and feedback"),
            (3.0, "wip limit bottleneck"),
        ];
        assert_eq!(ids(&library.nearest(&both, 5)), ["k00002", "k00001"]);
        assert_eq!(ids(&library.nearest(&both, 1)), ["k00002"]);
        assert!(Library::default().nearest(&both, 5).is_empty());
    }
    #[test]
    fn each_level_shows_more_of_less() {
        let library = library();
        assert_eq!(
            library.index(),
            "4 notes in 3 topics:\n\
             - Delivery flow (2): batch size, bottleneck, feedback, kanban, lead time\n\
             - Baking (1): flour, sourdough, starter\n\
             - Pricing (1): anchoring, price, tiers\n"
        );
        let flow = library.get("k00001").unwrap();
        assert_eq!(
            library.card(flow),
            "[k00001] Small batches shorten feedback loops · Delivery flow: Small batches shorten feedback loops. · see k00002"
        );
        let full = library.full(library.get("k00002").unwrap());
        assert!(full.starts_with("[k00002] Limits on work in progress expose bottlenecks\nTopic: Delivery flow\nA cap on work"), "{full}");
        assert!(full.ends_with("Linked from:\n  [k00001] Small batches shorten feedback loops · Delivery flow: Small batches shorten feedback loops. · see k00002\n"), "{full}");
    }
    #[test]
    fn a_turn_opens_notes_by_id_or_title_and_topics_by_name() {
        let library = library();
        let mut seen = BTreeSet::new();
        let wanted = [
            "[k00001]".into(),
            "pricing".into(),
            "A sourdough starter needs feeding every day".into(),
            "k99999".into(),
        ];
        let (text, opened) = library.open(&wanted, &mut seen);
        assert_eq!(opened, ["k00001", "k00004"]);
        assert!(
            text.contains(
                "[k00001] Small batches shorten feedback loops\nTopic: Delivery flow\nWork released"
            ),
            "{text}"
        );
        assert!(
            text.contains("TOPIC Pricing (1 note)\n[k00003] Anchoring sets"),
            "{text}"
        );
        assert!(
            text.contains("holds no note or topic called \"k99999\""),
            "{text}"
        );
        let (again, opened) = library.open(&["k00001".into(), "k00002".into()], &mut seen);
        assert_eq!(opened, ["k00002"]);
        assert!(again.starts_with("[k00001] is open above."), "{again}");
    }
    #[test]
    fn the_knowledge_base_is_beside_the_notebook() {
        // The variable is read, never set: tests run side by side.
        if std::env::var_os("TACIT_KNOWLEDGE").is_none() {
            let base = Path::new("/data/tacit/session.json");
            assert_eq!(
                root(base).unwrap(),
                Path::new("/data/tacit/session.knowledge")
            );
        }
    }
}
