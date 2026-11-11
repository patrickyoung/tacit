use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    #[default]
    Observation,
    Question,
    Tension,
    Possibility,
}
impl Role {
    pub const ALL: [Self; 4] = [
        Self::Observation,
        Self::Question,
        Self::Tension,
        Self::Possibility,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Observation => "What we're noticing",
            Self::Question => "What we're wondering",
            Self::Tension => "What pulls against it",
            Self::Possibility => "What we could try",
        }
    }
    pub fn next(self) -> Self {
        Self::ALL[(Self::ALL.iter().position(|r| *r == self).unwrap_or(0) + 1) % 4]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thought {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    /// Concise interpretation for the map; `text` always retains the original words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concept: Option<String>,
    /// The user brought this back from the parking lot; the agent may not park it again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub kept: bool,
    pub id: u64,
    pub text: String,
    pub role: Role,
    pub pinned: bool,
    #[serde(default)]
    pub category: Option<String>,
}
impl Thought {
    pub fn map_label(&self) -> &str {
        self.concept.as_deref().unwrap_or(&self.text)
    }
}
/// Why a thought waits beside the map instead of on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParkKind {
    /// Relevant, but the map has no home for it yet.
    Later,
    /// A different subject from the one this map is about.
    OffTopic,
    /// Filler, housekeeping or a false start with nothing to map.
    Aside,
}
impl ParkKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Later => "Not yet",
            Self::OffTopic => "Off topic",
            Self::Aside => "Aside",
        }
    }
}
/// A captured thought held in the parking lot. Its words are never lost and it can return.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parked {
    pub thought: Thought,
    pub kind: ParkKind,
    /// Keyword label shared by parked thoughts about the same subject.
    pub topic: String,
    pub reason: String,
    /// The user decided this stays parked; the agent may not bring it back.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
}
/// What the mind map agent remembers between turns. Captures never clear it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Charter {
    /// The central question or subject this map is about.
    #[serde(default)]
    pub focus: String,
    /// The one ordering idea its main branches share.
    #[serde(default)]
    pub principle: String,
    #[serde(default)]
    pub narrative: String,
    #[serde(default)]
    pub links: Vec<Relationship>,
}
pub const MAX_LINKS: usize = 12;
impl Charter {
    pub fn validate(&self) -> Result<(), String> {
        if self.focus.chars().count() > 300
            || self.principle.chars().count() > 300
            || self.narrative.chars().count() > 3000
            || self.links.len() > MAX_LINKS
            || self.links.iter().any(|r| {
                r.from == r.to || r.label.trim().is_empty() || r.label.chars().count() > 60
            })
        {
            return Err("Invalid map charter".into());
        }
        Ok(())
    }
}
pub fn validate_topic(topic: &str) -> Result<(), String> {
    if topic.trim().is_empty() || topic != topic.trim() || topic.chars().count() > 36 {
        return Err("A parked topic needs a short keyword label".into());
    }
    Ok(())
}
pub fn validate_concept(concept: &str) -> Result<(), String> {
    if concept.trim().is_empty()
        || concept != concept.trim()
        || concept.chars().count() > 58
        || concept.split_whitespace().count() > 8
        || concept.contains(['\n', '\r'])
    {
        return Err("Concept must be a short phrase (up to eight words and 58 characters)".into());
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub sources: crate::evolution::Sources,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<crate::evolution::RetiredThought>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directions: Vec<String>,
    /// Thoughts waiting beside the map: not yet placeable, off topic, or asides.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parked: Vec<Parked>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charter: Option<Charter>,
    pub version: u32,
    pub title: String,
    pub thoughts: Vec<Thought>,
    pub next_id: u64,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub interpretation: Option<Interpretation>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            sources: Default::default(),
            retired: vec![],
            directions: vec![],
            parked: vec![],
            charter: None,
            version: 1,
            title: "A thought taking shape".into(),
            thoughts: vec![],
            next_id: 1,
            categories: vec![],
            interpretation: None,
        }
    }
}
impl Session {
    pub fn add(&mut self, text: &str) -> Option<u64> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.interpretation = None;
        self.thoughts.push(Thought {
            evidence: vec![],
            concept: None,
            kept: false,
            id,
            text: text.into(),
            role: classify(text),
            pinned: false,
            category: None,
        });
        Some(id)
    }
    pub fn move_thought(&mut self, id: u64) {
        self.interpretation = None;
        if let Some(t) = self.thoughts.iter_mut().find(|t| t.id == id) {
            if self.categories.is_empty() {
                t.role = t.role.next();
            } else {
                let next = t
                    .category
                    .as_ref()
                    .and_then(|id| self.categories.iter().position(|c| &c.id == id))
                    .map_or(0, |i| (i + 1) % self.categories.len());
                t.category = Some(self.categories[next].id.clone());
            }
            t.pinned = true;
        }
    }
    pub fn placement(&self, thought: &Thought) -> &str {
        thought
            .category
            .as_ref()
            .and_then(|id| self.categories.iter().find(|c| &c.id == id))
            .map_or(thought.role.label(), |c| c.name.as_str())
    }
    /// Root-to-leaf path. Traversal is bounded even for unvalidated external input.
    pub fn category_path(&self, id: Option<&str>) -> Vec<&Category> {
        let mut path = Vec::new();
        let mut cursor = id;
        while let Some(id) = cursor {
            let Some(c) = self.categories.iter().find(|c| c.id == id) else {
                break;
            };
            if path.len() >= MAX_CATEGORY_DEPTH || path.iter().any(|old: &&Category| old.id == c.id)
            {
                break;
            }
            path.push(c);
            cursor = c.parent.as_deref();
        }
        path.reverse();
        path
    }
    pub fn within_category(&self, thought: &Thought, category: &str) -> bool {
        self.category_path(thought.category.as_deref())
            .iter()
            .any(|c| c.id == category)
    }
    pub fn placement_path(&self, thought: &Thought) -> String {
        let path = self.category_path(thought.category.as_deref());
        if path.is_empty() {
            thought.role.label().into()
        } else {
            path.iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join(" › ")
        }
    }
    pub fn narrative(&self) -> String {
        if let Some(i) = &self.interpretation {
            return i.narrative.clone();
        }
        // The agent's last reading stays visible while a new capture is still being placed.
        if let Some(c) = self
            .charter
            .as_ref()
            .filter(|c| !c.narrative.trim().is_empty())
        {
            return c.narrative.clone();
        }
        if self.thoughts.is_empty() {
            return "Start anywhere. A question, a friction, a possibility.".into();
        }
        let mut parts = Vec::new();
        for (role, lead) in [
            (Role::Observation, "Noticing"),
            (Role::Question, "Wondering"),
            (Role::Tension, "Holding a tension"),
            (Role::Possibility, "Considering"),
        ] {
            if let Some(t) = self.thoughts.iter().rev().find(|t| t.role == role) {
                parts.push(format!("{lead}: {}", t.text));
            }
        }
        parts.join("\n\n")
    }
    /// Move a thought off the map. Its words, label and evidence travel with it.
    pub fn park(
        &mut self,
        id: u64,
        kind: ParkKind,
        topic: &str,
        reason: &str,
        held: bool,
    ) -> Result<(), String> {
        validate_topic(topic)?;
        let index = self
            .thoughts
            .iter()
            .position(|t| t.id == id)
            .ok_or("Unknown thought")?;
        let mut thought = self.thoughts.remove(index);
        thought.category = None;
        thought.pinned = false;
        thought.kept = false;
        self.parked.push(Parked {
            thought,
            kind,
            topic: topic.into(),
            reason: reason.into(),
            held,
        });
        Ok(())
    }
    /// Return a parked thought to the map, unplaced until someone gives it a branch.
    pub fn revive(&mut self, id: u64) -> Result<&mut Thought, String> {
        let index = self
            .parked
            .iter()
            .position(|p| p.thought.id == id)
            .ok_or("Unknown parked thought")?;
        self.thoughts.push(self.parked.remove(index).thought);
        Ok(self.thoughts.last_mut().unwrap())
    }
    /// Rebuild the reading the views use from the charter, once every thought has a home.
    pub fn refresh_reading(&mut self) {
        self.interpretation = None;
        let Some(charter) = &self.charter else {
            return;
        };
        if charter.narrative.trim().is_empty()
            || self.thoughts.is_empty()
            || self
                .thoughts
                .iter()
                .any(|t| t.category.is_none() || t.concept.is_none())
        {
            return;
        }
        let live = |id: &u64| self.thoughts.iter().any(|t| t.id == *id);
        let reading = Interpretation {
            categories: self.categories.clone(),
            title: self.title.clone(),
            narrative: charter.narrative.clone(),
            assignments: self
                .thoughts
                .iter()
                .map(|t| Assignment {
                    concept: t.concept.clone(),
                    id: t.id,
                    category: t.category.clone(),
                    role: t.role,
                })
                .collect(),
            relationships: charter
                .links
                .iter()
                .filter(|r| live(&r.from) && live(&r.to))
                .cloned()
                .collect(),
        };
        if reading.validate(self).is_ok() {
            self.interpretation = Some(reading);
        }
    }
    pub fn demo() -> Self {
        let mut session = Self {
            title: "Make room for meaningful work".into(),
            ..Self::default()
        };
        for text in [
            "Our calendars keep filling with meetings.",
            "Why is it unclear who can decide?",
            "But people need to feel included.",
            "What if each decision had one owner?",
            "We invite everyone just in case.",
            "We could gather input before the meeting.",
        ] {
            session.add(text);
        }
        session
    }
    pub fn validate(&self) -> Result<(), String> {
        self.check(true)
    }
    /// The same for a map midway through a regrouping, which may hold more root themes than
    /// a map is left with.
    pub fn validate_midway(&self) -> Result<(), String> {
        self.check(false)
    }
    fn check(&self, settled: bool) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!("Unsupported session version {}", self.version));
        }
        if settled {
            validate_categories(&self.categories)?;
        } else {
            validate_hierarchy(&self.categories)?;
        }
        let category_ids: std::collections::HashSet<_> =
            self.categories.iter().map(|c| &c.id).collect();
        if self.thoughts.iter().any(|t| {
            t.category
                .as_ref()
                .is_some_and(|id| !category_ids.contains(id))
        }) {
            return Err("Thought references an unknown theme".into());
        }
        let mut ids = std::collections::HashSet::new();
        for thought in &self.thoughts {
            if let Some(concept) = &thought.concept {
                validate_concept(concept)?;
            }
            if thought.id == 0 || !ids.insert(thought.id) || thought.id >= self.next_id {
                return Err("Invalid or duplicate thought ID".into());
            }
            if thought.text.trim().is_empty() {
                return Err("Empty thought in session".into());
            }
        }
        for r in &self.retired {
            if r.thought.id == 0
                || r.thought.id >= self.next_id
                || !ids.insert(r.thought.id)
                || r.reason.trim().is_empty()
                || r.reason.len() > 1000
                || r.replaced_by
                    .iter()
                    .any(|id| *id == 0 || *id >= self.next_id)
            {
                return Err("Invalid retired concept history".into());
            }
        }
        for p in &self.parked {
            let t = &p.thought;
            if t.id == 0
                || t.id >= self.next_id
                || !ids.insert(t.id)
                || t.text.trim().is_empty()
                || t.pinned
                || t.category.is_some()
                || p.reason.chars().count() > 300
            {
                return Err("Invalid parked thought".into());
            }
            validate_topic(&p.topic)?;
            if let Some(concept) = &t.concept {
                validate_concept(concept)?;
            }
        }
        if let Some(charter) = &self.charter {
            charter.validate()?;
        }
        for thought in self
            .thoughts
            .iter()
            .chain(self.retired.iter().map(|r| &r.thought))
            .chain(self.parked.iter().map(|p| &p.thought))
        {
            if thought
                .evidence
                .iter()
                .any(|id| !self.sources.contains_key(id))
            {
                return Err("Concept references missing source words".into());
            }
        }
        if let Some(i) = &self.interpretation {
            i.validate(self)?;
        }
        Ok(())
    }
}

