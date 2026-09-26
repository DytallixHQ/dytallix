//! Real HTTP requests against controlled loopback fixtures. No public service is used.
use dytallix_node::consensus::http_circuit_breaker::{
    HttpCircuitBreakerConfig, HttpCircuitError, HttpCircuitState,
};
use dytallix_node::consensus::{AIOracleClient, AIServiceConfig, AIServiceType};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

#[path = "support/http_fixture.rs"]
mod http_fixture;
use http_fixture::Fixture;

const RECOVERY: Duration = Duration::from_millis(200);

fn policy(min_requests: usize, window_size: usize, threshold: u16) -> HttpCircuitBreakerConfig {
    HttpCircuitBreakerConfig {
        failure_threshold_bps: threshold,
        min_requests,
        window_size,
        recovery_timeout: RECOVERY,
    }
}
fn client(
    fixture: &Fixture,
    policy: HttpCircuitBreakerConfig,
    attempts: u32,
) -> Arc<AIOracleClient> {
    Arc::new(
        AIOracleClient::with_circuit_breaker(
            AIServiceConfig {
                base_url: fixture.url.clone(),
                timeout_seconds: 2,
                max_retries: attempts,
                retry_delay_ms: 0,
                ..AIServiceConfig::default()
            },
            policy,
        )
        .unwrap(),
    )
}
fn health(client: &Arc<AIOracleClient>) -> JoinHandle<anyhow::Result<bool>> {
    let client = client.clone();
    tokio::spawn(async move { client.health_check().await })
}
fn post(client: &Arc<AIOracleClient>) -> JoinHandle<anyhow::Result<Value>> {
    let client = client.clone();
    tokio::spawn(async move { client.post("analyze", &json!({"fixture": true})).await })
}
async fn health_status(fixture: &mut Fixture, client: &Arc<AIOracleClient>, status: u16) {
    let request = health(client);
    fixture.next("GET", "/health").await.respond(status, "{}");
    assert_eq!(
        request.await.unwrap().unwrap(),
        (200..300).contains(&status)
    );
}
fn assert_circuit_error(error: anyhow::Error, expected: HttpCircuitError) {
    assert_eq!(error.downcast_ref::<HttpCircuitError>(), Some(&expected));
}
async fn recovery_interval() {
    tokio::time::sleep(RECOVERY + Duration::from_millis(25)).await;
}

#[test]
fn rejects_invalid_policy_and_http_attempt_configuration() {
    let valid = policy(2, 4, 5000);
    for invalid in [
        HttpCircuitBreakerConfig {
            failure_threshold_bps: 0,
            ..valid
        },
        HttpCircuitBreakerConfig {
            failure_threshold_bps: 10001,
            ..valid
        },
        HttpCircuitBreakerConfig {
            min_requests: 0,
            ..valid
        },
        HttpCircuitBreakerConfig {
            min_requests: 5,
            ..valid
        },
        HttpCircuitBreakerConfig {
            window_size: 0,
            ..valid
        },
        HttpCircuitBreakerConfig {
            window_size: 65537,
            ..valid
        },
        HttpCircuitBreakerConfig {
            recovery_timeout: Duration::ZERO,
            ..valid
        },
    ] {
        assert!(matches!(
            AIOracleClient::with_circuit_breaker(AIServiceConfig::default(), invalid),
            Err(HttpCircuitError::InvalidConfig)
        ));
    }
    for config in [
        AIServiceConfig {
            max_retries: 0,
            ..AIServiceConfig::default()
        },
        AIServiceConfig {
            timeout_seconds: 0,
            ..AIServiceConfig::default()
        },
    ] {
        assert!(matches!(
            AIOracleClient::with_circuit_breaker(config, valid),
            Err(HttpCircuitError::InvalidConfig)
        ));
    }
}

#[tokio::test]
async fn minimum_samples_and_exact_threshold_open_and_block_http() {
    let mut fixture = Fixture::new().await;
    let client = client(
        &fixture,
        HttpCircuitBreakerConfig {
            recovery_timeout: Duration::from_secs(60),
            ..policy(2, 4, 5000)
        },
        1,
    );
    health_status(&mut fixture, &client, 503).await;
    assert_eq!(
        client.circuit_breaker_status().unwrap().unwrap().state,
        HttpCircuitState::Closed
    );
    health_status(&mut fixture, &client, 200).await;
    assert_circuit_error(
        client.health_check().await.unwrap_err(),
        HttpCircuitError::Open,
    );
    assert_circuit_error(
        client
            .post::<_, Value>("analyze", &json!({}))
            .await
            .unwrap_err(),
        HttpCircuitError::Open,
    );
    let status = client.circuit_breaker_status().unwrap().unwrap();
    assert_eq!(status.state, HttpCircuitState::Open);
    assert_eq!(
        (
            status.admitted,
            status.rejected,
            status.succeeded,
            status.failed
        ),
        (2, 2, 1, 1)
    );
    assert_eq!((status.window_requests, status.window_failures), (2, 1));
    assert_eq!(fixture.connections.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn bounded_window_evicts_old_outcomes() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(4, 4, 7500), 1);
    for status in [503, 200, 200, 200, 503, 503] {
        health_status(&mut fixture, &client, status).await;
        assert_eq!(
            client.circuit_breaker_status().unwrap().unwrap().state,
            HttpCircuitState::Closed
        );
    }
    health_status(&mut fixture, &client, 503).await;
    let status = client.circuit_breaker_status().unwrap().unwrap();
    assert_eq!(status.state, HttpCircuitState::Open);
    assert_eq!(
        (
            status.window_requests,
            status.window_failures,
            status.admitted
        ),
        (4, 3, 7)
    );
}

