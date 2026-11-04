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
    pub id: u64,
    pub text: String,
    pub role: Role,
    pub pinned: bool,
    #[serde(default)]
    pub category: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
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
    pub fn narrative(&self) -> String {
        if let Some(i) = &self.interpretation {
            return i.narrative.clone();
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
        if self.version != 1 {
            return Err(format!("Unsupported session version {}", self.version));
        }
        let mut category_ids = std::collections::HashSet::new();
        if self.categories.len() > 8 {
            return Err("Too many saved themes".into());
        }
        for c in &self.categories {
            if c.id.is_empty()
                || c.id.len() > 60
                || c.name.trim().is_empty()
                || c.name.chars().count() > 36
                || !category_ids.insert(&c.id)
            {
                return Err("Invalid saved theme".into());
            }
        }
        if self.thoughts.iter().any(|t| {
            t.category
                .as_ref()
                .is_some_and(|id| !category_ids.contains(id))
        }) {
            return Err("Thought references an unknown theme".into());
        }
        let mut ids = std::collections::HashSet::new();
        for thought in &self.thoughts {
            if thought.id == 0 || !ids.insert(thought.id) || thought.id >= self.next_id {
                return Err("Invalid or duplicate thought ID".into());
            }
            if thought.text.trim().is_empty() {
                return Err("Empty thought in session".into());
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
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
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
        let mut category_ids = std::collections::HashSet::new();
        if self.categories.len() > 8 {
            return Err("Too many themes (maximum eight)".into());
        }
        for c in &self.categories {
            if c.id.is_empty()
                || c.id.len() > 60
                || c.name.trim().is_empty()
                || c.name.chars().count() > 36
                || !category_ids.insert(&c.id)
            {
                return Err("Invalid or duplicate theme".into());
            }
        }
        for t in session.thoughts.iter().filter(|t| t.pinned) {
            if let Some(id) = &t.category {
                let original = session.categories.iter().find(|c| &c.id == id);
                if self.categories.iter().find(|c| &c.id == id) != original {
                    return Err("Review changed a pinned theme".into());
                }
            }
        }
        let ids: std::collections::HashSet<_> = session.thoughts.iter().map(|t| t.id).collect();
        let mut assigned = std::collections::HashSet::new();
        for a in &self.assignments {
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
        session.interpretation = Some(self);
        Ok(())
    }
}