/// A transparent offline starting point, not semantic AI. User corrections take precedence.
pub fn classify(text: &str) -> Role {
    let s = text.to_lowercase();
    let words: Vec<_> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if ["what if", "we could", "let's", "try ", "perhaps", "maybe"]
        .iter()
        .any(|p| s.starts_with(p))
    {
        Role::Possibility
    } else if ["but", "however", "although", "versus", "yet"]
        .iter()
        .any(|w| words.contains(w))
    {
        Role::Tension
    } else if s.ends_with('?')
        || ["why", "how", "what", "when", "who", "where"]
            .iter()
            .any(|w| words.first() == Some(w))
    {
        Role::Question
    } else {
        Role::Observation
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Category {
    /// Parent theme ID; absent in legacy flat maps and for root themes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub illustration: Option<String>,
    #[serde(default)]
    pub motif: Option<Motif>,
    pub id: String,
    pub name: String,
}
pub const MAX_CATEGORY_DEPTH: usize = 3;
pub const MAX_CATEGORIES: usize = 72;
const TOO_MANY: &str = "Use at most eight root themes and 72 themes in total";
pub fn validate_categories(categories: &[Category]) -> Result<(), String> {
    if categories.iter().filter(|c| c.parent.is_none()).count() > 8 {
        return Err(TOO_MANY.into());
    }
    validate_hierarchy(categories)
}
/// All that `validate_categories` asks but the count of root themes, which a regrouping in
/// progress may pass.
pub fn validate_hierarchy(categories: &[Category]) -> Result<(), String> {
    use std::collections::HashSet;
    if categories.len() > MAX_CATEGORIES {
        return Err(TOO_MANY.into());
    }
    let mut ids = HashSet::new();
    for c in categories {
        if c.id.is_empty()
            || c.id.len() > 60
            || c.name.trim().is_empty()
            || c.name.chars().count() > 36
            || !ids.insert(&c.id)
            || c.illustration
                .as_ref()
                .is_some_and(|s| s.chars().count() > 400)
        {
            return Err("Invalid or duplicate theme".into());
        }
    }
    for c in categories {
        let mut seen = HashSet::new();
        let mut current = Some(c);
        while let Some(node) = current {
            if !seen.insert(&node.id) {
                return Err("Theme hierarchy contains a cycle".into());
            }
            if seen.len() > MAX_CATEGORY_DEPTH {
                return Err("Theme hierarchy supports three classification levels".into());
            }
            current = match &node.parent {
                None => None,
                Some(id) => Some(
                    categories
                        .iter()
                        .find(|c| &c.id == id)
                        .ok_or("Unknown parent theme")?,
                ),
            };
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concept: Option<String>,
    pub id: u64,
    #[serde(default)]
    pub category: Option<String>,
    pub role: Role,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub from: u64,
    pub to: u64,
    pub label: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Interpretation {
    #[serde(default)]
    pub categories: Vec<Category>,
    pub title: String,
    pub narrative: String,
    pub assignments: Vec<Assignment>,
    pub relationships: Vec<Relationship>,
}
impl Interpretation {
    pub fn validate(&self, session: &Session) -> Result<(), String> {
        if self.title.trim().is_empty()
            || self.title.chars().count() > 100
            || self.narrative.trim().is_empty()
            || self.narrative.chars().count() > 3000
        {
            return Err("Organizer returned an invalid title or narrative".into());
        }
        validate_categories(&self.categories)?;
        let category_ids: std::collections::HashSet<_> =
            self.categories.iter().map(|c| &c.id).collect();
        // A pin protects the entire classification path, not just the leaf name.
        for t in session.thoughts.iter().filter(|t| t.pinned) {
            for original in session.category_path(t.category.as_deref()) {
                if !self.categories.iter().any(|c| {
                    c.id == original.id && c.name == original.name && c.parent == original.parent
                }) {
                    return Err("Review changed a pinned theme path".into());
                }
            }
        }
        let ids: std::collections::HashSet<_> = session.thoughts.iter().map(|t| t.id).collect();
        let mut assigned = std::collections::HashSet::new();
        for a in &self.assignments {
            if let Some(concept) = &a.concept {
                validate_concept(concept)?;
            }
            if (!self.categories.is_empty() && a.category.is_none())
                || a.category
                    .as_ref()
                    .is_some_and(|id| !category_ids.contains(id))
            {
                return Err("Unknown or missing theme assignment".into());
            }
            if !ids.contains(&a.id) || !assigned.insert(a.id) {
                return Err("Organizer returned an unknown or duplicate thought ID".into());
            }
        }
        if assigned != ids {
            return Err("Organizer did not classify every thought".into());
        }
        if self.relationships.len() > 12 {
            return Err("Too many relationships".into());
        }
        for r in &self.relationships {
            if !ids.contains(&r.from)
                || !ids.contains(&r.to)
                || r.from == r.to
                || r.label.trim().is_empty()
                || r.label.chars().count() > 60
            {
                return Err("Organizer returned an invalid relationship".into());
            }
        }
        Ok(())
    }
    pub fn apply(self, session: &mut Session) -> Result<(), String> {
        self.validate(session)?;
        for a in &self.assignments {
            if let Some(t) = session.thoughts.iter_mut().find(|t| t.id == a.id) {
                if let Some(concept) = &a.concept {
                    t.concept = Some(concept.clone());
                }
                if !t.pinned {
                    t.role = a.role;
                }
                // A legacy pin protects its narrative role; its first thematic
                // placement can still be established and is protected thereafter.
                if !t.pinned || t.category.is_none() {
                    t.category.clone_from(&a.category);
                }
            }
        }
        // Stable theme order preserves branch positions and colors between passes.
        let mut categories: Vec<_> = session
            .categories
            .iter()
            .filter_map(|old| self.categories.iter().find(|new| new.id == old.id).cloned())
            .collect();
        for c in &self.categories {
            if !categories.iter().any(|old| old.id == c.id) {
                categories.push(c.clone());
            }
        }
        session.categories = categories;
        session.title = self.title.clone();
        if let Some(charter) = &mut session.charter {
            charter.narrative.clone_from(&self.narrative);
            charter.links.clone_from(&self.relationships);
        }
        session.interpretation = Some(self);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motif {
    Clock,
    Compass,
    People,
    Sprout,
    Bridge,
    Mountain,
    Book,
    Bulb,
    Waves,
    Gears,
    Shield,
    Telescope,
}
impl Motif {
    pub const NAMES: [&'static str; 12] = [
        "clock",
        "compass",
        "people",
        "sprout",
        "bridge",
        "mountain",
        "book",
        "bulb",
        "waves",
        "gears",
        "shield",
        "telescope",
    ];
    pub fn infer(name: &str, words: &str) -> Self {
        let vocabulary = [
            (
                Self::Clock,
                "time calendar schedule meeting deadline hour pace",
            ),
            (
                Self::Compass,
                "decision direction ownership purpose choice strategy priorities",
            ),
            (
                Self::People,
                "people team inclusion included belonging community together trust",
            ),
            (
                Self::Sprout,
                "grow growth nature learning garden health nurture",
            ),
            (
                Self::Bridge,
                "connect connection bridge collaboration communication access",
            ),
            (
                Self::Mountain,
                "challenge goal ambition obstacle climb achievement risk",
            ),
            (
                Self::Book,
                "knowledge research reading study story stories education",
            ),
            (
                Self::Bulb,
                "idea creativity innovation possibility invent experiment",
            ),
            (
                Self::Waves,
                "water ocean flow energy balance change emotion",
            ),
            (
                Self::Gears,
                "system process tools technology automation building work",
            ),
            (
                Self::Shield,
                "safety protect security privacy boundaries resilience",
            ),
            (
                Self::Telescope,
                "future vision explore discovery question curiosity focus",
            ),
        ];
        let tokens = |s: &str| {
            s.to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let names = tokens(name);
        let words = tokens(words);
        vocabulary
            .into_iter()
            .map(|(motif, keys)| {
                let score = keys
                    .split_whitespace()
                    .map(|k| {
                        4 * usize::from(names.iter().any(|w| w == k))
                            + usize::from(words.iter().any(|w| w == k))
                    })
                    .sum::<usize>();
                (motif, score)
            })
            .max_by_key(|(_, score)| *score)
            .filter(|(_, score)| *score > 0)
            .map_or(Self::Sprout, |(motif, _)| motif)
    }
}
