//! A tiny loopback HTTP/1.1 server for the client and download manager tests.
//! Each route answers with a queue of replies; the last reply repeats.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone)]
pub struct Reply {
    status: u16,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
    /// Sends only this many body bytes, then stalls without closing.
    stall_after: Option<usize>,
}

impl Reply {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            headers: vec![],
            stall_after: None,
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            body: format!("status {status}").into_bytes(),
            headers: vec![],
            stall_after: None,
        }
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn stalling(body: impl Into<Vec<u8>>, after: usize) -> Self {
        Self {
            stall_after: Some(after),
            ..Self::ok(body)
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    /// `GET`, `POST`...
    pub method: String,
    pub path: String,
    /// Header names are lowercase.
    pub headers: HashMap<String, String>,
    /// The body, read up to its `Content-Length`.
    pub body: Vec<u8>,
}

#[derive(Default)]
struct State {
    routes: HashMap<String, VecDeque<Reply>>,
    requests: Vec<RecordedRequest>,
}

pub struct TestServer {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
}

impl TestServer {
    pub async fn start(routes: Vec<(&str, Vec<Reply>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(State {
            routes: routes
                .into_iter()
                .map(|(path, replies)| (path.to_owned(), replies.into()))
                .collect(),
            requests: vec![],
        }));
        let shared = Arc::clone(&state);
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                tokio::spawn(handle(socket, Arc::clone(&shared)));
            }
        });
        Self { addr, state }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state.lock().unwrap().requests.clone()
    }

    pub fn hits(&self, path: &str) -> usize {
        self.requests().iter().filter(|r| r.path == path).count()
    }
}

async fn handle(socket: TcpStream, state: Arc<Mutex<State>>) {
    let mut reader = BufReader::new(socket);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_owned();
    let path = parts.next().unwrap_or("/").to_owned();
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).await.is_err() {
        return;
    }
    let reply = {
        let mut state = state.lock().unwrap();
        state.requests.push(RecordedRequest {
            method,
            path: path.clone(),
            headers,
            body,
        });
        match state.routes.get_mut(&path) {
            Some(queue) if queue.len() > 1 => queue.pop_front().unwrap(),
            Some(queue) => queue.front().cloned().unwrap_or(Reply::status(404)),
            None => Reply::status(404),
        }
    };
    let mut socket = reader.into_inner();
    let mut head = format!(
        "HTTP/1.1 {} Test\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.body.len()
    );
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    if socket.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    match reply.stall_after {
        Some(after) => {
            let _ = socket.write_all(&reply.body[..after]).await;
            let _ = socket.flush().await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        None => {
            let _ = socket.write_all(&reply.body).await;
            let _ = socket.shutdown().await;
        }
    }
}
