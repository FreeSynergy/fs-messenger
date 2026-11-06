//! A scripted HTTP server. **It stands in for the wire only**: it plays bytes written from documentation
//! and records what reached it. It knows nothing of models, and no test may take what it says as proof
//! that a real server behaves like this. Hand-written HTTP/1.1, one request per connection, because a
//! hostile server must be able to write what no HTTP library would. Loopback only.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use super::events::{anthropic, llama};

use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

/// One thing the server does.
#[derive(Clone, Debug)]
pub enum Step {
    /// These bytes, at once.
    Send(Vec<u8>),
    /// These bytes one at a time, flushed each.
    Dribble(Vec<u8>),
    /// Stand still.
    Wait(Duration),
    /// Stand still until the other side has gone.
    Hang,
    /// These bytes again and again, until a write fails.
    Flood { piece: Vec<u8>, every: Duration },
}

pub type Reply = Vec<Step>;

/// A request as it reached the server.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    pub line: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Seen {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("the request body is JSON")
    }
}

#[derive(Default)]
pub struct Counters {
    pub connections: AtomicUsize,
    pub written: AtomicU64,
    pub write_failed: AtomicBool,
}

pub struct Scripted {
    pub origin: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    pub counters: Arc<Counters>,
    /// PEM of its certificate, over TLS.
    pub certificate: String,
}

impl Scripted {
    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
    /// Until this many requests have reached it.
    pub async fn until_requests(&self, count: usize) {
        for _ in 0..3000 {
            if self.seen.lock().unwrap().len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("{count} requests never came");
    }
    pub fn written(&self) -> u64 {
        self.counters.written.load(Ordering::SeqCst)
    }
    pub fn write_failed(&self) -> bool {
        self.counters.write_failed.load(Ordering::SeqCst)
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

async fn read_request<S: AsyncRead + Unpin>(stream: &mut S) -> Option<Seen> {
    let mut bytes = vec![];
    let mut buffer = [0u8; 4096];
    let head = loop {
        if let Some(at) = find(&bytes, b"\r\n\r\n") {
            break at;
        }
        let n = stream.read(&mut buffer).await.ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..n]);
    };
    let text = String::from_utf8_lossy(&bytes[..head]).to_string();
    let mut lines = text.split("\r\n");
    let line = lines.next()?.to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    while bytes.len() < head + 4 + length {
        let n = stream.read(&mut buffer).await.ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    let body = String::from_utf8_lossy(&bytes[head + 4..head + 4 + length]).to_string();
    Some(Seen {
        line,
        headers,
        body,
    })
}

async fn play<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    reply: Reply,
    counters: &Counters,
) -> std::io::Result<()> {
    let wrote = |n: usize| {
        counters.written.fetch_add(n as u64, Ordering::SeqCst);
    };
    for step in reply {
        match step {
            Step::Send(bytes) => {
                stream.write_all(&bytes).await?;
                stream.flush().await?;
                wrote(bytes.len());
            }
            Step::Dribble(bytes) => {
                for byte in bytes {
                    stream.write_all(&[byte]).await?;
                    stream.flush().await?;
                    wrote(1);
                    tokio::task::yield_now().await;
                }
            }
            Step::Wait(time) => tokio::time::sleep(time).await,
            Step::Hang => {
                let mut sink = [0u8; 64];
                while stream.read(&mut sink).await? > 0 {}
            }
            Step::Flood { piece, every } => loop {
                stream.write_all(&piece).await?;
                stream.flush().await?;
                wrote(piece.len());
                tokio::time::sleep(every).await;
            },
        }
    }
    Ok(())
}

async fn handle<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    reply: Reply,
    seen: &Mutex<Vec<Seen>>,
    counters: &Counters,
) {
    let Some(request) = read_request(&mut stream).await else {
        return;
    };
    seen.lock().unwrap().push(request);
    if play(&mut stream, reply, counters).await.is_err() {
        counters.write_failed.store(true, Ordering::SeqCst);
    }
    let _ = stream.shutdown().await;
}

async fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

/// Over plain HTTP on loopback. The nth connection gets the nth reply; more are closed unanswered.
pub async fn serve(replies: Vec<Reply>) -> Scripted {
    let (listener, port) = listener().await;
    let seen = Arc::new(Mutex::new(vec![]));
    let counters = Arc::new(Counters::default());
    let (held, count) = (seen.clone(), counters.clone());
    tokio::spawn(async move {
        let mut replies = replies.into_iter();
        while let Ok((stream, _)) = listener.accept().await {
            count.connections.fetch_add(1, Ordering::SeqCst);
            let reply = replies.next().unwrap_or_default();
            let (held, count) = (held.clone(), count.clone());
            tokio::spawn(async move { handle(stream, reply, &held, &count).await });
        }
    });
    Scripted {
        origin: format!("http://127.0.0.1:{port}"),
        seen,
        counters,
        certificate: String::new(),
    }
}

/// Over real TLS, with a certificate of its own for `localhost` that no system trusts.
pub async fn serve_tls(replies: Vec<Reply>) -> Scripted {
    let names = vec!["localhost".to_string()];
    let made = rcgen::generate_simple_self_signed(names).unwrap();
    let certificate = made.cert.pem();
    let chain = vec![CertificateDer::from_pem_slice(certificate.as_bytes()).unwrap()];
    let key = PrivateKeyDer::from_pem_slice(made.signing_key.serialize_pem().as_bytes()).unwrap();
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let (listener, port) = listener().await;
    let seen = Arc::new(Mutex::new(vec![]));
    let counters = Arc::new(Counters::default());
    let (held, count) = (seen.clone(), counters.clone());
    tokio::spawn(async move {
        let mut replies = replies.into_iter();
        while let Ok((stream, _)) = listener.accept().await {
            count.connections.fetch_add(1, Ordering::SeqCst);
            let reply = replies.next().unwrap_or_default();
            let (held, count, acceptor) = (held.clone(), count.clone(), acceptor.clone());
            tokio::spawn(async move {
                if let Ok(stream) = acceptor.accept(stream).await {
                    handle(stream, reply, &held, &count).await;
                }
            });
        }
    });
    Scripted {
        origin: format!("https://localhost:{port}"),
        seen,
        counters,
        certificate,
    }
}

/// The head of a 200 event stream that ends when the connection does.
pub fn sse_head() -> Vec<u8> {
    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n".to_vec()
}

/// A status with headers and a body.
pub fn answer(status: &str, headers: &[(&str, &str)], body: &str) -> Reply {
    let mut text = format!("HTTP/1.1 {status}\r\nconnection: close\r\n");
    for (name, value) in headers {
        text.push_str(&format!("{name}: {value}\r\n"));
    }
    text.push_str(&format!("content-length: {}\r\n\r\n{body}", body.len()));
    vec![Step::Send(text.into_bytes())]
}

/// A whole event stream, at once.
pub fn stream(body: impl Into<Vec<u8>>) -> Reply {
    let mut bytes = sse_head();
    bytes.extend(body.into());
    vec![Step::Send(bytes)]
}
