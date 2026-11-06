//! A person: a real provider and a real core, each on its own encrypted store in a temp dir.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fs_messenger_core::{
    Account, AccountSetup, Ingest, Message, Messenger, MessengerConfig, MessengerRepo,
    Result as CoreResult, SearchFilter, SyncReport,
};
use fs_messenger_matrix::{MatrixAccount, MatrixProvider, Password};
use fs_store::ObjectId;
use tempfile::TempDir;

use crate::http;

fn key() -> Vec<u8> {
    vec![7; 32]
}

/// The store the SDK's keys live in.
pub async fn open_keys(path: &Path) -> fs_store_unlogged::TursoStore {
    use fs_store_unlogged::{Cipher, EncryptionKey, OpenOptions, StaticKeyProvider, TursoStore};
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
    pub keys: Arc<fs_store_unlogged::TursoStore>,
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
        let repo = MessengerRepo::open_turso(messages, config).await.unwrap();
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
        };
        person.sync().await; // Uploads the device's keys.
        person
    }

    /// One sync through the core.
    pub async fn sync(&self) -> SyncReport {
        self.repo.sync(self.account.id).await.expect("sync")
    }

    /// Send through the core's `Draft`.
    pub async fn send(&self, to: &[&str], text: &str) -> CoreResult<ObjectId> {
        let mut draft = self.repo.compose(None).await?;
        draft.set_account(self.account.id).await?;
        let to = to.iter().map(ToString::to_string).collect();
        draft.set_recipients(to, vec![], vec![]).await?;
        draft
            .set_text(text.to_owned(), "text/plain".to_owned())
            .await?;
        draft.send(None).await
    }

    /// Answer a message through the core's `Draft`.
    pub async fn reply(&self, to: ObjectId, text: &str) -> CoreResult<ObjectId> {
        let mut draft = self.repo.compose(Some(to)).await?;
        draft
            .set_text(text.to_owned(), "text/plain".to_owned())
            .await?;
        draft.send(None).await
    }

    /// All stored messages holding `text`.
    pub async fn find(&self, text: &str) -> Vec<Message> {
        let page = self
            .repo
            .search(text, &SearchFilter::default(), None)
            .await
            .unwrap();
        let mut found = Vec::new();
        for summary in page.items {
            let context = summary.envelope.context.as_deref().unwrap();
            let conversation: ObjectId = context.parse().unwrap();
            let id = summary.envelope.id;
            found.push(self.repo.message(conversation, id).await.unwrap());
        }
        found
    }

    /// Sync until `text` is here, or fail.
    pub async fn wait_for(&self, text: &str) -> Message {
        for _ in 0..80 {
            self.sync().await;
            if let Some(found) = self.find(text).await.pop() {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        panic!("{} never got {text}", self.user_id);
    }

    /// Sync until invited, join, and sync the join in. Answers the room.
    pub async fn join_next_invite(&self) -> String {
        for _ in 0..80 {
            self.sync().await;
            if let Some(invite) = self.provider.invites().await.unwrap().pop() {
                self.provider.accept_invite(&invite.room_id).await.unwrap();
                self.sync().await;
                return invite.room_id;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        panic!("{} was never invited", self.user_id);
    }

    /// A token that reads what the server holds, from a device that has no keys.
    pub async fn server_view(&self) -> String {
        http::plain_login(&self.user_id, &self.password).await
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
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

/// The alias of `m` in `scheme`.
pub fn alias_of(m: &Message, scheme: &str) -> String {
    let known = m.aliases.iter().chain(&m.receipts);
    known
        .filter(|a| a.scheme == scheme)
        .map(|a| a.id.clone())
        .next()
        .expect("an alias")
}
