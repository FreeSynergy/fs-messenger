//! The stop marker: a small file beside the store, written before the store is.
//!
//! Pattern: Memento: what a stop was, in a place that does not depend on the store working. It is read
//! at open and removed only after a written `reopen`. Its presence alone is a stop: unreadable content
//! reads as a stop by nobody. The disk can stall too, so all of it runs off the async threads, and the
//! engine bounds how long it waits. Nothing in it is secret: who, when, and the hash of a used proof.

use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::state::StopMark;

/// Who a stop is by when nothing says otherwise.
pub(crate) const NOBODY: &str = "";

/// What the file says, if there is one. An error other than "not there" reads as a stop.
pub(crate) fn read(path: &Path) -> Option<StopMark> {
    let unknown = || StopMark {
        by: NOBODY.to_string(),
        at: Utc::now(),
        used: None,
    };
    match fs::read(path) {
        Ok(bytes) => Some(serde_json::from_slice(&bytes).unwrap_or_else(|_| unknown())),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(_) => Some(unknown()),
    }
}

fn write_now(path: &Path, mark: &StopMark) -> std::io::Result<()> {
    let mut temp = PathBuf::from(path);
    temp.as_mut_os_string().push(".new");
    let bytes = serde_json::to_vec(mark).map_err(std::io::Error::other)?;
    let mut file = fs::File::create(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    if let Some(folder) = path.parent() {
        fs::File::open(folder)?.sync_all()?;
    }
    Ok(())
}

/// Write it, atomically and to the disk.
pub(crate) async fn write(path: PathBuf, mark: StopMark) -> Result<(), String> {
    tokio::task::spawn_blocking(move || write_now(&path, &mark))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Take it away. Not there is fine.
pub(crate) async fn remove(path: PathBuf) -> Result<(), String> {
    tokio::task::spawn_blocking(move || match fs::remove_file(&path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    })
    .await
    .map_err(|e| e.to_string())?
}
