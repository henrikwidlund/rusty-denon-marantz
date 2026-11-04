//! A fake receiver: a small HTTP server that answers with what a test says.

#![allow(dead_code)] // Each test file uses a part of this.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A request that the fake receiver got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) body: Vec<u8>,
}

/// What the fake receiver does with a request.
pub(crate) enum Reply {
    /// HTTP 200 with this body.
    Body(Vec<u8>),
    /// This status, without a body.
    Status(u16),
    /// Never answer.
    Hang,
    /// Close the connection without an answer.
    Close,
    /// Send these bytes, then close the connection.
    Raw(Vec<u8>),
}

pub(crate) struct Fake {
    pub(crate) addr: SocketAddr,
    pub(crate) port: u16,
    pub(crate) seen: Arc<Mutex<Vec<Seen>>>,
    pub(crate) connections: Arc<AtomicUsize>,
}

impl Fake {
    pub(crate) async fn start(handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(handler);
        let (seen_for_task, connections_for_task) = (seen.clone(), connections.clone());
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                connections_for_task.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serve(socket, handler.clone(), seen_for_task.clone()));
            }
        });
        Self {
            addr,
            port: addr.port(),
            seen,
            connections,
        }
    }

    pub(crate) fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    pub(crate) fn connection_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

async fn serve(
    mut socket: TcpStream,
    handler: Arc<dyn Fn(&Seen) -> Reply + Send + Sync>,
    seen: Arc<Mutex<Vec<Seen>>>,
) {
    let mut buffer = Vec::new();
    loop {
        let Some(request) = read_request(&mut socket, &mut buffer).await else {
            return;
        };
        seen.lock().unwrap().push(request.clone());
        match handler(&request) {
            Reply::Body(body) => {
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                if socket.write_all(head.as_bytes()).await.is_err()
                    || socket.write_all(&body).await.is_err()
                {
                    return;
                }
            }
            Reply::Status(status) => {
                let head = format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\n\r\n");
                if socket.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
            }
            Reply::Hang => tokio::time::sleep(Duration::from_secs(3600)).await,
            Reply::Close => return,
            Reply::Raw(bytes) => {
                let _ = socket.write_all(&bytes).await;
                return;
            }
        }
    }
}

async fn read_request(socket: &mut TcpStream, buffer: &mut Vec<u8>) -> Option<Seen> {
    let head_end = loop {
        if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break position + 4;
        }
        let mut chunk = [0_u8; 1024];
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split(' ');
    let (method, path) = (first.next()?.to_owned(), first.next()?.to_owned());
    let length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len() < head_end + length {
        let mut chunk = [0_u8; 1024];
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = buffer[head_end..head_end + length].to_vec();
    buffer.drain(..head_end + length);
    Some(Seen { method, path, body })
}
