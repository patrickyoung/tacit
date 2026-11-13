//! The knowledge room: the notes, read the way a practised reader goes through a zettelkasten.
//!
//! The reader comes in by a topic and its overview, searches in the notes' own words, reads
//! one note whole, follows a link for the reason it gives in either direction, and goes back
//! to the file a note was taken from. A question can also be asked: the researcher then walks
//! the notes the same way and answers from the ones it read
//! ([`tacit::knowledge::research`]). Searching and reading stay on this machine; only asking
//! goes to a model.
use super::*;
use kit::Icon;
use tacit::knowledge::research::{self, Finding, Lead};
use tacit::knowledge::{Kind, Library, Note};

/// How many places the room remembers the way back through.
const TRAIL: usize = 64;
/// How many notes a search shows.
const HITS: usize = 12;
/// How many notes the way in lists as filed last.
const LATEST: usize = 5;
/// How wide a page of reading runs.
const MEASURE: f32 = 760.0;

/// Where a reader stands in the notes.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Place {
    /// The way in: the topics, the files notes were taken from, and what was filed last.
    Entry,
    Topic(String),
    Note(String),
    /// What a search for these words finds.
    Found(String),
    /// The answer to the last question asked.
    Answer,
}

pub(super) struct Room {
    pub(super) open: bool,
    /// The places gone through. The reader stands at the last of them.
    pub(super) trail: Vec<Place>,
    /// Words to look for, or a question to ask.
    pub(super) edit: TextEdit,
    /// The last question asked, and the job answering it while that is under way.
    pub(super) asked: String,
    pub(super) asking: Option<u64>,
    pub(super) progress: String,
    pub(super) finding: Option<Finding>,
    /// Why the last question got no answer.
    pub(super) failure: String,
    next: u64,
}
impl Default for Room {
    fn default() -> Self {
        Self {
            open: false,
            trail: vec![],
            edit: TextEdit::new(""),
            asked: String::new(),
            asking: None,
            progress: String::new(),
            finding: None,
            failure: String::new(),
            next: 1,
        }
    }
}
impl Room {
    pub(super) fn place(&self) -> &Place {
        self.trail.last().unwrap_or(&Place::Entry)
    }
}

impl State {
    /// The knowledge base, when it holds something to read.
    fn readable(&self) -> Option<&Library> {
        self.library
            .as_deref()
            .filter(|library| !library.is_empty())
    }
    /// The room is on screen in place of the map.
    pub(super) fn reading_notes(&self) -> bool {
        self.room.open && self.replay.is_none() && self.readable().is_some()
    }
    /// Reading changes nothing, so a session that cannot be edited can still read.
    pub(super) fn open_room(&mut self) {
        if self.replay.is_some() || self.readable().is_none() {
            return;
        }
        self.sessions_open = false;
        self.room.open = true;
        if self.room.trail.is_empty() {
            self.room.trail.push(Place::Entry);
        }
    }
    pub(super) fn close_room(&mut self) {
        self.room.open = false;
    }
    pub(super) fn room_go(&mut self, place: Place) {
        if self.room.trail.last() != Some(&place) {
            self.room.trail.push(place);
        }
        if self.room.trail.len() > TRAIL {
            self.room.trail.remove(0);
        }
    }
    /// One place back, and out of the room from the place it was entered at.
    pub(super) fn room_back(&mut self) {
        if self.room.trail.len() > 1 {
            self.room.trail.pop();
        } else {
            self.close_room();
        }
    }
    /// Looks for the words in the field, on this machine.
    pub(super) fn room_find(&mut self) {
        let words = self.room.edit.text.trim().to_owned();
        if !words.is_empty() {
            self.room_go(Place::Found(words));
        }
    }
    /// Asks the researcher what is in the field. The question and the notes it reads on the
    /// way go to the model.
    pub(super) fn room_ask(&mut self) {
        let question = self.room.edit.text.trim().to_owned();
        let Some(library) = self.library.clone() else {
            return;
        };
        if question.is_empty() {
            return;
        }
        let job = self.room.next;
        self.room.next += 1;
        self.room.finding = None;
        self.room.failure.clear();
        self.room.progress.clear();
        self.room.asked = question.clone();
        let request = research::Request {
            job,
            question,
            library,
        };
        let sent = |worker: &research::Worker| worker.send.send(request).is_ok();
        if self.researcher.as_ref().is_some_and(sent) {
            self.room.asking = Some(job);
        } else {
            self.room.asking = None;
            self.room.failure =
                "Asking is not available here · search and the notes are still here".into();
        }
        self.room_go(Place::Answer);
    }
    pub(super) fn receive_research(&mut self, response: research::Response) {
        // An answer to a question that was since replaced by another is not shown.
        if self.room.asking != Some(response.job) {
            return;
        }
        match response.outcome {
            research::Outcome::Progress(note) => self.room.progress = note,
            research::Outcome::Found(found) => {
                self.room.asking = None;
                self.room.progress.clear();
                match found {
                    Ok(finding) => self.room.finding = Some(*finding),
                    Err(error) => self.room.failure = error,
                }
            }
        }
    }
}

