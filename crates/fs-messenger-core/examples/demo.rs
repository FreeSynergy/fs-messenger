//! Pattern: Facade in use. The core end to end, over a real encrypted store, with a provider made up for this demo:
//! `cargo run -p fs-messenger-core --example demo`

use std::sync::Arc;

use async_trait::async_trait;
use fs_messenger_core::{
    AccountSetup, Encryption, EncryptionPlan, Error, ExternalId, Ingest, Kind, Messenger,
    MessengerConfig, MessengerRepo, Outgoing, Provider, RawMessage, Result, SendReceipt,
};
use fs_store::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, TursoStore};

/// Knows a key for Ada only, and sends nowhere.
struct DemoProvider;

#[async_trait]
impl Provider for DemoProvider {
    async fn send(&self, message: &Outgoing) -> Result<SendReceipt> {
        println!(
            "provider: sends to {:?} with {:?}",
            message.to(),
            message.encryption()
        );
        Ok(SendReceipt {
            aliases: vec![ExternalId::new("demo", message.id().to_string())],
            conversation: None,
            encryption: message.encryption(),
        })
    }

    async fn encryption_plan(&self, recipients: &[String]) -> Result<EncryptionPlan> {
        Ok(EncryptionPlan {
            encryption: Encryption::Openpgp,
            missing_keys: recipients
                .iter()
                .filter(|r| *r != "ada@example.org")
                .cloned()
                .collect(),
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("fs-messenger-demo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let options = OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![42; 32],
        }))),
        ..OpenOptions::default()
    };
    let store = TursoStore::open_with(dir.join("messages.db"), options).await?;
    let config = MessengerConfig::new("https://local.invalid/users/owner");
    let repo = MessengerRepo::open_turso(Arc::new(store), config).await?;

    let me = repo
        .add_account(AccountSetup {
            address: "me@example.org".into(),
            kind: "mail".into(),
            secret: String::new(),
        })
        .await?;
    repo.register_provider(me.id, Arc::new(DemoProvider));

    let thread = ExternalId::new("thread", "1");
    // What the transport calls it, here a made-up id: a Message-ID would only link.
    let mut hello = RawMessage::new(
        ExternalId::new("demo-uid", "a"),
        thread.clone(),
        Kind::Mail,
        "ada@example.org",
        "Hello!",
    );
    hello.encryption = Encryption::None;
    println!(
        "first delivery: {:?}",
        repo.ingest(me.id, hello.clone()).await?
    );
    println!("replay:         {:?}", repo.ingest(me.id, hello).await?);

    let conversation = repo.conversations(None, false, None).await?.items.remove(0);
    let shown = conversation.last_message.expect("the message");
    println!("incoming marked as {:?}", shown.encryption);

    let mut draft = repo.compose(Some(shown.envelope.id)).await?;
    draft.set_text("Hi Ada".into(), "text/plain".into()).await?;
    let id = draft.send(None).await?;
    println!("sent as {id}");

    let mut draft = repo.compose(None).await?;
    draft
        .set_recipients(vec!["eve@example.org".into()], vec![], vec![])
        .await?;
    match draft.send(None).await {
        Err(e @ Error::EncryptionRefused { .. }) => println!("{e}"),
        other => println!("unexpected: {other:?}"),
    }
    std::fs::remove_dir_all(&dir).expect("cleaning up");
    Ok(())
}
