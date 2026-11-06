//! A scripted IMAP server over real TLS, for what a real server will not do on request: go silent, stall
//! in the middle of a mail, report a size it does not send, or fail on one mailbox. It answers just enough
//! of RFC 9051 for `ImapFetch`, and records every command.

#![allow(dead_code)] // each test file uses a different subset

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// A plain small mail.
pub const MAIL: &[u8] = b"From: a@example.org\r\nMessage-ID: <m@e>\r\n\r\nhello\r\n";

/// One mail in `INBOX`.
#[derive(Clone)]
pub struct Stored {
    pub uid: u32,
    /// What `RFC822.SIZE` says.
    pub reported: u64,
    pub bytes: Vec<u8>,
}

/// What the server does.
#[derive(Clone, Default)]
pub struct Script {
    pub mails: Vec<Stored>,
    /// Accepts TCP and says nothing, not even the TLS handshake.
    pub silent: bool,
    /// Stops in the middle of the first body and stays connected.
    pub stall_in_body: bool,
    /// Sends a body in pieces with this pause between them.
    pub drip: Option<Duration>,
}

impl Script {
    /// `INBOX` holds the mail `MAIL` under UID 7.
    pub fn one_mail() -> Self {
        Self {
            mails: vec![Stored {
                uid: 7,
                reported: MAIL.len() as u64,
                bytes: MAIL.to_vec(),
            }],
            ..Self::default()
        }
    }
}

pub struct FakeImap {
    pub port: u16,
    pub certificate: String,
    pub commands: Arc<Mutex<Vec<String>>>,
}

pub fn tls() -> (TlsAcceptor, String) {
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

/// Writes `bytes`, as the script says.
pub async fn write_body<W: AsyncWrite + Unpin>(
    out: &mut W,
    bytes: &[u8],
    script_drip: Option<Duration>,
    stall: bool,
) {
    if stall {
        out.write_all(&bytes[..bytes.len() / 2]).await.unwrap();
        out.flush().await.unwrap();
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
    match script_drip {
        Some(pause) => {
            for piece in bytes.chunks(bytes.len().div_ceil(5).max(1)) {
                out.write_all(piece).await.unwrap();
                out.flush().await.unwrap();
                tokio::time::sleep(pause).await;
            }
        }
        None => out.write_all(bytes).await.unwrap(),
    }
}

fn wanted(command: &str, script: &Script) -> Vec<Stored> {
    let set = command.split_whitespace().nth(2).unwrap_or("");
    let uids: Vec<u32> = set.split(',').filter_map(|u| u.parse().ok()).collect();
    let mails = script.mails.iter();
    mails.filter(|m| uids.contains(&m.uid)).cloned().collect()
}

async fn answer<W: AsyncWrite + Unpin>(out: &mut W, line: &str, script: &Script) {
    let (tag, command) = line.split_once(' ').unwrap_or((line, ""));
    let upper = command.to_uppercase();
    let done = format!("{tag} OK done\r\n");
    let text = if upper.starts_with("LOGIN") {
        done
    } else if upper.starts_with("LIST") {
        format!("* LIST (\\Sent) \"/\" \"Bad\"\r\n{done}")
    } else if upper.starts_with("EXAMINE \"BAD\"") {
        format!("{tag} NO cannot open\r\n")
    } else if upper.starts_with("EXAMINE") {
        let count = script.mails.len();
        format!("* {count} EXISTS\r\n* OK [UIDVALIDITY 1] ok\r\n{tag} OK [READ-ONLY] done\r\n")
    } else if upper.starts_with("UID SEARCH") {
        let from: u32 = upper
            .rsplit("UID ")
            .next()
            .and_then(|r| r.split(':').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let uids: Vec<String> = script
            .mails
            .iter()
            .filter(|m| m.uid >= from)
            .map(|m| m.uid.to_string())
            .collect();
        format!("* SEARCH {}\r\n{done}", uids.join(" "))
    } else if upper.contains("RFC822.SIZE") {
        let lines: Vec<String> = wanted(command, script)
            .iter()
            .enumerate()
            .map(|(i, m)| {
                format!(
                    "* {} FETCH (UID {} RFC822.SIZE {})\r\n",
                    i + 1,
                    m.uid,
                    m.reported
                )
            })
            .collect();
        format!("{}{done}", lines.concat())
    } else if upper.contains("BODY.PEEK[]") {
        let head = upper
            .split("<0.")
            .nth(1)
            .and_then(|r| r.split('>').next())
            .and_then(|n| n.parse::<usize>().ok());
        for (i, m) in wanted(command, script).iter().enumerate() {
            let bytes = &m.bytes[..head.map_or(m.bytes.len(), |h| h.min(m.bytes.len()))];
            let section = if head.is_some() {
                "BODY[]<0>"
            } else {
                "BODY[]"
            };
            let intro = format!(
                "* {} FETCH (UID {} {section} {{{}}}\r\n",
                i + 1,
                m.uid,
                bytes.len()
            );
            out.write_all(intro.as_bytes()).await.unwrap();
            write_body(out, bytes, script.drip, script.stall_in_body).await;
            out.write_all(b")\r\n").await.unwrap();
        }
        done
    } else if upper.starts_with("LOGOUT") {
        format!("* BYE bye\r\n{done}")
    } else {
        format!("{tag} BAD unknown\r\n")
    };
    out.write_all(text.as_bytes()).await.unwrap();
    out.flush().await.unwrap();
}

pub async fn start(script: Script) -> FakeImap {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (acceptor, certificate) = tls();
    let commands = Arc::new(Mutex::new(vec![]));
    let log = commands.clone();
    tokio::spawn(async move {
        let mut held = vec![];
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            if script.silent {
                held.push(tcp);
                continue;
            }
            let (acceptor, log, script) = (acceptor.clone(), log.clone(), script.clone());
            tokio::spawn(async move {
                let Ok(stream) = acceptor.accept(tcp).await else {
                    return;
                };
                let (read, mut write) = tokio::io::split(stream);
                let mut lines = BufReader::new(read).lines();
                write.write_all(b"* OK ready\r\n").await.unwrap();
                write.flush().await.unwrap();
                while let Ok(Some(line)) = lines.next_line().await {
                    log.lock().unwrap().push(line.clone());
                    answer(&mut write, &line, &script).await;
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