/// A small knowledge base to show the room with: two files, one of which disputes the other.
pub(super) fn shelf() -> Library {
    let note = |id: &str, title: &str, topic: &str, gist: &str, body: &str| Note {
        id: id.into(),
        title: title.into(),
        topic: topic.into(),
        gist: gist.into(),
        body: body.into(),
        source: "Harbor team handbook.md".into(),
        asset: "handbook.md".into(),
        created: 1_794_546_461,
        ..Note::default()
    };
    let later = |mut note: Note| {
        note.source = "Harbor retrospective.md".into();
        note.asset = "retrospective.md".into();
        note.created = 1_794_546_481;
        note
    };
    let link = |note: &mut Note, to: &str, why: &str| {
        note.links.push(to.into());
        if !why.is_empty() {
            note.why.insert(to.into(), why.into());
        }
    };
    let mut six = note(
        "kefba9",
        "Harbor meetings have at most six people",
        "Meetings",
        "A Harbor meeting has at most six people; anyone who only needs the outcome reads the notes.",
        "The handbook caps a meeting at six people. Someone who wants to know what was decided, and has nothing to decide, reads the notes afterwards.",
    );
    six.keywords = vec!["attendance".into(), "six-person limit".into()];
    link(&mut six, "kde39a", "what makes the limit workable");
    let mut agenda = note(
        "k9b94b",
        "Late agendas cancel Harbor meetings",
        "Meetings",
        "A Harbor meeting without an agenda one working day ahead is cancelled.",
        "The agenda goes out one working day before the meeting and names the decision to be made. Without it the meeting does not take place.",
    );
    link(&mut agenda, "kde39a", "the other half of preparing");
    let log = note(
        "kde39a",
        "Status updates stay in the shared log",
        "Meetings",
        "Status updates are written in the shared log, so meeting time is kept for decisions.",
        "Nobody reads out a status in a Harbor meeting. Updates are written in the shared log before it.",
    );
    let mut reviews = later(note(
        "k8f862",
        "Incident reviews need everyone involved",
        "Meetings",
        "Harbor incident reviews should include everyone involved, whatever the six-person limit says.",
        "An outage review needs everyone who touched the system on the call. In one September incident eleven people were involved, and splitting the review into two meetings of six lost the thread both times.",
    ));
    reviews.keywords = vec!["incident reviews".into(), "attendance".into()];
    link(
        &mut reviews,
        "kefba9",
        "contradicts it: the general six-person limit does not fit incident reviews",
    );
    let mut missed = later(note(
        "k07098",
        "Eleven cancelled Harbor meetings were not missed",
        "Meetings",
        "Of eleven meetings cancelled for a late agenda in one quarter, nobody asked for one to be held again.",
        "The retrospective counted eleven meetings cancelled under the agenda rule in one quarter. None was rescheduled at anyone's request.",
    ));
    link(&mut missed, "k9b94b", "what happened when it was enforced");
    let mut owner = note(
        "kfdb0b",
        "One owner makes each Harbor decision",
        "Decision making",
        "Each Harbor decision has one owner, who hears everyone out and decides without a vote.",
        "A decision belongs to one named person. They hear everyone out, decide without a vote or consensus, and write down the decision, the reasons and what was rejected.",
    );
    link(
        &mut owner,
        "k9b94b",
        "the agenda names the decision this owner makes",
    );
    let mut overview = Note {
        id: "overview-meetings".into(),
        title: "Overview of Meetings".into(),
        topic: "Meetings".into(),
        kind: Kind::Overview,
        gist: "Harbor's meeting rules cover preparation and attendance, with an exception for incident reviews.".into(),
        body: "Begin with the agenda rule [[k9b94b|Late agendas cancel Harbor meetings]], which decides whether a meeting happens at all, and what followed when it was enforced [[k07098|Eleven cancelled Harbor meetings were not missed]]. The shared log [[kde39a|Status updates stay in the shared log]] takes status out of meeting time. For attendance, the six-person limit [[kefba9|Harbor meetings have at most six people]] holds in general, and incident reviews [[k8f862|Incident reviews need everyone involved]] are the exception the retrospective argues for.".into(),
        created: 1_794_546_490,
        ..Note::default()
    };
    for id in ["k9b94b", "k07098", "kde39a", "kefba9", "k8f862"] {
        link(&mut overview, id, "");
    }
    let file = |id: &str, title: &str, name: &str, asset: &str, gist: &str| Note {
        id: id.into(),
        title: title.into(),
        topic: "Meetings".into(),
        kind: Kind::Source,
        gist: gist.into(),
        source: name.into(),
        asset: asset.into(),
        created: 1_794_546_461,
        ..Note::default()
    };
    let handbook = file(
        "s9ada4",
        "Harbor team handbook",
        "Harbor team handbook.md",
        "handbook.md",
        "A team handbook: how Harbor prepares and runs meetings, and who owns a decision.",
    );
    let retrospective = file(
        "s37e59",
        "Harbor retrospective",
        "Harbor retrospective.md",
        "retrospective.md",
        "A retrospective on a quarter under the handbook: what held up and what did not.",
    );
    Library::of(vec![
        agenda,
        log,
        six,
        owner,
        missed,
        reviews,
        overview,
        handbook,
        retrospective,
    ])
}
impl State {
    /// One note open in the knowledge room, reached from its topic.
    pub fn room_demo() -> Self {
        let mut s = Self::mapped_demo();
        s.selected = None;
        s.knowledge_root = Some(PathBuf::from("session.knowledge"));
        s.library = Some(std::sync::Arc::new(shelf()));
        s.open_room();
        s.room_go(Place::Topic("Meetings".into()));
        s.room_go(Place::Note("kefba9".into()));
        s
    }
    /// A question the researcher has answered from the notes.
    pub fn answer_demo() -> Self {
        let mut s = Self::room_demo();
        s.room.trail.truncate(1);
        s.room.edit = TextEdit::new("How many people should be in a meeting?");
        s.room.asked = s.room.edit.text.clone();
        let point = |note: &str, says: &str| research::Point {
            note: note.into(),
            says: says.into(),
        };
        let step = |text: &str, note: Option<&str>| research::Step {
            text: text.into(),
            note: note.map(str::to_owned),
        };
        s.room.finding = Some(Finding {
            question: s.room.asked.clone(),
            reply: "At most six, as a general rule: people who only need the outcome read the notes instead [1]. The notes do not agree that this holds everywhere. The retrospective argues that an incident review needs everyone who touched the system, however many that is, after two reviews split to fit the limit lost the thread [2].".into(),
            points: vec![
                point("kefba9", "Sets the limit of six and sends everyone else to the notes."),
                point("k8f862", "Rejects the limit for incident reviews, from one September outage."),
            ],
            clashes: vec![research::Clash {
                notes: vec!["kefba9".into(), "k8f862".into()],
                over: "Whether the six-person limit applies to incident reviews.".into(),
            }],
            gaps: vec!["Nothing says how the limit was chosen, or whether it was tried at other sizes.".into()],
            next: vec![research::Onward {
                to: Lead::Note("kde39a".into()),
                why: "The shared log is what lets people stay out of a meeting.".into(),
            }],
            trail: vec![
                step("Came in through the overview of Meetings", Some("overview-meetings")),
                step("4 notes near the question's words", None),
                step("Read “Harbor meetings have at most six people”", Some("kefba9")),
                step("Read “Incident reviews need everyone involved”", Some("k8f862")),
            ],
            read: 3,
            spent: "2 model calls and 6410 tokens, $0.0052".into(),
        });
        s.room_go(Place::Answer);
        s
    }
}

