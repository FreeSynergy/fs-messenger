//! Logging in, and coming back after a restart.
//!
//! Pattern: Adapter, part of MatrixProvider (the type is spread over the files of this folder): building the SDK client over our stores, `login` and `restore`. It
//! builds the SDK client over the stores of `fs-store`, a concrete type and no trait object, so not a
//! Factory. The session lives in the same encrypted store as the keys. A stored session always wins
//! over a password: a new login would be a new device, with new keys, and the old messages would stay
//! unreadable.
//!
//! The session is stored with the origin of the homeserver that issued it, and a restore against
//! another origin (or a session stored without one) fails before the first request: the token goes
//! only to the server that made it. It also only ever goes to the configured origin: a homeserver cannot move it, neither with the
//! `well_known` of a login answer, which the SDK follows by default, nor with a redirect, which
//! `reqwest` follows by default. Requests have a time limit and a retry limit: the SDK's default is to
//! retry a failing server for ever. `MatrixAccount::bounded` is the hard end on top of that.

use std::sync::Arc;
use std::time::Duration;

use fs_store::TursoStore;
use matrix_sdk::Client;
use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::config::{RequestConfig, StoreConfig};
use matrix_sdk::cross_process_lock::CrossProcessLockConfig;
use matrix_sdk::reqwest;
use matrix_sdk::store::RoomLoadSettings;

use super::{MatrixAccount, MatrixProvider};
use crate::error::{MatrixError, Result};
use crate::secret::Password;
use crate::store::{FsCryptoStore, FsJournal, FsStateStore, Journal, Op};

const SESSION_KEY: &str = "session";
/// The origin of the homeserver that issued the session, stored with it.
const ORIGIN_KEY: &str = "homeserver";

/// reqwest is built without a crypto provider, so it needs a process-wide one. Installing twice is
/// not an error: whoever came first stays.
fn install_tls() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// How many times a failed request is tried again, and how long that may go on in all.
const RETRIES: usize = 3;

fn http_client(timeout: Duration) -> Result<reqwest::Client> {
    let client = reqwest::Client::builder()
        .user_agent("FreeSynergy.Messenger")
        .redirect(reqwest::redirect::Policy::none())
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .connect_timeout(timeout)
        .read_timeout(timeout)
        .build();
    client.map_err(|e| MatrixError::Sdk(e.to_string()))
}

const OTHER_SERVER: &str = "the stored session was issued by another homeserver; its token is not \
sent here. Configure that server, or use a new store and log in again";
const NO_ORIGIN: &str = "the stored session has no homeserver on record (an older draft); use a new \
store and log in again";

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
    let stored = sessions.load().await?;
    let saved = match stored.get(SESSION_KEY) {
        Some(text) => {
            let origin = account.origin()?;
            match stored.get(ORIGIN_KEY) {
                Some(issuer) if issuer.matches(&origin) => {}
                Some(_) => return Err(MatrixError::Config(OTHER_SERVER.into())),
                None => return Err(MatrixError::Config(NO_ORIGIN.into())),
            }
            Some(serde_json::from_str(text.expose()).map_err(crate::StoreFail::from)?)
        }
        None => None,
    };
    let config = StoreConfig::new(CrossProcessLockConfig::SingleProcess)
        .state_store(FsStateStore::open(Arc::new(state)).await?)
        .crypto_store(FsCryptoStore::open(Arc::new(crypto)).await?);
    let timeout = account.request_timeout;
    let requests = RequestConfig::new()
        .timeout(timeout)
        .retry_limit(RETRIES)
        .max_retry_time(timeout * 4);
    let client = Client::builder()
        .homeserver_url(&account.homeserver)
        .http_client(http_client(timeout)?)
        .request_config(requests)
        .respect_login_well_known(false)
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
        let auth = client.matrix_auth();
        let login = auth
            .login_username(&account.user_id, password.expose())
            .initial_device_display_name(&account.device_name)
            .send();
        account.bounded(login).await?;
        let session = client
            .matrix_auth()
            .session()
            .ok_or_else(|| MatrixError::Sdk("logged in, and no session".into()))?;
        let text = serde_json::to_string(&session).map_err(crate::StoreFail::from)?;
        let origin = account.origin()?;
        let ops = vec![
            Op::Put(SESSION_KEY.to_owned(), text.into()),
            Op::Put(ORIGIN_KEY.to_owned(), origin.into()),
        ];
        opened.sessions.append(ops).await?;
        Self::started(account, client).await
    }

    /// Come back after a restart with the stored session. `MatrixError::NoSession` if there is none.
    pub async fn restore(account: MatrixAccount, store: Arc<TursoStore>) -> Result<Self> {
        let opened = open(&account, store).await?;
        let session = opened.saved.ok_or(MatrixError::NoSession)?;
        resume(account, opened.client, session).await
    }
}
