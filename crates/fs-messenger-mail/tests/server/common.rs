//! The harness: real servers from `env.sh`, real encrypted stores, the core's `Draft.send`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use fs_messenger_core::{
    AccountSetup, Ingest, Message, Messenger, MessengerConfig, MessengerRepo, SearchFilter,
};
use fs_messenger_mail::fetch::{
    FetchedBatch, FetchedMail, ImapConfig, ImapFetch, MailCursor, MailFetch,
};
use fs_messenger_mail::jmap::{JmapClient, JmapConfig, JmapFetch, JmapSubmit};
use fs_messenger_mail::keys::{FsKeyStore, KeyStore};
use fs_messenger_mail::submit::{MailEnvelope, MailSubmit, SmtpConfig, SmtpSubmit};
use fs_messenger_mail::{Endpoint, MailError, MailIdentity, MailProvider, Password, TlsTrust};
use fs_store::{Cipher, EncryptionKey, ObjectId, OpenOptions, StaticKeyProvider, TursoStore};
use tokio::sync::{Mutex, MutexGuard};

/// One test at a time, on empty mailboxes: they share two real ones, some count what is in them, and
/// a key left in an old mail would be "first seen" for the next test's fresh store.
pub async fn serial() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = LOCK.get_or_init(|| Mutex::new(())).lock().await;
    wipe(Who::Alice).await;
    wipe(Who::Bob).await;
    guard
}

/// Destroy every mail of `who`, through the management port of the throwaway server.
async fn wipe(who: Who) {
    let env = Env::load();
    let url = format!("{}/jmap/", env.get("FS_MSGTEST_JMAP_URL"));
    let name = format!("{}_PASSWORD", who.user_var());
    // Plain HTTP on loopback, test credentials; the client still needs a TLS backend to exist.
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
    let http = reqwest::Client::builder().tls_backend_preconfigured(tls).build().unwrap();
    let mail = "urn:ietf:params:jmap:mail";
    let call = |calls: serde_json::Value| {
        let (http, url, user, pass) = (&http, &url, who.address(), env.get(&name).to_string());
        async move {
            let body = serde_json::json!({"using": ["urn:ietf:params:jmap:core", mail], "methodCalls": calls});
            let answer: serde_json::Value = http.post(url).basic_auth(user, Some(pass)).json(&body)
                .send().await.expect("wipe").json().await.expect("wipe answer");
            answer
        }
    };
    let session: serde_json::Value = http
        .get(format!("{}/jmap/session", env.get("FS_MSGTEST_JMAP_URL")))
        .basic_auth(who.address(), Some(env.get(&name)))
        .send().await.expect("session").json().await.expect("session json");
    let account = session["primaryAccounts"][mail].as_str().expect("account").to_string();
    loop {
        let q = serde_json::json!([["Email/query", {"accountId": account, "limit": 200}, "q"]]);
        let answer = call(q).await;
        let ids = answer["methodResponses"][0][1]["ids"].clone();
        if ids.as_array().is_none_or(Vec::is_empty) {
            return;
        }
        call(serde_json::json!([["Email/set", {"accountId": account, "destroy": ids}, "d"]])).await;
    }
}

/// Variables of `env.sh`, the only place they are defined.
pub struct Env(HashMap<String, String>);

impl Env {
    pub fn load() -> Self {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/test-servers/env.sh");
        let text = std::fs::read_to_string(path).expect("env.sh");
        let mut vars: HashMap<String, String> = HashMap::new();
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else { continue };
            if name.starts_with('#') || name.contains(' ') {
                continue;
            }
            let mut value = value.split(" #").next().unwrap_or("").trim().to_string();
            for (k, v) in &vars {
                value = value.replace(&format!("${k}"), v);
            }
            vars.insert(name.to_string(), value);
        }
        Self(vars)
    }

    pub fn get(&self, name: &str) -> &str {
        self.0.get(name).unwrap_or_else(|| panic!("{name} not in env.sh"))
    }

    fn port(&self, name: &str) -> u16 {
        self.get(name).parse().expect("port")
    }
}

/// The server's own certificate, read off the wire with openssl. The explicit test-only trust.
pub fn server_certificate() -> &'static str {
    static PEM: OnceLock<String> = OnceLock::new();
    PEM.get_or_init(|| {
        let env = Env::load();
        let out = Command::new("openssl")
            .args(["s_client", "-connect"])
            .arg(format!("127.0.0.1:{}", env.get("FS_MSGTEST_IMAP_PORT")))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .expect("openssl");
        let text = String::from_utf8_lossy(&out.stdout);
        let start = text.find("-----BEGIN CERTIFICATE-----").expect("server is up");
        let end = text.find("-----END CERTIFICATE-----").expect("pem end");
        text[start..end + "-----END CERTIFICATE-----".len()].to_string()
    })
}