/// A heading within a page.
fn heading(text: &str) -> El {
    kit::eyebrow(text, MUTED).pl(12.0).mt(10.0)
}
/// Reading text at the width a line is comfortable at.
fn reading(content: impl Into<String>, size: f32, color: Color) -> El {
    label(content, size, color)
        .max_w(MEASURE)
        .wrap_anywhere()
        .selectable(true)
}
/// Somewhere to go: a title, what is there in a sentence, and why it is listed here. `mark`
/// stands before the title; a reason that `warns` says the two notes disagree.
fn way(
    key: String,
    mark: &str,
    title: &str,
    says: &str,
    why: Option<(&str, bool)>,
    to: Place,
) -> El {
    let mut name = row().gap(8.0);
    if !mark.is_empty() {
        name = name.child(kit::caption(mark, 12.0, 600.0, GREEN));
    }
    name = name.child(
        text(title)
            .font(TextStyle::new(13.5, 600.0))
            .color(INK)
            .wrap()
            .grow(1.0)
            .min_w(0.0),
    );
    let mut way = col()
        .key(key)
        .role("button")
        .label(title)
        .focusable(true)
        .max_w(MEASURE)
        .gap(3.0)
        .px(12.0)
        .py(9.0)
        .radius(12.0)
        .hover(kit::wash(1.4))
        .on_press(move |s: &mut State| s.room_go(to.clone()))
        .child(name);
    if !says.is_empty() {
        way = way.child(label(says, 12.5, MUTED).wrap_anywhere());
    }
    match why {
        Some((why, true)) => {
            way = way.child(label(format!("Disagrees · {why}"), 12.0, RED).wrap_anywhere());
        }
        Some((why, false)) if !why.is_empty() => {
            way = way.child(label(why, 12.0, GOLD).wrap_anywhere());
        }
        _ => {}
    }
    way
}
/// The way to one note.
fn to_note(key: String, mark: &str, note: &Note, why: Option<(&str, bool)>) -> El {
    let mark = match (mark, note.kind) {
        ("", Kind::Source) => "File",
        ("", Kind::Overview) => "Overview",
        (mark, _) => mark,
    };
    let to = Place::Note(note.id.clone());
    way(key, mark, &note.title, &note.gist, why, to)
}
fn count(n: usize, of: &str) -> String {
    match n {
        1 => format!("1 {of}"),
        n => format!("{n} {of}s"),
    }
}

