//! A homeserver that behaves badly on purpose: raw HTTP on a loopback port, one answer per request.
//! Not a double of Matrix: it stands for an attacker's server, and only its bad answers are the point.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One answer.
pub struct Reply(String);

impl Reply {
    pub fn json(status: u16, body: &serde_json::Value) -> Self {
        let body = body.to_string();
        Self(format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ))
    }

    /// Hear the request and never answer.
    pub fn never() -> Self {
        Self(String::new())
    }

    pub fn redirect(to: &str) -> Self {
        Self(format!(
            "HTTP/1.1 307 X\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ))
    }
}

type Heard = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// A running server, and all it heard: path and `Authorization` header.
pub struct Server {
    port: u16,
    heard: Heard,
}

impl Server {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn heard(&self) -> Vec<(String, Option<String>)> {
        self.heard.lock().unwrap().clone()
    }

    pub async fn start(answer: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let heard: Heard = Arc::default();
        let (log, answer) = (Arc::clone(&heard), Arc::new(answer));
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (log, answer) = (Arc::clone(&log), Arc::clone(&answer));
                tokio::spawn(async move {
                    let mut buffer = vec![0; 64 * 1024];
                    let n = socket.read(&mut buffer).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..n]).into_owned();
                    let path = request
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_owned();
                    let auth = request
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
                        .map(ToOwned::to_owned);
                    log.lock().unwrap().push((path.clone(), auth));
                    let reply = answer(&path).0;
                    if reply.is_empty() {
                        std::future::pending::<()>().await;
                    }
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        Self { port, heard }
    }

    /// Takes the connection and never says a word.
    pub async fn silent() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        Self {
            port,
            heard: Arc::default(),
        }
    }
}
