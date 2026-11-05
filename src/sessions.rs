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
    #[serde(skip)]
    pub path: PathBuf,
}

pub fn list(base: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = vec![Entry {
        name: "Original session".into(),
        path: base.to_owned(),
    }];
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
            entries.push(entry);
        }
    }
    entries[1..].sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.path.cmp(&b.path))
    });
    if let Ok(bytes) = fs::read(base.with_extension("name.json")) {
        entries[0].name = serde_json::from_slice::<Entry>(&bytes)
            .map_err(io::Error::other)?
            .name;
    }
    Ok(entries)
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
    let target = name_path(path, base);
    let temporary = target.with_extension("tmp");
    let bytes = serde_json::to_vec(&Entry {
        name: name.into(),
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
        path,
    })
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
}
