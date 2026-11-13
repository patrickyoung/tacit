//! What the user already knows, kept beside the notebook as small linked notes and shown to the
//! map agent a level at a time.
//!
//! A file dropped into the knowledge base is kept whole, read into text, and filed by the
//! [`librarian`] as a zettelkasten. Three kinds of note are kept apart ([`Kind`]): a note that
//! holds one idea, the note on a source that says what a file is and how it runs, and the
//! overview of a topic that says how its ideas hang together. A link says why it is there.
//! The notes are markdown files in `notes/`, linked as `[[id|Title]]`, so the folder can be
//! read and edited with any notes tool, and a note written by hand is taken in like the rest.
//!
//! The librarian tends what it wrote: it merges, rewrites and re-sorts its own notes. A note
//! written or changed by hand is the user's ([`Note::hand`]) and is never rewritten. Every
//! page the librarian replaces or takes out is first copied to `history/`, and `changes.md`
//! says in plain words what was done ([`Library::apply`]).
//!
//! No turn reads all of it. The levels, each loaded only when the one before calls for it:
//! the index of topics ([`Library::index`]), the overview of a topic near a map
//! ([`Library::overview`]), the cards of the notes nearest it ([`Library::nearest`],
//! [`Library::card`]), and whole notes with the cards of what they link to
//! ([`Library::open`]), which the steward and a directed turn ask for by name.
//!
//! Knowledge tells the agent how a subject is organised. It is never put on a map.
//!
//! Reading goes the other way from filing, and is modelled on how a practised reader uses a
//! zettelkasten ([`research`]): come in by a topic and its overview, search in the notes' own
//! words ([`Library::find`]), read a note whole before relying on it, follow a link for the
//! reason it gives in either direction ([`Library::ties`]), and go back to the file a note
//! was taken from ([`Library::source_of`]).
use crate::timeline;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{fs, io};

pub mod librarian;
pub mod research;

/// Notes one consultation opens in full, the cards it shows of one topic, and the linked
/// notes it shows each way of a note opened.
const OPENED: usize = 6;
const LINKS: usize = 6;
const TOPIC_CARDS: usize = 15;
/// Characters of a note's text shown when it is opened: about four hundred words.
const BODY: usize = 2_400;
/// Topics the index lists by name.
const TOPICS: usize = 24;
/// A note file larger than this was not written as a note.
const MAX_NOTE: u64 = 256 * 1024;
/// Characters of the reason a link is there.
const WHY: usize = 160;
/// Characters of an overview shown where a turn did not ask for it.
const OVERVIEW: usize = 1_600;
/// Cards one search shows.
const FOUND: usize = 6;

/// What a note is there for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// One idea, written to stand alone.
    #[default]
    Idea,
    /// What one filed file is and how it runs, pointing to the ideas taken from it.
    Source,
    /// How the ideas of one topic hang together: where to start reading it.
    Overview,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Idea => "idea",
            Self::Source => "source",
            Self::Overview => "overview",
        }
    }
}

/// One note of the knowledge base.
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
    /// Why a link is there, by the id it leads to: "an example of it", "contradicts it: …".
    pub why: BTreeMap<String, String>,
    pub kind: Kind,
    /// Written or changed by hand. Such a note is the user's: the librarian reads it, links
    /// to it, and never rewrites it or takes it out.
    pub hand: bool,
    /// The name of the file it was filed from, and that file as kept in `sources/`.
    pub source: String,
    pub asset: String,
    /// When it was filed, in seconds since 1970.
    pub created: u64,
}

/// A note another is linked with, the way a reader follows it.
#[derive(Clone, Copy, Debug)]
pub struct Tie<'a> {
    pub note: &'a Note,
    /// Why the link is there, as the note that holds it says, or nothing.
    pub why: &'a str,
    /// The link leads from the note being read to this one, and not back to it.
    pub out: bool,
}
impl Tie<'_> {
    /// The two notes pull against each other: the librarian opens such a reason with
    /// "contradicts it".
    pub fn disputes(&self) -> bool {
        self.why
            .trim_start()
            .to_lowercase()
            .starts_with("contradict")
    }
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

