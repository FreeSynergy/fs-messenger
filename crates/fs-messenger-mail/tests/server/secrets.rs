//! On the real server: after real mail, no secret key and no password in any log or file; and a refused
//! login names no password, over any of the three protocols.

use fs_messenger_mail::fetch::{
    DEFAULT_IMAP_TIMEOUT, FetchLimits, ImapConfig, ImapFetch, MailCursor, MailFetch,
};
use fs_messenger_mail::jmap::{JmapClient, JmapConfig, JmapFetch};
use fs_messenger_mail::keys::KeyStore as _;
use fs_messenger_mail::submit::{MailEnvelope, MailSubmit, SmtpConfig, SmtpSubmit};
use fs_messenger_mail::{Endpoint, Password, TlsTrust};
use fs_store::{ActivityFilter, Store, TursoStore};
use std::sync::Arc;

use crate::common::{Fetch, Person, Submit, Who, serial, token};
use crate::env::{Env, server_certificate};
use crate::forge::{files_of, holds_any, key, secret_needles};

async fn log_of(store: &TursoStore) -> Vec<u8> {
    let log = Store::activities(store, &ActivityFilter::default())
        .await
        .unwrap();
    let mut all = format!("{log:?}").into_bytes();
    all.extend(serde_json::to_vec(&log).unwrap());
    all
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn after_real_mail_no_secret_is_in_any_log_or_file() {
    let _one = serial().await;
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = Person::open(Who::Alice, a.path(), Fetch::Imap, Submit::Smtp).await;
    let bob = Person::open(Who::Bob, b.path(), Fetch::Jmap, Submit::Jmap).await;
    let (secret, rpgp) = key(&Who::Alice.address());
    alice
        .keys
        .put_own_key(&Who::Alice.address(), &secret)
        .await
        .unwrap();
    alice.knows(&bob).await;
    bob.knows(&alice).await;
    let t = token();
    let text = format!("Secretless-{t}");
    alice
        .send(None, &[Who::Bob.address()], &[], &text, &[])
        .await
        .expect("send");
    let got = bob.wait_for(&text).await;
    bob.send(
        Some(got.id()),
        &[Who::Alice.address()],
        &[],
        &format!("Back-{t}"),
        &[],
    )
    .await
    .expect("reply");
    alice.wait_for(&format!("Back-{t}")).await;

    let env = Env::load();
    let mut needles = secret_needles(&rpgp);
    for name in [
        "FS_MSGTEST_MAIL_USER1_PASSWORD",
        "FS_MSGTEST_MAIL_USER2_PASSWORD",
    ] {
        needles.push(env.get(name).as_bytes().to_vec());
    }
    for person in [&alice, &bob] {
        assert!(
            !holds_any(&log_of(&person.message_store).await, &needles),
            "the message log"
        );
        assert!(
            !holds_any(&log_of(&person.key_store).await, &needles),
            "the key log"
        );
    }
    for dir in [a.path(), b.path()] {
        assert!(!holds_any(&files_of(dir), &needles), "the raw files");
    }
}

fn wrong() -> Password {
    Password::new("needle-wrong-password-4711")
}

fn endpoint(port: &str) -> Endpoint {
    Endpoint {
        host: "localhost".into(),
        port: Env::load().get(port).parse().unwrap(),
        trust: TlsTrust::OnlyThisCertificateForTests(server_certificate().to_string()),
    }
}

fn assert_clean(error: &fs_messenger_mail::MailError) {
    let shown = format!("{error} {error:?}");
    println!("refused: {shown}");
    assert!(!shown.contains("needle-wrong-password"), "{shown}");
    let core: fs_messenger_core::Error =
        fs_messenger_mail::MailError::Imap(error.to_string()).into();
    assert!(!format!("{core} {core:?}").contains("needle-wrong-password"));
}

#[tokio::test]
#[ignore = "needs scripts/test-servers/up.sh; run with --ignored"]
async fn a_refused_login_names_no_password() {
    let _one = serial().await;
    let user = Who::Bob.address();
    let limits = FetchLimits {
        mails: 1,
        bytes: 1024,
    };
    let imap = ImapFetch::new(ImapConfig {
        endpoint: endpoint("FS_MSGTEST_IMAP_PORT"),
        user: user.clone(),
        password: wrong(),
        timeout: DEFAULT_IMAP_TIMEOUT,
    });
    assert_clean(
        &imap
            .fetch(&MailCursor::default(), &limits)
            .await
            .err()
            .expect("refused"),
    );
    let smtp = SmtpSubmit::new(SmtpConfig {
        endpoint: endpoint("FS_MSGTEST_SMTP_PORT"),
        user: user.clone(),
        password: wrong(),
    });
    let envelope = MailEnvelope {
        from: user.clone(),
        recipients: vec![user.clone()],
    };
    assert_clean(&smtp.submit(&envelope, b"x").await.err().expect("refused"));
    let port = Env::load().get("FS_MSGTEST_HTTPS_PORT").to_string();
    let client = JmapClient::new(JmapConfig {
        session_url: format!("https://localhost:{port}/jmap/session"),
        user,
        password: wrong(),
        trust: TlsTrust::OnlyThisCertificateForTests(server_certificate().to_string()),
        rebase_on_session_origin: true,
        other_credential_origins: vec![],
    })
    .unwrap();
    let jmap = JmapFetch::new(Arc::new(client));
    assert_clean(
        &jmap
            .fetch(&MailCursor::default(), &limits)
            .await
            .err()
            .expect("refused"),
    );
}
