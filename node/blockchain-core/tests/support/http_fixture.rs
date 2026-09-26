//! Bounded loopback HTTP fixture. Dropping it aborts all connection handlers.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};

pub struct Request {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
    response: oneshot::Sender<(u16, String)>,
}
impl Request {
    pub fn respond(self, status: u16, body: &str) {
        self.response.send((status, body.to_owned())).unwrap();
    }
}
pub struct Fixture {
    pub url: String,
    requests: mpsc::Receiver<Request>,
    pub connections: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    pub async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, requests) = mpsc::channel(16);
        let connections = Arc::new(AtomicUsize::new(0));
        let accepted = connections.clone();
        let task = tokio::spawn(async move {
            let mut handlers = JoinSet::new();
            loop {
                tokio::select! {
                    result = handlers.join_next(), if !handlers.is_empty() => {
                        result.unwrap().unwrap();
                    }
                    accepted_socket = listener.accept() => {
                        let (mut stream, _) = accepted_socket.unwrap();
                        accepted.fetch_add(1, Ordering::SeqCst);
                        let tx = tx.clone();
                        handlers.spawn(async move {
                            let mut bytes = Vec::new();
                            let (method, path, body) = loop {
                                let mut buffer = [0; 1024];
                                let count = stream.read(&mut buffer).await.unwrap();
                                assert!(count > 0, "incomplete fixture request");
                                bytes.extend_from_slice(&buffer[..count]);
                                assert!(bytes.len() <= 16_384);
                                if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                                    let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                                    let length = headers.lines().filter_map(|line| line.split_once(':'))
                                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                                        .map(|(_, value)| value.trim().parse::<usize>().unwrap()).unwrap_or(0);
                                    if bytes.len() >= end + 4 + length {
                                        let mut first = headers.lines().next().unwrap().split_whitespace();
                                        break (first.next().unwrap().to_owned(), first.next().unwrap().to_owned(),
                                            bytes[end + 4..end + 4 + length].to_vec());
                                    }
                                }
                            };
                            let (response, rx) = oneshot::channel();
                            tx.send(Request { method, path, body, response }).await.unwrap();
                            if let Ok((status, body)) = rx.await {
                                let response = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                                // Cancellation may close the socket before this response.
                                let _ = stream.write_all(response.as_bytes()).await;
                            }
                        });
                    }
                }
            }
        });
        Self {
            url,
            requests,
            connections,
            task,
        }
    }

    pub async fn next(&mut self, method: &str, path: &str) -> Request {
        let request = tokio::time::timeout(Duration::from_secs(5), self.requests.recv())
            .await
            .unwrap()
            .expect("fixture server ended");
        assert_eq!(request.method, method);
        assert_eq!(request.path, path);
        request
    }
}
