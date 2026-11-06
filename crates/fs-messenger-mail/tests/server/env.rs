//! The servers from `env.sh`, the certificate, and who is who.

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};

use fs_messenger_mail::Password;
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

/// Destroy one mail of `who`, as another program of theirs would.
pub async fn destroy(who: Who, email: &str) {
    let answer = jmap_call(who, |account| {
        serde_json::json!([["Email/set", {"accountId": account, "destroy": [email]}, "d"]])
    })
    .await;
    assert!(
        answer["methodResponses"][0][1]["destroyed"][0] == email,
        "{answer}"
    );
}

fn loopback_client() -> reqwest::Client {
    // Plain HTTP on loopback, test credentials; the client still needs a TLS backend to exist.
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(rustls::RootCertStore::empty())
    .with_no_client_auth();
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .build()
        .unwrap()
}

/// One JMAP request by `who`, through the management port of the throwaway server.
async fn jmap_call(who: Who, calls: impl FnOnce(&str) -> serde_json::Value) -> serde_json::Value {
    let env = Env::load();
    let name = format!("{}_PASSWORD", who.user_var());
    let http = loopback_client();
    let base = env.get("FS_MSGTEST_JMAP_URL");
    let session: serde_json::Value = http
        .get(format!("{base}/jmap/session"))
        .basic_auth(who.address(), Some(env.get(&name)))
        .send()
        .await
        .expect("session")
        .json()
        .await
        .expect("session json");
    let mail = "urn:ietf:params:jmap:mail";
    let account = session["primaryAccounts"][mail].as_str().expect("account");
    let body = serde_json::json!({"using": ["urn:ietf:params:jmap:core", mail], "methodCalls": calls(account)});
    http.post(format!("{base}/jmap/"))
        .basic_auth(who.address(), Some(env.get(&name)))
        .json(&body)
        .send()
        .await
        .expect("jmap")
        .json()
        .await
        .expect("jmap json")
}

/// Destroy every mail of `who`, through the management port of the throwaway server.
async fn wipe(who: Who) {
    loop {
        let answer = jmap_call(who, |account| {
            serde_json::json!([["Email/query", {"accountId": account, "limit": 200}, "q"]])
        })
        .await;
        let ids = answer["methodResponses"][0][1]["ids"].clone();
        if ids.as_array().is_none_or(Vec::is_empty) {
            return;
        }
        jmap_call(who, |account| {
            serde_json::json!([["Email/set", {"accountId": account, "destroy": ids}, "d"]])
        })
        .await;
    }
}

/// Variables of `env.sh`, the only place they are defined.
pub struct Env(HashMap<String, String>);

impl Env {
    pub fn load() -> Self {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/test-servers/env.sh"
        );
        let text = std::fs::read_to_string(path).expect("env.sh");
        let mut vars: HashMap<String, String> = HashMap::new();
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
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
        self.0
            .get(name)
            .unwrap_or_else(|| panic!("{name} not in env.sh"))
    }

    pub fn port(&self, name: &str) -> u16 {
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
        let start = text
            .find("-----BEGIN CERTIFICATE-----")
            .expect("server is up");
        let end = text.find("-----END CERTIFICATE-----").expect("pem end");
        text[start..end + "-----END CERTIFICATE-----".len()].to_string()
    })
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
    pub fn password(self) -> Password {
        let name = format!("{}_PASSWORD", self.user_var());
        Password::new(Env::load().get(&name))
    }
}
