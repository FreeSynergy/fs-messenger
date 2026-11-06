//! How a program puts a Matrix account together: keys in an encrypted store, messages in another.
//! Run with `cargo run --example wiring`; it connects to nothing and shows the first answer of a new
//! store: no session, so a password login comes next.

use std::sync::Arc;

use fs_messenger_matrix::{MatrixAccount, MatrixError, MatrixProvider};
use fs_store::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, TursoStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let options = OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32], // A real program takes this from the key vault (ADR 0024).
        }))),
        ..OpenOptions::default()
    };
    let keys = Arc::new(TursoStore::open_with(dir.path().join("keys.db"), options).await?);
    let account = MatrixAccount::new("https://matrix.example.org", "@me:example.org");

    // `MatrixProvider::login(account, &Password::new(..), keys)` for the first time, then:
    match MatrixProvider::restore(account, keys).await {
        Err(MatrixError::NoSession) => println!("no session yet: log in with a password"),
        Err(other) => return Err(other.into()),
        Ok(provider) => println!("back as {provider:?}"),
    }
    // Then `MessengerRepo::open_turso` on a second encrypted store, `add_account` with the Matrix id
    // and kind `matrix`, and `register_provider`. `tests/server/common.rs` shows it whole.
    Ok(())
}
