//! A session library beside the existing notebook; each archive remains self-contained.
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    /// Missing in older libraries. Only their placeholder names become automatic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automatic: Option<bool>,
    #[serde(skip)]
    pub path: PathBuf,
}

impl Entry {
    fn is_automatic(&self) -> bool {
        self.automatic.unwrap_or_else(|| placeholder(&self.name))
    }
}

fn placeholder(name: &str) -> bool {
    matches!(
        name.trim().to_lowercase().as_str(),
        "" | "new session"
            | "a new session"
            | "untitled"
            | "untitled session"
            | "original session"
            | "a thought taking shape"
            | "a living conversation"
    )
}

fn conversation_name(session: &crate::model::Session, fallback: &str) -> String {
    let source = if !placeholder(&session.title) {
        &session.title
    } else if let Some(thought) = session.thoughts.first() {
        thought.concept.as_deref().unwrap_or(&thought.text)
    } else {
        return fallback.to_owned();
    };
    let words = source.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() <= 64 {
        return words;
    }
    let shortened = words.chars().take(61).collect::<String>();
    // Prefer complete words, while still handling scripts with no word separators.
    let end = shortened.rfind(' ').filter(|&i| i >= shortened.len() / 2);
    format!(
        "{}…",
        end.map_or(shortened.as_str(), |i| &shortened[..i])
            .trim_end()
    )
}

pub fn list(base: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = vec![Entry {
        name: "Original session".into(),
        automatic: Some(true),
        path: base.to_owned(),
    }];
    resolve_name(base, &mut entries[0])?;
    let root = root(base);
    let dirs = match fs::read_dir(root) {
        Ok(dirs) => dirs,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(entries),
        Err(e) => return Err(e),
    };
    for dir in dirs {
        let dir = dir?;
        if !dir.file_type()?.is_dir() {
            continue;
        }
        let path = dir.path().join("session.json");
        if let Ok(bytes) = fs::read(dir.path().join("name.json")) {
            let mut entry: Entry = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            entry.path = path;
            resolve_name(base, &mut entry)?;
            entries.push(entry);
        }
    }
    // Old, still-empty placeholders need distinct labels too. Use creation-path
    // order, not filesystem iteration order, so their labels remain predictable.
    entries[1..].sort_by(|a, b| a.path.cmp(&b.path));
    for i in 1..entries.len() {
        if entries[i].is_automatic() && placeholder(&entries[i].name) {
            entries[i].name = next_conversation_name(&entries);
        }
    }
    entries[1..].sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.path.cmp(&b.path))
    });
    Ok(entries)
}

fn resolve_name(base: &Path, entry: &mut Entry) -> io::Result<()> {
    if entry.path == base
        && let Ok(bytes) = fs::read(name_path(base, base))
    {
        let saved: Entry = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        entry.name = saved.name;
        entry.automatic = saved.automatic;
    }
    if entry.is_automatic() {
        entry.automatic = Some(true);
        // The compatibility snapshot is sufficient for a library label. Browsing does
        // not replay large frame archives, call a model, or rewrite old metadata.
        if let Ok(session) = crate::storage::load(&entry.path) {
            entry.name = conversation_name(&session, &entry.name);
        }
    }
    Ok(())
}
fn root(base: &Path) -> PathBuf {
    base.with_extension("sessions")
}
fn name_path(path: &Path, base: &Path) -> PathBuf {
    if path == base {
        base.with_extension("name.json")
    } else {
        path.with_file_name("name.json")
    }
}
pub fn rename(base: &Path, path: &Path, name: &str) -> io::Result<()> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(io::Error::other(
            "Use a session name between 1 and 100 characters",
        ));
    }
    write_name(base, path, name, false)
}