/// The way in: a reader starts from a topic, a file, or what came in last.
fn entry(library: &Library) -> El {
    let topics = library.topics();
    let ideas = |notes: &[&Note]| notes.iter().filter(|n| n.kind == Kind::Idea).count();
    let held = library.notes().iter().collect::<Vec<_>>();
    let mut page = col()
        .gap(2.0)
        .child(kit::eyebrow("THE KNOWLEDGE BASE", GREEN).pl(12.0))
        .child(
            reading(
                format!(
                    "{} in {}",
                    count(ideas(&held), "idea"),
                    count(topics.len(), "topic")
                ),
                21.0,
                INK,
            )
            .pl(12.0),
        )
        .child(
            reading(
                "Start from a topic and read its overview, look for a word, or ask a question.",
                13.0,
                MUTED,
            )
            .pl(12.0),
        )
        .child(heading("TOPICS"));
    for (at, (name, notes)) in topics.iter().enumerate() {
        let says = match library.overview(name) {
            Some(overview) => format!(
                "{} · {}",
                count(ideas(notes), "idea"),
                library.prose(&overview.gist)
            ),
            None => count(ideas(notes), "idea"),
        };
        let to = Place::Topic(name.to_string());
        page = page.child(way(format!("room-topic-{at}"), "", name, &says, None, to));
    }
    let files = held.iter().filter(|note| note.kind == Kind::Source);
    let files = files.collect::<Vec<_>>();
    if !files.is_empty() {
        page = page.child(heading("FILES THE NOTES WERE TAKEN FROM"));
    }
    for file in files {
        let taken = count(library.taken_from(file).len(), "idea");
        let says = format!("{taken} · {}", file.gist);
        let to = Place::Note(file.id.clone());
        let key = format!("room-file-{}", file.id);
        page = page.child(way(key, "", &file.title, &says, None, to));
    }
    let latest = held.iter().filter(|note| note.kind == Kind::Idea);
    let mut latest = latest.collect::<Vec<_>>();
    // The sort is stable, so notes of one filing stay in the order they were written.
    latest.sort_by_key(|note| std::cmp::Reverse(note.created));
    if latest.iter().any(|note| note.created > 0) {
        page = page.child(heading("FILED LAST"));
        for note in latest.into_iter().take(LATEST) {
            page = page.child(to_note(format!("room-latest-{}", note.id), "", note, None));
        }
    }
    page
}

