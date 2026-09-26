//! Tests the current HTTP-status health contract with loopback fixtures.
//! A successful HTTP status does not prove oracle authenticity or result correctness.
use dytallix_node::consensus::{AIOracleClient, AIServiceConfig};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

struct Fixture {
    base_url: String,
    task: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(
    method: &'static str,
    path: &'static str,
    status: Option<u16>,
    body: &'static str,
    delay: Duration,
) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 1024];
            let n = stream.read(&mut buffer).await.unwrap();
            assert!(n > 0, "request ended before headers and body");
            request.extend_from_slice(&buffer[..n]);
            assert!(request.len() <= 16384, "test request exceeds fixture limit");
            if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&request[..end]).unwrap();
                let length = headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    assert_eq!(
                        headers.lines().next().unwrap(),
                        format!("{method} {path} HTTP/1.1")
                    );
                    break;
                }
            }
        }
        tokio::time::sleep(delay).await;
        if let Some(status) = status {
            let response = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
    });
    Fixture { base_url, task }
}

fn client(fixture: &Fixture, timeout_seconds: u64) -> AIOracleClient {
    AIOracleClient::new(AIServiceConfig {
        base_url: fixture.base_url.clone(),
        timeout_seconds,
        api_key: "fixture-key".into(),
        max_retries: 1,
        ..AIServiceConfig::default()
    })
}

#[tokio::test]
async fn successful_http_status_returns_true() {
    for status in [200, 204] {
        let mut fixture = fixture("GET", "/health", Some(status), "", Duration::ZERO).await;
        assert!(client(&fixture, 2).health_check().await.unwrap());
        (&mut fixture.task).await.unwrap();
    }
}

#[tokio::test]
async fn unsuccessful_http_status_returns_false() {
    for status in [404, 503] {
        let mut fixture = fixture("GET", "/health", Some(status), "{}", Duration::ZERO).await;
        assert!(!client(&fixture, 2).health_check().await.unwrap());
        (&mut fixture.task).await.unwrap();
    }
}

#[tokio::test]
async fn connection_closed_without_response_returns_error() {
    let mut fixture = fixture("GET", "/health", None, "", Duration::ZERO).await;
    assert!(client(&fixture, 2).health_check().await.is_err());
    (&mut fixture.task).await.unwrap();
}

fn assert_timeout(error: anyhow::Error) {
    assert!(error
        .downcast_ref::<reqwest::Error>()
        .expect("HTTP transport error")
        .is_timeout());
}

#[tokio::test]
async fn constructor_timeout_limits_health_request() {
    let fixture = fixture("GET", "/health", Some(200), "{}", Duration::from_secs(3)).await;
    let result = tokio::time::timeout(Duration::from_secs(4), client(&fixture, 1).health_check())
        .await
        .unwrap();
    assert_timeout(result.unwrap_err());
}

#[tokio::test]
async fn updated_timeout_limits_health_request() {
    let fixture = fixture("GET", "/health", Some(200), "{}", Duration::from_secs(3)).await;
    let mut client = client(&fixture, 5);
    client.set_timeout(1);
    let result = tokio::time::timeout(Duration::from_secs(4), client.health_check())
        .await
        .unwrap();
    assert_timeout(result.unwrap_err());
}

#[tokio::test]
async fn updated_timeout_limits_post_request() {
    let fixture = fixture("POST", "/services", Some(200), "{}", Duration::from_secs(3)).await;
    let mut client = client(&fixture, 5);
    client.set_timeout(1);
    let result = tokio::time::timeout(
        Duration::from_secs(4),
        client.post::<_, serde_json::Value>("services", &serde_json::json!({})),
    )
    .await
    .unwrap();
    assert_timeout(result.unwrap_err());
}

#[tokio::test]
async fn post_response_is_decoded_after_timeout_update() {
    let mut fixture = fixture(
        "POST",
        "/services",
        Some(200),
        "{\"status\":\"available\"}",
        Duration::ZERO,
    )
    .await;
    let mut client = client(&fixture, 5);
    client.set_timeout(2);
    let response: serde_json::Value = client
        .post("services", &serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(response, serde_json::json!({"status": "available"}));
    (&mut fixture.task).await.unwrap();
}

#[test]
fn test_health_check_response_creation() {
    use dytallix_node::consensus::{AIHealthCheckResponse, AIServiceLoad, AIServiceStatus};

    // Test creating a health check response manually
    let health_response = AIHealthCheckResponse {
        status: AIServiceStatus::Healthy,
        timestamp: 1234567890,
        response_time_ms: 150,
        version: Some("1.0.0".to_string()),
        details: Some(serde_json::json!({"test": "data"})),
        endpoints: Some(vec!["fraud".to_string(), "risk".to_string()]),
        load: Some(AIServiceLoad {
            cpu_usage: Some(45.5),
            memory_usage: Some(67.2),
            queue_size: Some(5),
            requests_per_second: Some(12.3),
            avg_response_time_ms: Some(150.0),
        }),
    };

    assert_eq!(health_response.status, AIServiceStatus::Healthy);
    assert_eq!(health_response.response_time_ms, 150);
    assert!(health_response.version.is_some());
    assert!(health_response.details.is_some());
    assert!(health_response.endpoints.is_some());
    assert!(health_response.load.is_some());

    println!("Health check response creation test passed");
}
