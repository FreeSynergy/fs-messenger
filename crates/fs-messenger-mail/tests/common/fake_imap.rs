//! A scripted IMAP server over real TLS, for what a real server will not do on request: go silent, or
//! fail on one mailbox. It answers just enough of RFC 9051 for `ImapFetch`, and records every command.

#![allow(dead_code)] // each test file uses a different subset

use std::sync::{Arc, Mutex};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// One mail in `INBOX`, UID 7; a `\Sent` mailbox `Bad` that refuses to open.
pub const MAIL: &[u8] = b"From: a@example.org\r\nMessage-ID: <m@e>\r\n\r\nhello\r\n";

pub struct FakeImap {
    pub port: u16,
    pub certificate: String,
    pub commands: Arc<Mutex<Vec<String>>>,
}

fn tls() -> (TlsAcceptor, String) {
    let made = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let certificate = made.cert.pem();
    let der = CertificateDer::from(made.cert.der().to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(made.signing_key.serialize_der()));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![der], key)
        .unwrap();
    (TlsAcceptor::from(Arc::new(config)), certificate)
}

fn answer(line: &str) -> String {
    let (tag, command) = line.split_once(' ').unwrap_or((line, ""));
    let upper = command.to_uppercase();
    if upper.starts_with("LOGIN") {
        format!("{tag} OK done\r\n")
    } else if upper.starts_with("LIST") {
        format!("* LIST (\\Sent) \"/\" \"Bad\"\r\n{tag} OK done\r\n")
    } else if upper.starts_with("EXAMINE \"BAD\"") {
        format!("{tag} NO cannot open\r\n")
    } else if upper.starts_with("EXAMINE") {
        format!("* 1 EXISTS\r\n* OK [UIDVALIDITY 1] ok\r\n{tag} OK [READ-ONLY] done\r\n")
    } else if upper.starts_with("UID SEARCH") {
        format!("* SEARCH 7\r\n{tag} OK done\r\n")
    } else if upper.contains("RFC822.SIZE") {
        format!(
            "* 1 FETCH (UID 7 RFC822.SIZE {})\r\n{tag} OK done\r\n",
            MAIL.len()
        )
    } else if upper.starts_with("UID FETCH") {
        let body = String::from_utf8_lossy(MAIL);
        format!(
            "* 1 FETCH (UID 7 BODY[] {{{}}}\r\n{body})\r\n{tag} OK done\r\n",
            MAIL.len()
        )
    } else if upper.starts_with("LOGOUT") {
        format!("* BYE bye\r\n{tag} OK done\r\n")
    } else {
        format!("{tag} BAD unknown\r\n")
    }
}

/// `silent`: accepts TCP and says nothing, not even the TLS handshake.
pub async fn start(silent: bool) -> FakeImap {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (acceptor, certificate) = tls();
    let commands = Arc::new(Mutex::new(vec![]));
    let log = commands.clone();
    tokio::spawn(async move {
        let mut held = vec![];
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            if silent {
                held.push(tcp);
                continue;
            }
            let (acceptor, log) = (acceptor.clone(), log.clone());
            tokio::spawn(async move {
                let Ok(stream) = acceptor.accept(tcp).await else {
                    return;
                };
                let (read, mut write) = tokio::io::split(stream);
                let mut lines = BufReader::new(read).lines();
                write.write_all(b"* OK ready\r\n").await.unwrap();
                while let Ok(Some(line)) = lines.next_line().await {
                    log.lock().unwrap().push(line.clone());
                    write.write_all(answer(&line).as_bytes()).await.unwrap();
                }
            });
        }
    });
    FakeImap {
        port,
        certificate,
        commands,
    }
}