/// One topic: its overview first, then its notes in the order the overview takes them.
fn topic(library: &Library, name: &str) -> El {
    let Some(name) = library.topic(name) else {
        return gone("This topic is no longer in the knowledge base.");
    };
    let topics = library.topics();
    let held = topics.iter().find(|(topic, _)| *topic == name);
    let mut notes = held.map(|(_, notes)| notes.clone()).unwrap_or_default();
    notes.retain(|note| note.kind != Kind::Overview);
    let mut page = col()
        .gap(2.0)
        .child(kit::eyebrow("TOPIC", GREEN).pl(12.0))
        .child(reading(name, 21.0, INK).pl(12.0));
    if let Some(overview) = library.overview(name) {
        // Notes the overview names come first, in the order it reads them.
        let order = |note: &&Note| {
            let named = overview.links.iter().position(|id| id == &note.id);
            named.unwrap_or(usize::MAX)
        };
        notes.sort_by_key(order);
        page = page
            .child(heading("START HERE"))
            .child(
                reading(&overview.gist, 15.0, INK)
                    .key("room-overview")
                    .pl(12.0),
            )
            .child(
                reading(library.prose(&overview.body), 13.5, MUTED)
                    .pl(12.0)
                    .mt(4.0),
            );
    }
    page = page.child(heading(&count(notes.len(), "NOTE").to_uppercase()));
    for note in notes {
        page = page.child(to_note(format!("room-in-{}", note.id), "", note, None));
    }
    page
}

/// One note, whole: what it says, where it came from, and what it is linked with and why.
fn note(library: &Library, id: &str) -> El {
    let Some(note) = library.get(id) else {
        return gone("This note is no longer in the knowledge base.");
    };
    let kind = match note.kind {
        Kind::Idea => "NOTE",
        Kind::Source => "FILE",
        Kind::Overview => "OVERVIEW",
    };
    let mut crumbs = row()
        .gap(4.0)
        .center_y()
        .child(kit::eyebrow(kind, GREEN).pl(12.0));
    if !note.topic.is_empty() {
        let to = Place::Topic(note.topic.clone());
        crumbs = crumbs.child(
            kit::tool(None, &note.topic, "room-note-topic", move |s| {
                s.room_go(to.clone())
            })
            .help("Go to this topic."),
        );
    }
    if note.hand {
        // Its page was written or changed by hand, so the librarian leaves it as it is.
        crumbs = crumbs.child(kit::caption("Yours", 12.0, 600.0, BLUE).key("room-yours"));
    }
    let mut page = col()
        .gap(2.0)
        .child(crumbs)
        .child(reading(&note.title, 21.0, INK).key("room-title").pl(12.0));
    if !note.gist.is_empty() {
        page = page.child(reading(&note.gist, 15.0, INK).pl(12.0).mt(2.0));
    }
    if !note.body.trim().is_empty() {
        page = page.child(
            reading(library.prose(note.body.trim()), 14.0, MUTED)
                .key("room-body")
                .pl(12.0)
                .mt(6.0),
        );
    }
    let mut origin = vec![];
    if !note.keywords.is_empty() {
        origin.push(note.keywords.join(", "));
    }
    if let Some(day) = note.filed() {
        origin.push(format!("filed {day}"));
    }
    if !origin.is_empty() {
        page = page.child(reading(origin.join(" · "), 12.0, MUTED).pl(12.0).mt(6.0));
    }
    // Back to the file: what the note was taken from says how far to trust it.
    match library.source_of(note) {
        Some(file) => {
            page = page.child(heading("TAKEN FROM")).child(to_note(
                "room-source".into(),
                "",
                file,
                None,
            ));
        }
        None if !note.source.is_empty() && note.kind == Kind::Idea => {
            page = page
                .child(heading("TAKEN FROM"))
                .child(reading(&note.source, 13.0, INK).pl(12.0));
        }
        None => {}
    }
    let ties = library.ties(note);
    for (out, title, prefix) in [
        (true, "LEADS TO", "room-to"),
        (false, "LINKED FROM", "room-from"),
    ] {
        let mut ties = ties.iter().filter(|tie| tie.out == out).peekable();
        if ties.peek().is_some() {
            page = page.child(heading(title));
        }
        for tie in ties {
            let why = Some((tie.why, tie.disputes()));
            let key = format!("{prefix}-{}", tie.note.id);
            page = page.child(to_note(key, "", tie.note, why));
        }
    }
    if note.kind == Kind::Source {
        // A file's own links already list its ideas when the librarian wrote them down.
        let listed = |idea: &&Note| !note.links.contains(&idea.id);
        let taken = library.taken_from(note);
        let taken = taken.into_iter().filter(listed).collect::<Vec<_>>();
        if !taken.is_empty() {
            page = page.child(heading("IDEAS TAKEN FROM IT"));
        }
        for idea in taken {
            page = page.child(to_note(format!("room-taken-{}", idea.id), "", idea, None));
        }
    }
    page
}

