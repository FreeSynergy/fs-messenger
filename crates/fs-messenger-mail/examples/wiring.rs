//! How a program puts a mail account together: IMAP or JMAP to read, SMTP or JMAP to send, keys in an
//! encrypted store. Run with `cargo run --example wiring`; it connects to nothing and prints the plan.

use std::sync::Arc;

use fs_messenger_core::Provider;
use fs_messenger_mail::fetch::{DEFAULT_IDLE_TIMEOUT, ImapConfig, ImapFetch};
use fs_messenger_mail::keys::FsKeyStore;
use fs_messenger_mail::submit::{SmtpConfig, SmtpSubmit};
use fs_messenger_mail::{Endpoint, MailIdentity, MailProvider, Password, TlsTrust};
use fs_objects::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, TursoStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = "me@example.org";
    let endpoint = |port| Endpoint {
        host: "mail.example.org".to_string(),
        port,
        trust: TlsTrust::SystemRoots,
    };
    let password = Password::new("from the person, never from a file");
    let fetch = ImapFetch::new(ImapConfig {
        endpoint: endpoint(993),
        user: address.to_string(),
        password: password.clone(),
        idle_timeout: DEFAULT_IDLE_TIMEOUT,
    });
    let submit = SmtpSubmit::new(SmtpConfig {
        endpoint: endpoint(465),
        user: address.to_string(),
        password,
    });

    let dir = tempfile::tempdir()?;
    let options = OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32], // A real program takes this from the key vault (ADR 0024).
        }))),
        ..OpenOptions::default()
    };
    let store = TursoStore::open_with(dir.path().join("keys.db"), options).await?;
    let keys = Arc::new(FsKeyStore::open_turso(
        Arc::new(store),
        "https://local.invalid/me",
    )?);

    let provider = MailProvider::new(
        MailIdentity::new(address),
        Arc::new(fetch),
        Arc::new(submit),
        keys,
    );
    let key = provider.ensure_own_key().await?;
    println!("own key: {key:?}");
    let plan = provider
        .encryption_plan(&["someone@example.org".to_string()])
        .await?;
    println!(
        "plan: {:?}, lacking a key: {:?}",
        plan.encryption, plan.missing_keys
    );
    Ok(())
}