fn trust() -> TlsTrust {
    TlsTrust::OnlyThisCertificateForTests(server_certificate().to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    Imap,
    Jmap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submit {
    Smtp,
    Jmap,
}

#[derive(Clone, Copy)]
pub enum Who {
    Alice,
    Bob,
}

impl Who {
    pub fn address(self) -> String {
        Env::load().get(self.user_var()).to_string()
    }
    fn user_var(self) -> &'static str {
        match self {
            Who::Alice => "FS_MSGTEST_MAIL_USER1",
            Who::Bob => "FS_MSGTEST_MAIL_USER2",
        }
    }
    fn password(self) -> Password {
        let name = format!("{}_PASSWORD", self.user_var());
        Password::new(Env::load().get(&name))
    }
}

pub fn imap_fetch(who: Who) -> ImapFetch {
    let env = Env::load();
    ImapFetch::new(ImapConfig {
        endpoint: Endpoint {
            host: "localhost".into(),
            port: env.port("FS_MSGTEST_IMAP_PORT"),
            trust: trust(),
        },
        user: who.address(),
        password: who.password(),
    })
}

pub fn smtp_submit(who: Who) -> SmtpSubmit {
    let env = Env::load();
    SmtpSubmit::new(SmtpConfig {
        endpoint: Endpoint {
            host: "localhost".into(),
            port: env.port("FS_MSGTEST_SMTP_PORT"),
            trust: trust(),
        },
        user: who.address(),
        password: who.password(),
    })
}

fn jmap_client(who: Who) -> Arc<JmapClient> {
    let env = Env::load();
    let session_url = format!(
        "https://localhost:{}/jmap/session",
        env.get("FS_MSGTEST_HTTPS_PORT")
    );
    Arc::new(
        JmapClient::new(JmapConfig {
            session_url,
            user: who.address(),
            password: who.password(),
            trust: trust(),
            rebase_on_session_origin: true,
        })
        .expect("jmap client"),
    )
}

/// Every mail in `who`'s mailboxes, as raw bytes, read with a fresh IMAP connection.
pub async fn raw_mails(who: Who) -> Vec<FetchedMail> {
    let fetch = imap_fetch(who);
    let mut cursor = MailCursor::default();
    let mut all = vec![];
    loop {
        let batch = fetch.fetch(&cursor, 50).await.expect("raw fetch");
        all.extend(batch.mails);
        cursor = batch.cursor;
        if !batch.more {
            return all;
        }
    }
}

pub fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// The mail on the server that carries this `Message-ID`, read separately from the provider.
pub async fn raw_by_message_id(who: Who, id: &str) -> Option<Vec<u8>> {
    let header = format!("<{id}>");
    raw_mails(who)
        .await
        .into_iter()
        .find(|m| contains(&m.bytes, &header))
        .map(|m| m.bytes)
}

/// A unique token, so that a test finds its own mail in shared mailboxes.
pub fn token() -> String {
    static N: AtomicUsize = AtomicUsize::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("tok{now}x{}", N.fetch_add(1, Ordering::SeqCst))
}

/// Decorator over a real strategy: counts what the server really delivered.
pub struct Counting {
    inner: Arc<dyn MailFetch>,
    pub delivered: AtomicUsize,
}

#[async_trait]
impl MailFetch for Counting {
    async fn fetch(&self, cursor: &MailCursor, limit: usize) -> Result<FetchedBatch, MailError> {
        let batch = self.inner.fetch(cursor, limit).await?;
        self.delivered.fetch_add(batch.mails.len(), Ordering::SeqCst);
        Ok(batch)
    }
    async fn store_sent(&self, raw: &[u8]) -> Result<(), MailError> {
        self.inner.store_sent(raw).await
    }
}

fn encrypted() -> OpenOptions {
    OpenOptions {
        key_provider: Some(Arc::new(StaticKeyProvider(EncryptionKey {
            cipher: Cipher::Aes256Gcm,
            key: vec![7; 32],
        }))),
        ..OpenOptions::default()
    }
}

/// One person with an encrypted store on disk, the core and a provider on the real server.
pub struct Person {
    pub who: Who,
    pub repo: MessengerRepo,
    pub account: ObjectId,
    pub provider: Arc<MailProvider>,
    pub counting: Arc<Counting>,
    pub dir: PathBuf,
}

impl Person {
    pub async fn open(who: Who, dir: &Path, fetch: Fetch, submit: Submit) -> Self {
        let store = TursoStore::open_with(dir.join("messages.db"), encrypted()).await.unwrap();
        let actor = format!("https://local.invalid/users/{}", who.address());
        let repo = MessengerRepo::open_turso(Arc::new(store), MessengerConfig::new(actor.clone()))
            .await
            .unwrap();
        // ADR 0018: keys in a store of their own, encrypted; never synchronised with the messages.
        let key_store = TursoStore::open_with(dir.join("keys.db"), encrypted()).await.unwrap();
        let keys = Arc::new(FsKeyStore::open_turso(Arc::new(key_store), actor).unwrap());
        let address = who.address();
        let account = match repo.accounts().await.unwrap().pop() {
            Some(a) => a,
            None => repo
                .add_account(AccountSetup {
                    address: address.clone(),
                    kind: "mail".into(),
                    secret: String::new(),
                })
                .await
                .unwrap(),
        };
        let client = jmap_client(who);
        let fetcher: Arc<dyn MailFetch> = match fetch {
            Fetch::Imap => Arc::new(imap_fetch(who)),
            Fetch::Jmap => Arc::new(JmapFetch::new(client.clone())),
        };
        let counting = Arc::new(Counting {
            inner: fetcher,
            delivered: AtomicUsize::new(0),
        });
        let submitter: Arc<dyn MailSubmit> = match submit {
            Submit::Smtp => Arc::new(smtp_submit(who)),
            Submit::Jmap => Arc::new(JmapSubmit::new(client)),
        };
        let provider = Arc::new(MailProvider::new(
            MailIdentity::new(&address),
            counting.clone(),
            submitter,
            keys.clone() as Arc<dyn KeyStore>,
        ));
        provider.ensure_own_key().await.unwrap();
        repo.register_provider(account.id, provider.clone());
        Self {
            who,
            repo,
            account: account.id,
            provider,
            counting,
            dir: dir.to_path_buf(),
        }
    }

    /// `self` learns the other's key by hand, the first contact when nothing leaves unencrypted.
    pub async fn knows(&self, other: &Person) {
        let cert = other.provider.ensure_own_key().await.unwrap().public();
        self.provider
            .import_peer_key(&other.who.address(), &cert)
            .await
            .unwrap();
    }

    /// Send through the core: `Draft.send`.
    pub async fn send(
        &self,
        reply_to: Option<ObjectId>,
        to: &[String],
        bcc: &[String],
        text: &str,
        files: &[(&str, &str, &[u8])],
    ) -> fs_messenger_core::Result<ObjectId> {
        let mut draft = self.repo.compose(reply_to).await?;
        draft.set_account(self.account).await?;
        draft.set_recipients(to.to_vec(), vec![], bcc.to_vec()).await?;
        draft.set_text(text.into(), "text/plain".into()).await?;
        for (name, media, data) in files {
            draft
                .attach((*name).to_string(), (*media).to_string(), data.to_vec())
                .await?;
        }
        draft.send(None).await
    }

    /// Sync until a message with `needle` is here.
    pub async fn wait_for(&self, needle: &str) -> Message {
        for _ in 0..60 {
            self.repo.sync(self.account).await.expect("sync");
            if let Some(m) = self.find(needle).await.pop() {
                return m;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("no message with {needle} within 30 s");
    }

    /// Everything stored with `needle`.
    pub async fn find(&self, needle: &str) -> Vec<Message> {
        let filter = SearchFilter::default();
        let page = self.repo.search(needle, &filter, None).await.expect("search");
        let mut found = vec![];
        for s in page.items {
            let conversation = s.envelope.context.as_deref().unwrap().parse().unwrap();
            found.push(self.repo.message(conversation, s.envelope.id).await.unwrap());
        }
        found
    }

    pub async fn message(&self, id: ObjectId) -> Message {
        let page = self.repo.conversations(None, false, None).await.unwrap();
        for c in page.items {
            if let Ok(m) = self.repo.message(c.id, id).await {
                return m;
            }
        }
        panic!("message {id} not found");
    }
}

/// Submit bytes directly, past the core and past every check: what a stranger's mail looks like.
pub async fn smtp_raw(who: Who, to: Who, raw: &[u8]) {
    let envelope = MailEnvelope {
        from: who.address(),
        recipients: vec![to.address()],
    };
    smtp_submit(who).submit(&envelope, raw).await.expect("smtp");
}
