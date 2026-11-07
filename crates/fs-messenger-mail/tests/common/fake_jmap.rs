//! A scripted JMAP server over real TLS (HTTP/1.1, hand-written), for what a real server will not do on
//! request: stall in a download, report a size it does not send, or fail on the sent mailbox.

#![allow(dead_code)] // each test file uses a different subset

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{
    AsyncBufReadExt as _, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, BufReader,
};
use tokio::net::TcpListener;

use super::fake_imap::{tls, write_body};

/// One mail in the inbox.
#[derive(Clone)]
pub struct Stored {
    pub id: String,
    /// What the `size` property says.
    pub reported: u64,
    pub bytes: Vec<u8>,
}

/// What the server does.
#[derive(Clone, Default)]
pub struct Script {
    pub mails: Vec<Stored>,
    /// What `Email/changes` answers as created.
    pub created: Vec<String>,
    pub sent_fails: bool,
    pub drip: Option<Duration>,
    pub stall_in_body: bool,
}

pub struct FakeJmap {
    pub port: u16,
    pub certificate: String,
    /// Ids of the downloads asked for.
    pub downloads: Arc<Mutex<Vec<String>>>,
}

fn method(call: &Value, script: &Script) -> Value {
    let (name, args, id) = (call[0].as_str().unwrap_or(""), &call[1], &call[2]);
    let answer = |body: Value| json!([name, body, id]);
    match name {
        "Mailbox/get" => answer(json!({"list": [
            {"id": "i", "role": "inbox"}, {"id": "s", "role": "sent"}]})),
        "Email/changes" => answer(json!({"newState": "s2", "created": script.created,
            "updated": [], "hasMoreChanges": false})),
        "Email/query" if args["filter"]["inMailbox"] == "s" => {
            if script.sent_fails {
                json!(["error", {"type": "serverFail"}, id])
            } else {
                answer(json!({"ids": []}))
            }
        }
        "Email/query" => {
            let ids: Vec<&String> = script.mails.iter().map(|m| &m.id).collect();
            let start = args["anchor"]
                .as_str()
                .and_then(|a| ids.iter().position(|i| *i == a))
                .map_or(0, |at| at + 1);
            let limit = usize::try_from(args["limit"].as_u64().unwrap_or(50)).unwrap();
            let page: Vec<&&String> = ids.iter().skip(start).take(limit).collect();
            answer(json!({"ids": page}))
        }
        "Email/get" => {
            let asked = args["ids"].as_array().cloned().unwrap_or_default();
            let list: Vec<Value> = script
                .mails
                .iter()
                .filter(|m| asked.iter().any(|a| a == m.id.as_str()))
                .map(|m| {
                    json!({"id": m.id, "blobId": m.id, "size": m.reported,
                    "mailboxIds": {"i": true}})
                })
                .collect();
            answer(json!({"state": "s1", "list": list}))
        }
        _ => json!(["error", {"type": "unknownMethod"}, id]),
    }
}

async fn respond<W: AsyncWrite + Unpin>(out: &mut W, body: &[u8], script: Option<&Script>) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    out.write_all(head.as_bytes()).await.unwrap();
    out.flush().await.unwrap();
    match script {
        Some(s) => write_body(out, body, s.drip, s.stall_in_body).await,
        None => out.write_all(body).await.unwrap(),
    }
    out.flush().await.unwrap();
    let _ = out.shutdown().await;
}

pub async fn start(script: Script) -> FakeJmap {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (acceptor, certificate) = tls();
    let downloads = Arc::new(Mutex::new(vec![]));
    let log = downloads.clone();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let (acceptor, script, log) = (acceptor.clone(), script.clone(), log.clone());
            tokio::spawn(async move {
                let Ok(stream) = acceptor.accept(tcp).await else {
                    return;
                };
                let (read, mut write) = tokio::io::split(stream);
                let mut read = BufReader::new(read);
                let mut first = String::new();
                read.read_line(&mut first).await.unwrap();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    read.read_line(&mut line).await.unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                read.read_exact(&mut body).await.unwrap();
                let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
                if path.ends_with("/session") {
                    let origin = format!("https://localhost:{port}");
                    let doc = json!({"apiUrl": format!("{origin}/api"),
                        "uploadUrl": format!("{origin}/up/{{accountId}}"),
                        "downloadUrl": format!("{origin}/down/{{blobId}}"),
                        "primaryAccounts": {"urn:ietf:params:jmap:mail": "a"}});
                    respond(&mut write, doc.to_string().as_bytes(), None).await;
                } else if let Some(id) = path.strip_prefix("/down/") {
                    log.lock().unwrap().push(id.to_string());
                    let mail = script.mails.iter().find(|m| m.id == id).unwrap();
                    respond(&mut write, &mail.bytes, Some(&script)).await;
                } else {
                    let request: Value = serde_json::from_slice(&body).unwrap();
                    let calls = request["methodCalls"].as_array().unwrap();
                    let answers: Vec<Value> = calls.iter().map(|c| method(c, &script)).collect();
                    let doc = json!({"methodResponses": answers});
                    respond(&mut write, doc.to_string().as_bytes(), None).await;
                }
            });
        }
    });
    FakeJmap {
        port,
        certificate,
        downloads,
    }
}
