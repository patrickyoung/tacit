use crate::model::Session;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("TACIT_SESSION") {
        return path.into();
    }
    if let Some(base) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(base).join("tacit/session.json");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/tacit/session.json");
    }
    PathBuf::from(".tacit/session.json")
}
pub fn load(path: &Path) -> io::Result<Session> {
    match fs::read(path) {
        Ok(bytes) => {
            let session: Session = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            session.validate().map_err(io::Error::other)?;
            Ok(session)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Session::default()),
        Err(e) => Err(e),
    }
}
pub fn save(path: &Path, session: &Session) -> io::Result<()> {
    session.validate().map_err(io::Error::other)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(session).map_err(io::Error::other)?;
    use std::io::Write;
    let mut file = fs::File::create(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(temporary, path)
}

/// Keep this file alive for the session. A second window must not overwrite the first.
pub fn lock(path: &Path) -> io::Result<fs::File> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    file.try_lock().map_err(|e| {
        io::Error::other(format!("Session is already open or cannot be locked: {e}"))
    })?;
    Ok(file)
}