fn write_name(base: &Path, path: &Path, name: &str, automatic: bool) -> io::Result<()> {
    let target = name_path(path, base);
    let temporary = target.with_extension("tmp");
    let bytes = serde_json::to_vec(&Entry {
        name: name.into(),
        automatic: Some(automatic),
        path: path.into(),
    })?;
    use io::Write;
    let mut file = fs::File::create(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(temporary, target)
}
pub fn create(base: &Path, name: &str) -> io::Result<Entry> {
    if name.trim().is_empty() || name.trim().chars().count() > 100 {
        return Err(io::Error::other(
            "Use a session name between 1 and 100 characters",
        ));
    }
    let root = root(base);
    fs::create_dir_all(&root)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let dir = root.join(format!("{stamp}-{}", std::process::id()));
    fs::create_dir(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.join("session.json");
    crate::storage::save(&path, &crate::model::Session::default())?;
    rename(base, &path, name)?;
    Ok(Entry {
        name: name.trim().into(),
        automatic: Some(false),
        path,
    })
}

/// Start immediately; empty conversations get distinct labels until they have a topic.
pub fn create_automatic(base: &Path) -> io::Result<Entry> {
    let entries = list(base)?;
    let mut entry = create(base, &next_conversation_name(&entries))?;
    write_name(base, &entry.path, &entry.name, true)?;
    entry.automatic = Some(true);
    Ok(entry)
}

fn next_conversation_name(entries: &[Entry]) -> String {
    let index = (1..)
        .find(|i| {
            !entries
                .iter()
                .any(|e| e.name == format!("Conversation {i}"))
        })
        .unwrap();
    format!("Conversation {index}")
}

/// Follow the existing map title without overwriting a user-chosen name.
pub fn update_automatic_name(
    base: &Path,
    path: &Path,
    session: &crate::model::Session,
) -> io::Result<String> {
    let saved = match fs::read(name_path(path, base)) {
        Ok(bytes) => serde_json::from_slice::<Entry>(&bytes).map_err(io::Error::other)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Entry {
            name: "Original session".into(),
            automatic: Some(true),
            path: path.to_owned(),
        },
        Err(e) => return Err(e),
    };
    if !saved.is_automatic() {
        return Ok(saved.name);
    }
    let mut name = conversation_name(session, &saved.name);
    if path != base
        && placeholder(&name)
        && let Some(entry) = list(base)?.into_iter().find(|e| e.path == path)
    {
        name = entry.name;
    }
    if name != saved.name || saved.automatic != Some(true) {
        write_name(base, path, &name, true)?;
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_sessions_and_names_survive_reopening() {
        let root = std::env::temp_dir().join(format!(
            "tacit-library-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let base = root.join("session.json");
        fs::create_dir_all(&root).unwrap();
        let a = create(&base, "Research / ideas").unwrap();
        let b = create(&base, "Research / ideas").unwrap();
        assert_ne!(a.path, b.path);
        let mut session = crate::model::Session::default();
        session.add("Only in the first session");
        crate::storage::save(&a.path, &session).unwrap();
        assert!(crate::storage::load(&b.path).unwrap().thoughts.is_empty());
        rename(&base, &a.path, "Renamed").unwrap();
        rename(&base, &base, "My original").unwrap();
        let entries = list(&base).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "My original");
        assert!(
            entries
                .iter()
                .any(|e| e.name == "Renamed" && e.path == a.path)
        );
        assert!(create(&base, "  ").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn automatic_names_follow_content_and_preserve_explicit_renames() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("session.json");
        let a = create_automatic(&base).unwrap();
        let b = create_automatic(&base).unwrap();
        assert_eq!(a.name, "Conversation 1");
        assert_eq!(b.name, "Conversation 2");
        let mut session = crate::model::Session::default();
        session.add("How can we make onboarding simpler?");
        assert_eq!(
            update_automatic_name(&base, &a.path, &session).unwrap(),
            "How can we make onboarding simpler?"
        );
        session.title = "A better first week".into();
        crate::storage::save(&a.path, &session).unwrap();
        update_automatic_name(&base, &a.path, &session).unwrap();
        assert_eq!(
            list(&base)
                .unwrap()
                .iter()
                .find(|e| e.path == a.path)
                .unwrap()
                .name,
            "A better first week"
        );
        // Even an explicitly chosen placeholder is a name, not permission to rename it.
        rename(&base, &a.path, "New session").unwrap();
        session.title = "A later interpretation".into();
        assert_eq!(
            update_automatic_name(&base, &a.path, &session).unwrap(),
            "New session"
        );
        assert_eq!(
            list(&base)
                .unwrap()
                .iter()
                .find(|e| e.path == a.path)
                .unwrap()
                .name,
            "New session"
        );
    }

    #[test]
    fn legacy_placeholders_get_topics_without_rewriting_library_files() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("session.json");
        let a = create(&base, "New session").unwrap();
        let b = create(&base, "New session").unwrap();
        for entry in [&a, &b] {
            fs::write(name_path(&entry.path, &base), br#"{"name":"New session"}"#).unwrap();
        }
        let empty = list(&base).unwrap();
        assert_ne!(
            empty.iter().find(|e| e.path == a.path).unwrap().name,
            empty.iter().find(|e| e.path == b.path).unwrap().name
        );
        assert_eq!(
            update_automatic_name(&base, &b.path, &crate::model::Session::default()).unwrap(),
            empty.iter().find(|e| e.path == b.path).unwrap().name
        );
        let mut session = crate::model::Session::default();
        session.add("Make time for research");
        crate::storage::save(&a.path, &session).unwrap();
        session.title = "Reduce hosting costs".into();
        crate::storage::save(&b.path, &session).unwrap();
        let entries = list(&base).unwrap();
        assert_eq!(
            entries.iter().find(|e| e.path == a.path).unwrap().name,
            "Make time for research"
        );
        assert_eq!(
            entries.iter().find(|e| e.path == b.path).unwrap().name,
            "Reduce hosting costs"
        );
        assert_eq!(
            fs::read(name_path(&a.path, &base)).unwrap(),
            br#"{"name":"New session"}"#
        );
    }

    #[test]
    fn original_name_loads_without_other_sessions_and_titles_stay_compact() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("session.json");
        rename(&base, &base, "My notebook").unwrap();
        assert_eq!(list(&base).unwrap()[0].name, "My notebook");
        rename(&base, &base, "Conversation 1").unwrap();
        assert_eq!(create_automatic(&base).unwrap().name, "Conversation 2");
        let session = crate::model::Session {
            title: "研究".repeat(50),
            ..Default::default()
        };
        let name = conversation_name(&session, "Conversation 1");
        assert!(name.chars().count() <= 64);
        assert!(name.ends_with('…'));
    }
}