/// What a page is sealed with: a hash of all of it but the seal. A page whose seal still
/// fits is as the librarian wrote it.
fn seal(page: &str) -> String {
    let lines = page.lines().filter(|line| !line.starts_with("seal:"));
    let sealed = lines.collect::<Vec<_>>().join("\n");
    blake3::hash(sealed.trim_end().as_bytes()).to_hex()[..16].to_owned()
}
/// A link's reason as written after it: "— an example of it".
fn reason(text: &str) -> String {
    let dashes = |c: char| c.is_whitespace() || ['—', '–', '-', ':', '·'].contains(&c);
    clip(text.trim_start_matches(dashes), WHY)
}

impl Note {
    /// The day it was filed, "2026-11-12", when that is known.
    pub fn filed(&self) -> Option<String> {
        (self.created > 0).then(|| date(self.created)[..10].to_owned())
    }
    /// The note as its markdown file, sealed as the librarian's own. `titled` names the notes
    /// it links to.
    fn page(&self, titled: &dyn Fn(&str) -> Option<String>) -> String {
        let mut out = format!(
            "---\nid: {}\ntitle: {}\ntopic: {}\nkeywords: {}\n",
            self.id,
            flat(&self.title),
            flat(&self.topic),
            self.keywords.join(", "),
        );
        if self.kind != Kind::Idea {
            let _ = writeln!(out, "kind: {}", self.kind.name());
        }
        let _ = write!(
            out,
            "source: {}\nasset: {}\ncreated: {}\n---\n\n# {}\n\n> {}\n\n{}\n",
            flat(&self.source),
            self.asset,
            self.created,
            flat(&self.title),
            flat(&self.gist),
            self.body.trim()
        );
        // A link the text makes is not listed again under it.
        let mut made = vec![];
        links_in(&self.body, &mut made);
        let listed = |id: &&String| !made.contains(id) || self.why.contains_key(*id);
        let listed: Vec<&String> = self.links.iter().filter(listed).collect();
        if !listed.is_empty() {
            out.push_str("\nSee also:\n");
        }
        for id in listed {
            match titled(id) {
                // A `|` or `]` in a title would end the link early.
                Some(title) => {
                    let _ = write!(out, "- [[{id}|{}]]", title.replace(['|', '[', ']'], " "));
                }
                None => {
                    let _ = write!(out, "- [[{id}]]");
                }
            }
            match self.why.get(id).map(|why| flat(why)) {
                Some(why) if !why.is_empty() => {
                    let _ = writeln!(out, " — {why}");
                }
                _ => out.push('\n'),
            }
        }
        let seal = seal(&out);
        out.replacen("\n---\n", &format!("\nseal: {seal}\n---\n"), 1)
    }
    /// A note from its file, however much of the layout a hand has changed. `stem` is the
    /// file's name, which names the note when nothing inside does.
    fn parse(stem: &str, raw: &str) -> Self {
        let raw = raw.replace("\r\n", "\n");
        let mut note = Note {
            id: id_of(stem),
            hand: true,
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
                    "kind" | "type" => {
                        note.kind = match value.to_lowercase().as_str() {
                            "source" | "literature" => Kind::Source,
                            "overview" | "structure" | "hub" | "moc" => Kind::Overview,
                            _ => Kind::Idea,
                        }
                    }
                    "source" => note.source = value.into(),
                    "asset" => note.asset = value.into(),
                    "created" => note.created = value.parse().unwrap_or_default(),
                    "seal" => note.hand = value != seal(&raw),
                    _ => {}
                }
            }
            rest = after.split_once('\n').map_or("", |(_, body)| body);
        }
        // `listing`: under a "See also:" heading, where each line is a link and why.
        let (mut body, mut gist, mut started, mut listing) = (vec![], vec![], false, false);
        let mut also = vec![];
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
                links_in(links, &mut also);
                listing = true;
                continue;
            }
            let item = text.strip_prefix(['-', '*']).map(str::trim_start);
            if listing && let Some(link) = item.and_then(|item| item.strip_prefix("[[")) {
                let (to, why) = link.split_once("]]").unwrap_or((link, ""));
                let (to, why) = (id_of(to), reason(why));
                if !to.is_empty() {
                    if !why.is_empty() {
                        note.why.insert(to.clone(), why);
                    }
                    if !also.contains(&to) {
                        also.push(to);
                    }
                }
                continue;
            }
            // A blank line in or after the list belongs to it, not to the text.
            if listing && text.is_empty() {
                continue;
            }
            listing = false;
            body.push(line);
        }
        note.body = body.join("\n").trim().to_owned();
        links_in(&note.body, &mut note.links);
        for id in also {
            if !note.links.contains(&id) {
                note.links.push(id);
            }
        }
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
        note.why.retain(|id, _| *id != own);
        note
    }
}