/// What a search finds, nearest first.
fn found(library: &Library, words: &str) -> El {
    let hits = library.find(words, HITS);
    let mut page = col()
        .gap(2.0)
        .child(kit::eyebrow("LOOKED FOR", GREEN).pl(12.0))
        .child(reading(format!("“{words}”"), 21.0, INK).pl(12.0));
    if hits.is_empty() {
        return page.child(
            reading(
                "No note uses these words. The notes may put it another way: try a different word, or ask it as a question.",
                13.5,
                MUTED,
            )
            .key("room-nothing")
            .pl(12.0)
            .mt(6.0),
        );
    }
    page = page.child(heading(&count(hits.len(), "NOTE").to_uppercase()));
    for hit in hits {
        page = page.child(to_note(format!("room-hit-{}", hit.id), "", hit, None));
    }
    page
}

/// The answer to a question: what the notes say, which notes say it, where they disagree,
/// and what they do not hold.
fn answer(s: &State, library: &Library) -> El {
    let room = &s.room;
    let mut page = col()
        .gap(2.0)
        .child(kit::eyebrow("ASKED", GREEN).pl(12.0))
        .child(reading(&room.asked, 21.0, INK).pl(12.0));
    if room.asking.is_some() {
        let progress = match room.progress.is_empty() {
            true => "Looking through the notes",
            false => &room.progress,
        };
        return page.child(kit::progress(format!("{progress}…"), "room-progress").mt(6.0));
    }
    if !room.failure.is_empty() {
        return page.child(
            row()
                .key("room-failure")
                .mt(8.0)
                .ml(12.0)
                .px(14.0)
                .py(8.0)
                .radius(16.0)
                .max_w(MEASURE)
                .fill(GOLD.alpha(0.09))
                .child(
                    label(&room.failure, 12.5, INK)
                        .grow(1.0)
                        .min_w(0.0)
                        .wrap_anywhere(),
                ),
        );
    }
    let Some(finding) = &room.finding else {
        return page;
    };
    page = page.child(
        reading(&finding.reply, 15.5, INK)
            .key("room-reply")
            .pl(12.0)
            .mt(6.0),
    );
    if finding.points.is_empty() {
        page = page.child(
            reading(
                "This answer rests on no note in the knowledge base.",
                12.5,
                GOLD,
            )
            .key("room-unfounded")
            .pl(12.0)
            .mt(4.0),
        );
    } else {
        page = page.child(heading("RESTS ON"));
    }
    let title = |id: &str| {
        library
            .get(id)
            .map_or(id, |note| note.title.as_str())
            .to_owned()
    };
    for (at, point) in finding.points.iter().enumerate() {
        let (key, mark) = (format!("room-basis-{}", at + 1), format!("{}", at + 1));
        let to = Place::Note(point.note.clone());
        let says = match (point.says.is_empty(), library.get(&point.note)) {
            (true, Some(note)) => note.gist.as_str(),
            _ => point.says.as_str(),
        };
        page = page.child(way(key, &mark, &title(&point.note), says, None, to));
    }
    if !finding.clashes.is_empty() {
        page = page.child(heading("THE NOTES DISAGREE"));
    }
    for (at, clash) in finding.clashes.iter().enumerate() {
        page = page.child(reading(&clash.over, 13.5, RED).pl(12.0).mt(2.0));
        for (side, id) in clash.notes.iter().enumerate() {
            let (key, to) = (format!("room-clash-{at}-{side}"), Place::Note(id.clone()));
            page = page.child(way(key, "", &title(id), "", None, to));
        }
    }
    if !finding.gaps.is_empty() {
        page = page.child(heading("NOT IN THE NOTES"));
    }
    for gap in &finding.gaps {
        page = page.child(reading(gap, 13.5, INK).pl(12.0).mt(2.0));
    }
    if !finding.next.is_empty() {
        page = page.child(heading("READ NEXT"));
    }
    for (at, onward) in finding.next.iter().enumerate() {
        let key = format!("room-next-{at}");
        let why = Some((onward.why.as_str(), false));
        page = page.child(match &onward.to {
            Lead::Note(id) => {
                let to = Place::Note(id.clone());
                way(key, "", &title(id), "", why, to)
            }
            Lead::Topic(name) => {
                let to = Place::Topic(name.clone());
                way(key, "Topic", name, "", why, to)
            }
        });
    }
    if !finding.trail.is_empty() {
        page = page.child(heading("HOW IT WAS FOUND"));
    }
    for (at, step) in finding.trail.iter().enumerate() {
        let line = label(&step.text, 12.5, MUTED)
            .wrap_anywhere()
            .max_w(MEASURE);
        let line = line.key(format!("room-step-{at}")).px(12.0).py(4.0);
        page = page.child(match step.note.clone() {
            // A step that read a note leads to it.
            Some(id) => line
                .role("button")
                .focusable(true)
                .radius(10.0)
                .hover(kit::wash(1.4))
                .on_press(move |s: &mut State| s.room_go(Place::Note(id.clone()))),
            None => line,
        });
    }
    let mut cost = count(finding.read, "note") + " read in full";
    if !finding.spent.is_empty() {
        cost = format!("{cost} · {}", finding.spent);
    }
    page.child(reading(cost, 11.5, MUTED).key("room-cost").pl(12.0).mt(8.0))
}