#[tokio::test]
async fn one_recovery_probe_blocks_concurrent_calls_and_success_closes() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 1);
    health_status(&mut fixture, &client, 503).await;
    recovery_interval().await;
    let probe = health(&client);
    let pending = fixture.next("GET", "/health").await;
    assert_eq!(
        client.circuit_breaker_status().unwrap().unwrap().state,
        HttpCircuitState::HalfOpen
    );
    for _ in 0..8 {
        assert_circuit_error(
            client.health_check().await.unwrap_err(),
            HttpCircuitError::ProbeInProgress,
        );
    }
    assert_eq!(fixture.connections.load(Ordering::SeqCst), 2);
    pending.respond(200, "{}");
    assert!(probe.await.unwrap().unwrap());
    let status = client.circuit_breaker_status().unwrap().unwrap();
    assert_eq!(status.state, HttpCircuitState::Closed);
    assert_eq!((status.window_requests, status.window_failures), (0, 0));
    health_status(&mut fixture, &client, 200).await;
}

#[tokio::test]
async fn failed_probe_reopens_and_later_probe_can_recover() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 1);
    health_status(&mut fixture, &client, 503).await;
    recovery_interval().await;
    health_status(&mut fixture, &client, 503).await;
    assert_eq!(
        client.circuit_breaker_status().unwrap().unwrap().state,
        HttpCircuitState::Open
    );
    recovery_interval().await;
    health_status(&mut fixture, &client, 200).await;
    assert_eq!(
        client.circuit_breaker_status().unwrap().unwrap().state,
        HttpCircuitState::Closed
    );
}

#[tokio::test]
async fn cancelled_probe_reopens_without_leaving_probe_lock() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 1);
    health_status(&mut fixture, &client, 503).await;
    recovery_interval().await;
    let probe = health(&client);
    let pending = fixture.next("GET", "/health").await;
    probe.abort();
    assert!(probe.await.unwrap_err().is_cancelled());
    drop(pending);
    let status = client.circuit_breaker_status().unwrap().unwrap();
    assert_eq!(status.state, HttpCircuitState::Open);
    assert_eq!(status.cancelled, 1);
    recovery_interval().await;
    health_status(&mut fixture, &client, 200).await;
    assert_eq!(
        client.circuit_breaker_status().unwrap().unwrap().state,
        HttpCircuitState::Closed
    );
}

#[tokio::test]
async fn cancelled_closed_request_does_not_report_service_failure() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 1);
    let request = post(&client);
    let pending = fixture.next("POST", "/analyze").await;
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    drop(pending);
    let status = client.circuit_breaker_status().unwrap().unwrap();
    assert_eq!(status.state, HttpCircuitState::Closed);
    assert_eq!(
        (status.cancelled, status.failed, status.window_requests),
        (1, 0, 0)
    );
}

#[tokio::test]
async fn reset_ignores_old_completion_and_clears_statistics() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 1);
    let request = health(&client);
    let pending = fixture.next("GET", "/health").await;
    health_status(&mut fixture, &client, 503).await;
    client.reset_circuit_breaker().unwrap();
    pending.respond(503, "{}");
    assert!(!request.await.unwrap().unwrap());
    let status = client.circuit_breaker_status().unwrap().unwrap();
    assert_eq!(status.state, HttpCircuitState::Closed);
    assert_eq!(
        (
            status.admitted,
            status.rejected,
            status.succeeded,
            status.failed,
            status.cancelled,
            status.window_requests
        ),
        (0, 0, 0, 0, 0, 0)
    );
    health_status(&mut fixture, &client, 200).await;
}

#[tokio::test]
async fn stale_success_cannot_close_an_open_circuit() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 1);
    let request = health(&client);
    let pending = fixture.next("GET", "/health").await;
    health_status(&mut fixture, &client, 503).await;
    pending.respond(200, "{}");
    assert!(request.await.unwrap().unwrap());
    let status = client.circuit_breaker_status().unwrap().unwrap();
    assert_eq!(status.state, HttpCircuitState::Open);
    assert_eq!(
        (status.succeeded, status.failed, status.window_failures),
        (1, 1, 1)
    );
}

