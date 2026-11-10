//! Opening the encrypted store.
//!
//! Pattern: Facade — one function opens the store file with its key and hands back the core.
//!
//! **Provisional (ADR 0024):** the key vault does not exist yet, but the core opens on an encrypted store only. Until it
//! exists the key is a random 32-byte file `<store>.key` next to the store, readable by its owner only, made on the first
//! start. That protects the store file when it is copied away without the key file, and nothing more: anyone who can read
//! both files can read the messages. It is replaced by the vault, not extended.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs_messenger_core::{MessengerConfig, MessengerRepo};
use fs_objects::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, TursoStore};

/// Why the key could not be had.
#[derive(Debug)]
pub struct KeyProblem(pub String);

impl std::fmt::Display for KeyProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Who acts on this store. Provisional: there is no sign-in yet (as in the other programs).
const ACTOR: &str = "https://localhost/actors/owner";

/// The data folder of the user: `$XDG_DATA_HOME/freesynergy/messenger`, else `$HOME/.local/share/freesynergy/messenger`.
#[must_use]
pub fn default_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("freesynergy").join("messenger"))
}

fn key_path(store: &Path) -> PathBuf {
    let mut name = store.as_os_str().to_owned();
    name.push(".key");
    PathBuf::from(name)
}

fn read_or_make_key(path: &Path) -> Result<Vec<u8>, KeyProblem> {
    if let Ok(bytes) = std::fs::read(path) {
        return if bytes.len() == 32 {
            Ok(bytes)
        } else {
            Err(KeyProblem(format!("{}: not a 32-byte key", path.display())))
        };
    }
    let mut key = vec![0_u8; 32];
    getrandom::fill(&mut key).map_err(|e| KeyProblem(format!("no random bytes: {e}")))?;
    write_private(path, &key).map_err(|e| KeyProblem(format!("{}: {e}", path.display())))?;
    Ok(key)
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    // No owner-only mode here; the file is in the user's own data folder. Named in DESIGN.md.
    std::fs::write(path, bytes)
}

/// Open the core on the encrypted store `file` (or the default one in the data folder).
///
/// # Errors
/// The message for the person: no data folder, a key that cannot be had, a store that does not open.
pub async fn open_repo(file: Option<&Path>) -> Result<MessengerRepo, String> {
    let file = match file {
        Some(f) => f.to_owned(),
        None => {
            let dir = default_dir()
                .ok_or("no data folder: give --store or set $XDG_DATA_HOME or $HOME")?;
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            dir.join("messages.db")
        }
    };
    let key = read_or_make_key(&key_path(&file)).map_err(|e| e.to_string())?;
    let options = OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key,
        }))),
        ..OpenOptions::default()
    };
    let store = TursoStore::open_with(&file, options)
        .await
        .map_err(|e| format!("cannot open the store: {e}"))?;
    MessengerRepo::open_turso(Arc::new(store), MessengerConfig::new(ACTOR))
        .await
        .map_err(|e| format!("cannot open the messenger: {e}"))
}
