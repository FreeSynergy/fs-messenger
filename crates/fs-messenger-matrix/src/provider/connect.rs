//! Logging in, and coming back after a restart.
//!
//! Pattern: Factory: builds the SDK client over the stores of `fs-store`. The session lives in the
//! same encrypted store as the keys. A stored session always wins over a password: a new login would
//! be a new device, with new keys, and the old messages would stay unreadable.

use std::sync::Arc;

use fs_store_unlogged::TursoStore;
use matrix_sdk::Client;
use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::config::StoreConfig;
use matrix_sdk::cross_process_lock::CrossProcessLockConfig;
use matrix_sdk::store::RoomLoadSettings;

use super::{MatrixAccount, MatrixProvider};
use crate::error::{MatrixError, Result};
use crate::secret::Password;
use crate::store::{FsCryptoStore, FsJournal, FsStateStore, Journal, Op};

const SESSION_KEY: &str = "session";

/// reqwest is built without a crypto provider, so it needs a process-wide one. Installing twice is
/// not an error: whoever came first stays.
fn install_tls() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

struct Opened {
    client: Client,
    sessions: FsJournal,
    saved: Option<MatrixSession>,
}

async fn open(account: &MatrixAccount, store: Arc<TursoStore>) -> Result<Opened> {
    account.check_homeserver()?;
    install_tls();
    let name = |kind: &str| format!("matrix-{kind}:{}", account.user_id);
    let crypto = FsJournal::open(store.clone(), name("crypto")).await?;
    let state = FsJournal::open(store.clone(), name("state")).await?;
    let sessions = FsJournal::open(store, name("session")).await?;
    let saved = match sessions.load().await?.get(SESSION_KEY) {
        Some(text) => Some(serde_json::from_str(text).map_err(crate::StoreFail::from)?),
        None => None,
    };
    let config = StoreConfig::new(CrossProcessLockConfig::SingleProcess)
        .state_store(FsStateStore::open(Arc::new(state)).await?)
        .crypto_store(FsCryptoStore::open(Arc::new(crypto)).await?);
    let client = Client::builder()
        .homeserver_url(&account.homeserver)
        .store_config(config)
        .build()
        .await
        .map_err(|e| MatrixError::Sdk(e.to_string()))?;
    Ok(Opened {
        client,
        sessions,
        saved,
    })
}

async fn resume(
    account: MatrixAccount,
    client: Client,
    session: MatrixSession,
) -> Result<MatrixProvider> {
    client
        .matrix_auth()
        .restore_session(session, RoomLoadSettings::default())
        .await?;
    MatrixProvider::started(account, client).await
}

impl MatrixProvider {
    /// Log in with a password, unless `store` already holds this account's session: then that is
    /// restored, the password is not used, and the device stays the same. `store` must be encrypted.
    pub async fn login(
        account: MatrixAccount,
        password: &Password,
        store: Arc<TursoStore>,
    ) -> Result<Self> {
        let opened = open(&account, store).await?;
        if let Some(session) = opened.saved {
            return resume(account, opened.client, session).await;
        }
        let client = opened.client;
        client
            .matrix_auth()
            .login_username(&account.user_id, password.expose())
            .initial_device_display_name(&account.device_name)
            .send()
            .await?;
        let session = client
            .matrix_auth()
            .session()
            .ok_or_else(|| MatrixError::Sdk("logged in, and no session".into()))?;
        let text = serde_json::to_string(&session).map_err(crate::StoreFail::from)?;
        let op = Op::Put(SESSION_KEY.to_owned(), text);
        opened.sessions.append(vec![op]).await?;
        Self::started(account, client).await
    }

    /// Come back after a restart with the stored session. `MatrixError::NoSession` if there is none.
    pub async fn restore(account: MatrixAccount, store: Arc<TursoStore>) -> Result<Self> {
        let opened = open(&account, store).await?;
        let session = opened.saved.ok_or(MatrixError::NoSession)?;
        resume(account, opened.client, session).await
    }
}