#[tokio::test]
async fn old_failure_cannot_reopen_after_successful_probe() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 1);
    let old = health(&client);
    let pending = fixture.next("GET", "/health").await;
    health_status(&mut fixture, &client, 503).await;
    recovery_interval().await;
    health_status(&mut fixture, &client, 200).await;
    pending.respond(503, "{}");
    assert!(!old.await.unwrap().unwrap());
    let status = client.circuit_breaker_status().unwrap().unwrap();
    assert_eq!(status.state, HttpCircuitState::Closed);
    assert_eq!(
        (
            status.window_requests,
            status.window_failures,
            status.failed
        ),
        (0, 0, 2)
    );
}

#[tokio::test]
async fn post_retries_count_once_and_recovery_probe_never_retries() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 3);
    let request = post(&client);
    for _ in 0..3 {
        let received = fixture.next("POST", "/analyze").await;
        assert_eq!(
            serde_json::from_slice::<Value>(&received.body).unwrap(),
            json!({"fixture": true})
        );
        received.respond(503, "{}");
    }
    assert!(request.await.unwrap().is_err());
    let status = client.circuit_breaker_status().unwrap().unwrap();
    assert_eq!((status.admitted, status.failed), (1, 1));
    recovery_interval().await;
    let probe = post(&client);
    fixture.next("POST", "/analyze").await.respond(503, "{}");
    assert!(probe.await.unwrap().is_err());
    assert_eq!(fixture.connections.load(Ordering::SeqCst), 4);
    assert_eq!(
        client.circuit_breaker_status().unwrap().unwrap().state,
        HttpCircuitState::Open
    );
}

#[tokio::test]
async fn post_requires_valid_json_and_successful_probe_returns_real_body() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 1);
    let request = post(&client);
    fixture
        .next("POST", "/analyze")
        .await
        .respond(200, "invalid json");
    assert!(request.await.unwrap().is_err());
    assert_eq!(
        client.circuit_breaker_status().unwrap().unwrap().state,
        HttpCircuitState::Open
    );
    recovery_interval().await;
    let probe = post(&client);
    fixture
        .next("POST", "/analyze")
        .await
        .respond(200, "{\"fixture_response\":42}");
    assert_eq!(
        probe.await.unwrap().unwrap(),
        json!({"fixture_response": 42})
    );
    assert_eq!(
        client.circuit_breaker_status().unwrap().unwrap().state,
        HttpCircuitState::Closed
    );
}

#[tokio::test]
async fn transport_failure_and_timeout_are_counted_as_failures() {
    for timeout in [false, true] {
        let mut fixture = Fixture::new().await;
        let mut config = AIServiceConfig {
            base_url: fixture.url.clone(),
            max_retries: 1,
            ..AIServiceConfig::default()
        };
        config.timeout_seconds = 1;
        let client =
            Arc::new(AIOracleClient::with_circuit_breaker(config, policy(1, 2, 5000)).unwrap());
        let request = health(&client);
        let pending = fixture.next("GET", "/health").await;
        let hold = if timeout {
            Some(pending)
        } else {
            drop(pending);
            None
        };
        let error = tokio::time::timeout(Duration::from_secs(3), request)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        let transport = error.downcast_ref::<reqwest::Error>().unwrap();
        assert_eq!(transport.is_timeout(), timeout);
        drop(hold);
        let status = client.circuit_breaker_status().unwrap().unwrap();
        assert_eq!(status.state, HttpCircuitState::Open);
        assert_eq!((status.failed, status.cancelled), (1, 0));
    }
}

#[tokio::test]
async fn unfinished_analysis_returns_error_without_fabricated_success() {
    let fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 1);
    let error = client
        .request_ai_analysis(AIServiceType::FraudDetection, Default::default())
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "AI analysis transport is not implemented"
    );
    assert_eq!(fixture.connections.load(Ordering::SeqCst), 0);
    assert_eq!(
        client.circuit_breaker_status().unwrap().unwrap().admitted,
        0
    );
}

#[tokio::test]
async fn integer_threshold_distinguishes_adjacent_basis_points() {
    for threshold in [3333, 3334] {
        let mut fixture = Fixture::new().await;
        let client = client(&fixture, policy(3, 3, threshold), 1);
        for status in [503, 200, 200] {
            health_status(&mut fixture, &client, status).await;
        }
        assert_eq!(
            client.circuit_breaker_status().unwrap().unwrap().state,
            if threshold == 3333 {
                HttpCircuitState::Open
            } else {
                HttpCircuitState::Closed
            }
        );
    }
}

#[tokio::test]
async fn reset_during_probe_ignores_its_cancellation() {
    let mut fixture = Fixture::new().await;
    let client = client(&fixture, policy(1, 2, 5000), 1);
    health_status(&mut fixture, &client, 503).await;
    recovery_interval().await;
    let probe = post(&client);
    let pending = fixture.next("POST", "/analyze").await;
    client.reset_circuit_breaker().unwrap();
    probe.abort();
    assert!(probe.await.unwrap_err().is_cancelled());
    drop(pending);
    let status = client.circuit_breaker_status().unwrap().unwrap();
    assert_eq!(status.state, HttpCircuitState::Closed);
    assert_eq!(
        (status.admitted, status.cancelled, status.failed),
        (0, 0, 0)
    );
    health_status(&mut fixture, &client, 200).await;
}