fn gone(text: &str) -> El {
    col().child(reading(text, 13.5, MUTED).key("room-gone").pl(12.0))
}

/// The field that both looks and asks, with the way back and the way out.
fn bar(s: &State) -> El {
    let written = !s.room.edit.text.trim().is_empty();
    let field = row()
        .role("textbox")
        .center_y()
        .h(40.0)
        .px(14.0)
        .radius(20.0)
        .grow(1.0)
        .min_w(0.0)
        .fill(kit::wash(1.0))
        .child(
            text_input(&s.room.edit, TextStyle::new(13.5, 400.0))
                .key("room-input")
                .label("Look for a word in the notes, or ask them a question")
                .help(
                    "Enter looks for these words on this Mac. Ask sends the question to the model.",
                )
                .single_line()
                .grow(1.0)
                .min_w(0.0)
                .color(INK)
                .placeholder("Look for a word, or ask a question…", MUTED)
                .on_edit(|s: &mut State| &mut s.room.edit)
                .on_submit(|s: &mut State, _| s.room_find()),
        );
    let back = kit::icon_button(Icon::Back, "Back", "room-back", State::room_back)
        .help("Back to where you were reading. Escape does the same.");
    let ask = kit::primary(Some(Icon::Spark), "Ask", "room-ask", GREEN, State::room_ask)
        .help("Has the researcher read through the notes and answer from them. The question and the notes it reads go to the model.");
    row()
        .key("room-bar")
        .gap(8.0)
        .center_y()
        .child(back)
        .child(field)
        .child(enabled(
            kit::tool(None, "Find", "room-find", State::room_find)
                .help("Looks for these words in the notes, on this Mac."),
            written,
        ))
        .child(enabled(ask, written && s.room.asking.is_none()))
        .child(
            kit::tool(
                Some(Icon::Folder),
                "Show folder",
                "room-folder",
                State::show_knowledge,
            )
            .help("The notes are plain Markdown files. Shows the folder they are kept in."),
        )
}

/// The room, in the place the map has on screen.
pub(super) fn view(s: &State) -> El {
    let Some(library) = s.library.as_deref() else {
        return el();
    };
    let page = match s.room.place() {
        Place::Entry => entry(library),
        Place::Topic(name) => topic(library, name),
        Place::Note(id) => note(library, id),
        Place::Found(words) => found(library, words),
        Place::Answer => answer(s, library),
    };
    // Keyed by how far in the reader is, so each place opens at its top.
    let page = page
        .key(format!("room-page-{}", s.room.trail.len()))
        .grow(1.0)
        .min_h(0.0)
        .pb(24.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    kit::stage(col().key("room"))
        .grow(1.0)
        .min_h(0.0)
        .px(24.0)
        .pt(18.0)
        .gap(14.0)
        .child(bar(s))
        .child(page)
}
