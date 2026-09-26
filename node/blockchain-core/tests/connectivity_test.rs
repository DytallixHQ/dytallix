//! HTTP status and transport checks use local fixtures with explicit outcomes.
use dytallix_node::consensus::{AIOracleClient, AIServiceConfig};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

#[path = "support/http_fixture.rs"]
mod http_fixture;
use http_fixture::Fixture;

fn client(fixture: &Fixture, seconds: u64) -> Arc<AIOracleClient> {
    Arc::new(AIOracleClient::new(AIServiceConfig {
        base_url: fixture.url.clone(),
        timeout_seconds: seconds,
        ..AIServiceConfig::default()
    }))
}
async fn health(fixture: &mut Fixture, client: &Arc<AIOracleClient>, status: u16) -> bool {
    let shared = client.clone();
    let task = tokio::spawn(async move { shared.health_check().await });
    let request = fixture.next("GET", "/health").await;
    assert!(request.body.is_empty());
    request.respond(status, "{}");
    task.await.unwrap().unwrap()
}

#[tokio::test]
async fn successful_health_status_confirms_http_connectivity() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, 2);
    assert!(health(&mut fixture, &client, 200).await);
    assert!(client.circuit_breaker_status().unwrap().is_none());
}

#[tokio::test]
async fn configured_timeout_reports_transport_timeout() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, 1);
    let task = tokio::spawn(async move { client.health_check().await });
    let pending = fixture.next("GET", "/health").await;
    let error = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.downcast_ref::<reqwest::Error>().unwrap().is_timeout());
    drop(pending);
}

#[tokio::test]
async fn same_client_can_make_repeated_health_requests() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, 2);
    for _ in 0..3 {
        assert!(health(&mut fixture, &client, 204).await);
    }
    // This fixture closes each response. This does not assert connection-pool reuse.
    assert_eq!(fixture.connections.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn connection_closed_without_response_reports_transport_error() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, 2);
    let task = tokio::spawn(async move { client.health_check().await });
    drop(fixture.next("GET", "/health").await);
    assert!(task
        .await
        .unwrap()
        .unwrap_err()
        .downcast_ref::<reqwest::Error>()
        .is_some());
}

#[tokio::test]
async fn client_retains_selected_endpoint() {
    let fixture = Fixture::new().await;
    assert_eq!(client(&fixture, 2).get_config().base_url, fixture.url);
}

#[tokio::test]
async fn missing_health_endpoint_returns_false() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, 2);
    assert!(!health(&mut fixture, &client, 404).await);
}
