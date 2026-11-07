//! A person: a real provider and a real core, each on its own encrypted store in a temp dir.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fs_messenger_core::{
    Account, AccountSetup, Message, Messenger, MessengerConfig, MessengerRepo,
};
use fs_messenger_matrix::{EventAlias, MatrixAccount, MatrixProvider, Password};
use tempfile::TempDir;

use crate::http;

fn key() -> Vec<u8> {
    vec![7; 32]
}

/// The store the SDK's keys live in.
pub async fn open_keys(path: &Path) -> fs_store::TursoStore {
    use fs_store::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, TursoStore};
    let options = OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: key(),
        }))),
        ..OpenOptions::default()
    };
    TursoStore::open_with(path, options).await.unwrap()
}

/// The store of the core's messages.
async fn open_messages(path: &Path) -> fs_store::TursoStore {
    use fs_store::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, TursoStore};
    let options = OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: key(),
        }))),
        ..OpenOptions::default()
    };
    TursoStore::open_with(path, options).await.unwrap()
}

/// A short unique word, so that tests can run side by side on one server.
pub fn token() -> String {
    let id = fs_store::ObjectId::new().to_string().replace('-', "");
    id[id.len() - 12..].to_owned()
}

/// A user on the server, and where this person's data lives.
pub struct Person {
    pub user_id: String,
    pub password: String,
    pub dir: Arc<TempDir>,
    pub provider: Arc<MatrixProvider>,
    pub repo: MessengerRepo,
    pub account: Account,
    pub keys: Arc<fs_store::TursoStore>,
    pub messages: Arc<fs_store::TursoStore>,
}

/// A name for a new user.
pub fn new_user(name: &str) -> (String, String) {
    (
        format!("{name}-{}", token()),
        format!("throwaway-{}", token()),
    )
}

/// The server's name for a local user.
pub fn mxid(local: &str) -> String {
    format!(
        "@{local}:{}",
        http::setting("FS_MSGTEST_MATRIX_SERVER_NAME")
    )
}

impl Person {
    /// Register a new user and log in, with a new device.
    pub async fn new(name: &str) -> Self {
        Self::with_limit(name, None).await
    }

    /// As `new`, with a timeline limit per `/sync`.
    pub async fn with_limit(name: &str, limit: Option<u32>) -> Self {
        let (local, password) = new_user(name);
        http::register(&local, &password).await;
        let dir = Arc::new(TempDir::new().unwrap());
        Self::start(&local, password, dir, limit, true).await
    }

    /// A second device of the same user, starting from nothing.
    pub async fn second_device(&self) -> Self {
        let local = self.user_id[1..self.user_id.find(':').unwrap()].to_owned();
        let dir = Arc::new(TempDir::new().unwrap());
        Self::start(&local, self.password.clone(), dir, None, true).await
    }

    /// Close everything and come back, as after a restart: same files, a new process's worth of objects.
    pub async fn restart(self) -> Self {
        let local = self.user_id[1..self.user_id.find(':').unwrap()].to_owned();
        let (password, dir) = (self.password.clone(), self.dir.clone());
        drop(self.repo);
        drop(self.provider);
        drop(self.keys);
        drop(self.messages);
        tokio::time::sleep(Duration::from_millis(100)).await;
        Self::start(&local, password, dir, None, false).await
    }

    async fn start(
        local: &str,
        password: String,
        dir: Arc<TempDir>,
        limit: Option<u32>,
        first: bool,
    ) -> Self {
        let user_id = mxid(local);
        let keys = Arc::new(open_keys(&dir.path().join("keys.db")).await);
        let mut setup = MatrixAccount::new(http::base(), user_id.clone());
        if let Some(limit) = limit {
            setup.timeline_limit = limit;
        }
        let provider = if first {
            MatrixProvider::login(setup, &Password::new(password.clone()), keys.clone())
                .await
                .expect("login")
        } else {
            // No password here: a restart must not need one.
            MatrixProvider::restore(setup, keys.clone())
                .await
                .expect("restore")
        };
        let provider = Arc::new(provider);
        let messages = Arc::new(open_messages(&dir.path().join("messages.db")).await);
        let config = MessengerConfig::new(format!("https://local.invalid/users/{local}"));
        let repo = MessengerRepo::open_turso(messages.clone(), config)
            .await
            .unwrap();
        let account = match repo.accounts().await.unwrap().pop() {
            Some(account) => account,
            None => {
                let setup = AccountSetup {
                    address: user_id.clone(),
                    kind: "matrix".into(),
                    secret: String::new(),
                };
                repo.add_account(setup).await.unwrap()
            }
        };
        repo.register_provider(account.id, provider.clone());
        let person = Self {
            user_id,
            password,
            dir,
            provider,
            repo,
            account,
            keys,
            messages,
        };
        person.sync().await; // Uploads the device's keys.
        person
    }
}

/// Alice opens an encrypted room with Bob, and Bob joins. Answers the room.
pub async fn encrypted_room(alice: &Person, bob: &Person) -> String {
    let room = alice
        .provider
        .create_encrypted_room(std::slice::from_ref(&bob.user_id))
        .await
        .unwrap();
    assert_eq!(bob.join_next_invite().await, room);
    alice.sync().await;
    room
}

/// The Matrix event id inside the alias of `m`: the alias is `<room>|<event>`.
pub fn event_id_of(m: &Message) -> String {
    let mut known = m.aliases.iter().chain(&m.receipts);
    known
        .find_map(EventAlias::from_external)
        .expect("an event alias")
        .event
}