/// Changes to the knowledge base that are written together.
#[derive(Clone, Debug, Default)]
pub struct Changes {
    /// Notes that are new, or that replace the note of the same id.
    pub notes: Vec<Note>,
    /// The ids of notes taken out.
    pub retired: Vec<String>,
    /// What was done, a line each, in words the user can read.
    pub log: Vec<String>,
}
/// A time as the log gives it: "2026-11-12 14:03" in UTC.
fn date(seconds: u64) -> String {
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Days since 1970 as a date of the Gregorian calendar, counting eras of 400 years from
    // a first of March.
    let shifted = days + 719_468;
    let (era, day) = (shifted / 146_097, shifted % 146_097);
    let year = (day - day / 1_460 + day / 36_524 - day / 146_096) / 365;
    let of_year = day - (365 * year + year / 4 - year / 100);
    let month = (5 * of_year + 2) / 153;
    let (day, march) = (of_year - (153 * month + 2) / 5 + 1, month < 10);
    let month = if march { month + 3 } else { month - 9 };
    let year = year + era * 400 + u64::from(!march);
    let (hour, minute) = (rest / 3_600, rest % 3_600 / 60);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
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
            note.why.retain(|id, _| note.links.contains(id));
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

    /// The overview of a topic, when one was written.
    pub fn overview(&self, topic: &str) -> Option<&Note> {
        let of = |note: &&Note| {
            note.kind == Kind::Overview && note.topic.eq_ignore_ascii_case(topic.trim())
        };
        self.notes.iter().find(of)
    }
    /// The level between the index and the cards: how a topic's notes hang together, as its
    /// overview says it, or nothing when none was written.
    pub fn outline(&self, topic: &str) -> Option<String> {
        let overview = self.overview(topic)?;
        let text = clip(&overview.body, OVERVIEW);
        Some(format!("{}: {text}\n", flat(&overview.title)))
    }

    /// Write `notes` into the folder and take them in.
    pub fn add(&mut self, root: &Path, notes: Vec<Note>) -> io::Result<()> {
        let changes = Changes {
            notes,
            ..Default::default()
        };
        self.apply(root, changes, 0).map(|_| ())
    }
    /// Write a set of changes into the folder and take them in. A page that is replaced or
    /// taken out is first copied to `history/`, and what was done is added to `changes.md`.
    /// A page a hand has written or changed, as the folder holds it now, is left alone
    /// whatever `changes` says. Returns the ids of the notes that were left alone for that.
    pub fn apply(&mut self, root: &Path, changes: Changes, now: u64) -> io::Result<Vec<String>> {
        timeline::private_dir(root)?;
        let folder = root.join("notes");
        timeline::private_dir(&folder)?;
        let page = |id: &str| folder.join(format!("{id}.md"));
        // The page as it is on disk decides, not what was read when the app started: the
        // user may have edited it since.
        let mut spared = vec![];
        let mut old: BTreeMap<&str, String> = BTreeMap::new();
        let touched = changes.notes.iter().map(|note| note.id.as_str());
        for id in touched.chain(changes.retired.iter().map(String::as_str)) {
            let Ok(raw) = fs::read_to_string(page(id)) else {
                continue;
            };
            if Note::parse(id, &raw).hand {
                spared.push(id.to_owned());
            } else {
                old.insert(id, raw);
            }
        }
        if !old.is_empty() {
            let history = root.join("history");
            timeline::private_dir(&history)?;
            for (id, raw) in &old {
                let kept = history.join(format!("{id}-{now}.md"));
                timeline::atomic_write(&kept, raw.as_bytes())?;
            }
        }
        let retired = |id: &str| changes.retired.iter().any(|gone| gone == id);
        let written = |note: &&Note| !spared.contains(&note.id) && !retired(&note.id);
        let written: Vec<&Note> = changes.notes.iter().filter(written).collect();
        let titled = |id: &str| {
            let new = written.iter().find(|note| note.id == id).copied();
            new.or_else(|| self.get(id)).map(|note| note.title.clone())
        };
        for note in &written {
            timeline::atomic_write(&page(&note.id), note.page(&titled).as_bytes())?;
        }
        for id in changes.retired.iter().filter(|id| !spared.contains(id)) {
            match fs::remove_file(page(id)) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        if !changes.log.is_empty() {
            let path = root.join("changes.md");
            let mut log = fs::read_to_string(&path).unwrap_or_default();
            if log.is_empty() {
                log.push_str("# What the librarian changed\n\nEvery page it replaced or took out is kept in `history/`, named by the note and the time.\n\n");
            }
            for line in &changes.log {
                let _ = writeln!(log, "- {} · {}", date(now), flat(line));
            }
            timeline::atomic_write(&path, log.as_bytes())?;
        }
        let gone = |note: &Note| {
            let replaced = written.iter().any(|new| new.id == note.id);
            replaced || (retired(&note.id) && !spared.contains(&note.id))
        };
        let mut all: Vec<Note> = self.notes.iter().filter(|n| !gone(n)).cloned().collect();
        all.extend(written.into_iter().cloned());
        *self = Self::of(all);
        Ok(spared)
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
    /// The notes nearest some words someone is looking for, of every kind, nearest first.
    pub fn find(&self, words: &str, limit: usize) -> Vec<&Note> {
        self.nearest(&[(1.0, words)], limit)
    }
    /// Every note this one is linked with, each with the reason the note that holds the link
    /// gives: first the notes it leads to, then the notes that lead to it. A note linked both
    /// ways is given both ways, since each note says its own reason.
    pub fn ties<'a>(&'a self, note: &'a Note) -> Vec<Tie<'a>> {
        let reason = |holder: &'a Note, to: &str| holder.why.get(to).map_or("", String::as_str);
        let out = note
            .links
            .iter()
            .filter_map(|id| self.get(id))
            .map(|to| Tie {
                note: to,
                why: reason(note, &to.id),
                out: true,
            });
        let back = self.linked_from(&note.id).into_iter().map(|from| Tie {
            note: from,
            why: reason(from, &note.id),
            out: false,
        });
        out.chain(back).collect()
    }
    /// The note on the file this note was taken from.
    pub fn source_of(&self, note: &Note) -> Option<&Note> {
        let from = |other: &&Note| other.kind == Kind::Source && other.asset == note.asset;
        match note.kind == Kind::Source || note.asset.is_empty() {
            true => None,
            false => self.notes.iter().find(from),
        }
    }
    /// The ideas taken from the file a source note is about, in the order they were filed.
    pub fn taken_from(&self, source: &Note) -> Vec<&Note> {
        let taken = |note: &&Note| note.kind == Kind::Idea && note.asset == source.asset;
        match source.asset.is_empty() {
            true => vec![],
            false => self.notes.iter().filter(taken).collect(),
        }
    }

    /// A note's text as someone reads it: every `[[id|Title]]` as the title it names, and a
    /// bare `[[id]]` as the title of the note it leads to.
    pub fn prose(&self, text: &str) -> String {
        let (mut out, mut rest) = (String::new(), text);
        while let Some(start) = rest.find("[[") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("]]") else { break };
            out.push_str(&rest[..start]);
            let (id, title) = after[..end].split_once('|').unwrap_or((&after[..end], ""));
            let held = self.get(&id_of(id)).map(|note| note.title.as_str());
            out.push_str(match (title.trim(), held) {
                ("", Some(held)) => held,
                ("", None) => id.trim(),
                (title, _) => title,
            });
            rest = &after[end + 2..];
        }
        out + rest
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
            let overview = match self.overview(name) {
                Some(_) => ", with an overview",
                None => "",
            };
            // The overview is named beside the count, so it is not counted in it.
            let held = notes.iter().filter(|note| note.kind != Kind::Overview);
            let (held, words) = (held.count(), words.join(", "));
            let _ = writeln!(out, "- {name} ({held}{overview}): {words}");
        }
        if topics.len() > TOPICS {
            let _ = writeln!(out, "- and {} smaller topics", topics.len() - TOPICS);
        }
        out
    }
    /// The second level: one line of a note, enough to tell whether it bears on a map.
    pub fn card(&self, note: &Note) -> String {
        let mut out = format!("[{}] {}", note.id, flat(&note.title));
        if note.kind != Kind::Idea {
            out.insert_str(note.id.len() + 2, &format!(" ({})", note.kind.name()));
        }
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
                for linked in notes.iter().take(LINKS) {
                    // The reason is the one the note that holds the link gives.
                    let why = match heading {
                        "Links to" => note.why.get(&linked.id),
                        _ => linked.why.get(&note.id),
                    };
                    let _ = write!(out, "  {}", self.card(linked));
                    match why {
                        Some(why) => {
                            let _ = writeln!(out, " — why: {why}");
                        }
                        None => out.push('\n'),
                    }
                }
                if notes.len() > LINKS {
                    let _ = writeln!(out, "  and {} more", notes.len() - LINKS);
                }
            }
        }
        out
    }
    /// What a search for each of `phrases` finds, as cards: the way in when no overview leads
    /// to what is wanted. A card says when its note is already open in `seen`. Returns the
    /// text and the ids of the notes found.
    pub fn found(&self, phrases: &[String], seen: &BTreeSet<String>) -> (String, Vec<String>) {
        let (mut out, mut ids) = (String::new(), vec![]);
        for words in phrases
            .iter()
            .map(|p| clip(p, 120))
            .filter(|p| !p.is_empty())
        {
            let notes = self.find(&words, FOUND);
            if notes.is_empty() {
                let _ = writeln!(
                    out,
                    "FOUND {words:?}: nothing. The notes may put it in other words.\n"
                );
                continue;
            }
            let _ = writeln!(out, "FOUND {words:?} ({})", count(notes.len(), "note"));
            for note in notes {
                let open = match seen.contains(&note.id) {
                    true => " · open above",
                    false => "",
                };
                let _ = writeln!(out, "{}{open}", self.card(note));
                if !ids.contains(&note.id) {
                    ids.push(note.id.clone());
                }
            }
            out.push('\n');
        }
        (out, ids)
    }
    /// What a turn asked to read: notes by id or title, in full, and topics by name, as the
    /// cards of their notes. A name that is neither is searched for. `seen` holds what the
    /// turn was shown before, and takes what it is shown now. Returns the text and the ids of
    /// the notes opened.
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
                let within =
                    |n: &&Note| n.topic.eq_ignore_ascii_case(topic) && n.kind != Kind::Overview;
                let notes: Vec<&Note> = self.notes.iter().filter(within).collect();
                let _ = writeln!(out, "TOPIC {topic} ({})", count(notes.len(), "note"));
                // Its overview comes first: it says how the notes below hang together.
                if let Some(overview) = self.overview(topic)
                    && seen.insert(overview.id.clone())
                {
                    let _ = writeln!(out, "Overview: {}", clip(&overview.body, BODY));
                }
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
                    "The knowledge base holds no note or topic called {name:?}."
                );
                // Words that name nothing are what someone is looking for.
                match self.found(std::slice::from_ref(&name), seen) {
                    (found, ids) if !ids.is_empty() => out.push_str(&found),
                    _ => out.push('\n'),
                }
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
        written.links = vec!["k00002".into(), "k00009".into(), "k00010".into()];
        let why = [
            ("k00002", "an example of it"),
            ("k00010", "contradicts it: weekly, not daily"),
        ];
        written.why = why.map(|(id, why)| (id.into(), why.into())).into();
        written.source = "lean notes.pdf".into();
        written.asset = "abc.pdf".into();
        written.created = 1_794_000_000;
        let page = written.page(&|id| (id == "k00002").then(|| "Limits | on [work]".into()));
        assert!(
            page.starts_with("---\nid: k3fa2c\ntitle: Small batches\n"),
            "{page}"
        );
        assert!(page.contains("\n# Small batches\n\n> Smaller batches mean faster feedback.\n"));
        // A link says why it is there. One the text makes is listed again only for that.
        assert!(
            page.ends_with(
                "\nSee also:\n- [[k00002|Limits   on  work ]] — an example of it\n- [[k00009]]\n- [[k00010]] — contradicts it: weekly, not daily\n"
            ),
            "{page}"
        );
        assert_eq!(Note::parse("k3fa2c", &page), written);
        // Windows line endings, as an editor there would save it.
        assert_eq!(Note::parse("k3fa2c", &page.replace('\n', "\r\n")), written);
        // The page is sealed as the librarian's. One edit and it is the user's.
        assert!(page.contains("\ncreated: 1794000000\nseal: "), "{page}");
        let edited = Note::parse("k3fa2c", &page.replace("First paragraph", "My paragraph"));
        assert!(edited.hand && edited.body.starts_with("My paragraph."));
        let unsealed: Vec<&str> = page.lines().filter(|l| !l.starts_with("seal:")).collect();
        assert!(Note::parse("k3fa2c", &unsealed.join("\n")).hand);
    }
    #[test]
    fn a_note_says_what_kind_it_is() {
        let mut overview = note(
            "overview-flow",
            "Overview of Flow",
            "Flow",
            &[],
            "Start at [[k00001|Small batches]].",
        );
        overview.kind = Kind::Overview;
        overview.links = vec!["k00001".into()];
        let page = overview.page(&|_| None);
        assert!(page.contains("\nkind: overview\n"), "{page}");
        assert!(!page.contains("See also"), "{page}");
        assert_eq!(Note::parse("overview-flow", &page), overview);
        // As other tools name them.
        let kind = |kind: &str| Note::parse("x", &format!("---\ntype: {kind}\n---\nBody.")).kind;
        assert_eq!(kind("MOC"), Kind::Overview);
        assert_eq!(kind("literature"), Kind::Source);
        assert_eq!(kind("permanent"), Kind::Idea);
        // Reasons the user wrote by hand, in a list or on one line.
        let by_hand = Note::parse(
            "mine",
            "# Mine\n\nA thought.\n\nSee also: [[k00003]]\n* [[k00001|Small batches]] - the reason behind it\n- [[k00002]]\n\nAfter the list.\n- [[k00004]] is in the text\n",
        );
        assert!(by_hand.hand);
        assert_eq!(by_hand.links, ["k00004", "k00003", "k00001", "k00002"]);
        assert_eq!(by_hand.why["k00001"], "the reason behind it");
        assert_eq!(by_hand.why.len(), 1);
        assert_eq!(
            by_hand.body,
            "A thought.\n\nAfter the list.\n- [[k00004]] is in the text"
        );
    }
    #[test]
    fn a_time_is_written_as_a_date() {
        assert_eq!(date(0), "1970-01-01 00:00");
        assert_eq!(date(951_782_400), "2000-02-29 00:00");
        assert_eq!(date(951_782_400 + 86_400), "2000-03-01 00:00");
        assert_eq!(date(1_794_018_420), "2026-11-07 02:27");
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
            page.ends_with(
                "\nSee also:\n- [[k00002|Limits on work in progress expose bottlenecks]]\n"
            ),
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
    fn what_the_user_wrote_is_spared_and_what_is_replaced_is_kept() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let mut kept = Library::default();
        kept.add(root, library().notes().to_vec()).unwrap();
        assert!(!root.join("history").exists() && !root.join("changes.md").exists());
        let page = |id: &str| root.join(format!("notes/{id}.md"));
        let read = |path: PathBuf| fs::read_to_string(path).unwrap();
        // The user edits one page after the app read it.
        let (first, second) = (read(page("k00001")), read(page("k00002")));
        let edited = second.replace("A cap on work", "My own words: a cap on work");
        fs::write(page("k00002"), &edited).unwrap();
        let rewritten = |id: &str| {
            let mut note = kept.get(id).unwrap().clone();
            note.gist = "Rewritten.".into();
            note
        };
        let changes = Changes {
            notes: vec![rewritten("k00001"), rewritten("k00002")],
            retired: vec!["k00004".into(), "k99999".into()],
            log: vec!["Rewrote [k00001].".into(), "Took out\n[k00004].".into()],
        };
        let now = 1_794_018_420;
        assert_eq!(kept.apply(root, changes, now).unwrap(), ["k00002"]);
        assert_eq!(kept.get("k00001").unwrap().gist, "Rewritten.");
        assert_ne!(kept.get("k00002").unwrap().gist, "Rewritten.");
        assert!(kept.get("k00004").is_none() && !page("k00004").exists());
        assert_eq!(read(page("k00002")), edited);
        // What a page was is kept, named by the note and the time.
        assert_eq!(read(root.join(format!("history/k00001-{now}.md"))), first);
        assert!(root.join(format!("history/k00004-{now}.md")).exists());
        assert!(!root.join(format!("history/k00002-{now}.md")).exists());
        let log = read(root.join("changes.md"));
        assert!(log.starts_with("# What the librarian changed\n"), "{log}");
        assert!(
            log.ends_with(
                "\n- 2026-11-07 02:27 · Rewrote [k00001].\n- 2026-11-07 02:27 · Took out [k00004].\n"
            ),
            "{log}"
        );
        // Read again, the folder holds what the knowledge base does, and the user's page
        // as they left it.
        let found = Library::load(root);
        assert_eq!(found.len(), 3);
        assert_eq!(found.get("k00001"), kept.get("k00001"));
        let mine = found.get("k00002").unwrap();
        assert!(mine.hand && mine.body.starts_with("My own words"));
        assert!(!root.join("history/.md").exists());
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
    fn an_overview_leads_into_its_topic() {
        let mut notes = library().notes().to_vec();
        let mut overview = note(
            "overview-delivery-flow",
            "Overview of Delivery flow",
            "Delivery flow",
            &["feedback"],
            "Start with [[k00001|Small batches]], then [[k00002|Limits]].",
        );
        overview.kind = Kind::Overview;
        overview.links = vec!["k00001".into(), "k00002".into()];
        let mut source = note(
            "s00001",
            "Lean handbook",
            "Delivery flow",
            &[],
            "How it runs:\n- One.",
        );
        source.kind = Kind::Source;
        source.links = vec!["k00001".into(), "k00003".into()];
        source.why = [("k00003".to_owned(), "says the same".to_owned())].into();
        notes.extend([overview, source]);
        let library = Library::of(notes);
        assert_eq!(
            library.overview("delivery FLOW").unwrap().id,
            "overview-delivery-flow"
        );
        assert!(library.overview("Pricing").is_none());
        let index = library.index();
        assert!(
            index.starts_with(
                "6 notes in 3 topics:\n- Delivery flow (3, with an overview): feedback, "
            ),
            "{index}"
        );
        assert!(index.contains("\n- Pricing (1): "), "{index}");
        let card = library.card(library.get("s00001").unwrap());
        assert!(
            card.starts_with("[s00001] (source) Lean handbook · Delivery flow: "),
            "{card}"
        );
        // A link's reason is shown with the note that holds it.
        let full = library.full(library.get("s00001").unwrap());
        assert!(
            full.contains("\nLinks to:\n  [k00001] Small batches"),
            "{full}"
        );
        assert!(
            full.contains(
                " · Pricing: Anchoring sets what a price is compared with. — why: says the same\n"
            ),
            "{full}"
        );
        let full = library.full(library.get("k00003").unwrap());
        assert!(full.ends_with("Linked from:\n  [s00001] (source) Lean handbook · Delivery flow: Lean handbook. · see k00001, k00003 — why: says the same\n"), "{full}");
        // A topic opens with its overview, once.
        let mut seen = BTreeSet::new();
        let (text, _) = library.open(&["Delivery flow".into()], &mut seen);
        assert!(
            text.starts_with("TOPIC Delivery flow (3 notes)\nOverview: Start with [[k00001|Small batches]], then [[k00002|Limits]].\n[k00001] Small"),
            "{text}"
        );
        assert!(!text.contains("[overview-delivery-flow]"), "{text}");
        let (again, _) = library.open(&["delivery flow".into()], &mut seen);
        assert!(
            again.starts_with("TOPIC Delivery flow (3 notes)\n[k00001] Small"),
            "{again}"
        );
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
    fn a_reader_searches_follows_links_and_goes_to_the_source() {
        let mut flow = note(
            "k00001",
            "Small batches shorten feedback loops",
            "Delivery flow",
            &["batch size", "feedback"],
            "Work released in small batches reaches users sooner.",
        );
        flow.links = vec!["k00002".into()];
        flow.why = [("k00002".into(), "the step before it".into())].into();
        flow.asset = "abc.pdf".into();
        let mut limits = note(
            "k00002",
            "Limits on work in progress expose bottlenecks",
            "Delivery flow",
            &["wip limit", "bottleneck"],
            "A cap on work in progress makes a queue visible.",
        );
        limits.links = vec!["k00001".into()];
        limits.why = [(
            "k00001".into(),
            " Contradicts it: large batches hide queues".into(),
        )]
        .into();
        let mut file = note(
            "s00001",
            "Lean notes on delivery",
            "Delivery flow",
            &["lean"],
            "Notes from a talk on flow.",
        );
        (file.kind, file.asset) = (Kind::Source, "abc.pdf".into());
        file.links = vec!["k00001".into()];
        let own = note(
            "mine",
            "My own note",
            "Delivery flow",
            &[],
            "Written by hand.",
        );
        let library = Library::of(vec![flow, limits, file, own]);
        let (flow, limits) = (
            library.get("k00001").unwrap(),
            library.get("k00002").unwrap(),
        );
        let (file, own) = (library.get("s00001").unwrap(), library.get("mine").unwrap());

        // A note's ties are the notes it leads to, then the notes that lead to it, each with
        // the reason its holder gives.
        let ties = library.ties(flow);
        let told: Vec<(&str, &str, bool, bool)> = ties
            .iter()
            .map(|tie| (tie.note.id.as_str(), tie.why, tie.out, tie.disputes()))
            .collect();
        assert_eq!(
            told,
            [
                ("k00002", "the step before it", true, false),
                (
                    "k00002",
                    " Contradicts it: large batches hide queues",
                    false,
                    true
                ),
                ("s00001", "", false, false),
            ]
        );
        assert!(library.ties(own).is_empty());
        // A note leads back to the file it was taken from, and the file to its notes.
        assert_eq!(
            library.source_of(flow).map(|note| note.id.as_str()),
            Some("s00001")
        );
        assert!(library.source_of(limits).is_none());
        assert!(library.source_of(file).is_none());
        assert_eq!(ids(&library.taken_from(file)), ["k00001"]);
        assert!(library.taken_from(own).is_empty());

        // A search finds notes of every kind and says which are already open.
        assert_eq!(ids(&library.find("bottleneck", 5)), ["k00002"]);
        assert!(ids(&library.find("lean talk", 5)).contains(&"s00001".to_owned()));
        let seen: BTreeSet<String> = ["k00002".to_owned()].into();
        let phrases = [
            "bottleneck".to_owned(),
            "zebra crossing".to_owned(),
            " ".to_owned(),
        ];
        let (text, found) = library.found(&phrases, &seen);
        assert_eq!(found, ["k00002"]);
        assert_eq!(
            text,
            "FOUND \"bottleneck\" (1 note)\n[k00002] Limits on work in progress expose bottlenecks · Delivery flow: Limits on work in progress expose bottlenecks. · see k00001 · open above\n\nFOUND \"zebra crossing\": nothing. The notes may put it in other words.\n\n"
        );
        // Links read as the titles they lead to.
        assert_eq!(
            library.prose("Start with [[k00001|Small batches]], then [[k00002]] and [[gone]]."),
            "Start with Small batches, then Limits on work in progress expose bottlenecks and gone."
        );
        assert_eq!(
            library.prose("An open [[bracket stays"),
            "An open [[bracket stays"
        );
        assert_eq!(library.get("k00001").unwrap().filed(), None);
        let dated = Note {
            created: 1_794_500_000,
            ..Default::default()
        };
        assert_eq!(dated.filed().as_deref(), Some("2026-11-12"));
        // Words that name no note or topic are searched for.
        let mut seen = BTreeSet::new();
        let (text, opened) = library.open(&["queue visible".into()], &mut seen);
        assert!(opened.is_empty() && seen.is_empty());
        assert!(
            text.starts_with(
                "The knowledge base holds no note or topic called \"queue visible\".\nFOUND \"queue visible\" (1 note)\n[k00002] "
            ),
            "{text}"
        );
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
