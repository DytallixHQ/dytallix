//! HTTP client for Dytallix faucet endpoints.

use reqwest::Url;

use crate::error::SdkError;
use crate::{Balance, FaucetStatus};
use dytallix_core::address::DAddr;

/// Client for requesting DGT and DRT from a Dytallix faucet.
#[derive(Debug, Clone)]
pub struct FaucetClient {
    endpoint: String,
    http: reqwest::Client,
}

impl FaucetClient {
    /// Creates a faucet client for the provided endpoint.
    pub fn new(endpoint: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!("dytallix-sdk/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            http,
        }
    }

    /// Creates a faucet client for the canonical Dytallix testnet faucet.
    pub fn testnet() -> Self {
        Self::new("https://dytallix.com/api/faucet")
    }

    /// Requests both DGT and DRT for the provided address.
    pub async fn fund(&self, address: &DAddr) -> Result<Balance, SdkError> {
        let limits = self.get_limits().await?;
        let response = self
            .post_request(
                "/request",
                &FundRequest::new(address.clone(), limits.dgt, limits.drt),
            )
            .await?;

        if response.balances.dgt > 0 || response.balances.drt > 0 {
            Ok(response.balances)
        } else {
            Ok(Balance {
                dgt: response.funded.dgt,
                drt: response.funded.drt,
            })
        }
    }

    /// Requests DGT for the provided address.
    pub async fn fund_dgt(&self, address: &DAddr) -> Result<u128, SdkError> {
        let limits = self.get_limits().await?;
        let response = self
            .post_request(
                "/request",
                &FundRequest::new(address.clone(), limits.dgt, 0),
            )
            .await?;

        Ok(if response.funded.dgt > 0 {
            response.funded.dgt
        } else {
            limits.dgt
        })
    }

    /// Requests DRT for the provided address.
    pub async fn fund_drt(&self, address: &DAddr) -> Result<u128, SdkError> {
        let limits = self.get_limits().await?;
        let response = self
            .post_request(
                "/request",
                &FundRequest::new(address.clone(), 0, limits.drt),
            )
            .await?;

        Ok(if response.funded.drt > 0 {
            response.funded.drt
        } else {
            limits.drt
        })
    }

    /// Fetches faucet eligibility and retry information for the provided address.
    pub async fn status(&self, address: &DAddr) -> Result<FaucetStatus, SdkError> {
        let url = self.url(&format!("/check/{address}"))?;
        let response =
            self.http
                .get(url.clone())
                .send()
                .await
                .map_err(|err| SdkError::FaucetUnavailable {
                    endpoint: url.to_string(),
                    reason: err.to_string(),
                })?;

        if response.status().is_success() {
            let status: FaucetCheckResponse = response
                .json()
                .await
                .map_err(|err| SdkError::Serialization(err.to_string()))?;
            let retry_after_seconds = status.retry_after_seconds.or_else(|| {
                status
                    .time_until_next
                    .map(|minutes| minutes.saturating_mul(60))
            });

            Ok(FaucetStatus {
                can_request: status.can_request,
                retry_after_seconds,
            })
        } else if response.status().as_u16() == 429 {
            Err(rate_limited_error(response).await)
        } else {
            let reason = response
                .text()
                .await
                .unwrap_or_else(|_| "request failed".to_owned());
            Err(SdkError::FaucetUnavailable {
                endpoint: url.to_string(),
                reason,
            })
        }
    }

    async fn post_request(
        &self,
        path: &str,
        request: &FundRequest,
    ) -> Result<FundResponse, SdkError> {
        let url = self.url(path)?;
        let response = self
            .http
            .post(url.clone())
            .json(request)
            .send()
            .await
            .map_err(|err| SdkError::FaucetUnavailable {
                endpoint: url.to_string(),
                reason: err.to_string(),
            })?;

        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|err| SdkError::Serialization(err.to_string()))
        } else if response.status().as_u16() == 429 {
            Err(rate_limited_error(response).await)
        } else {
            let reason = response
                .text()
                .await
                .unwrap_or_else(|_| "request failed".to_owned());
            Err(SdkError::FaucetUnavailable {
                endpoint: url.to_string(),
                reason,
            })
        }
    }

    async fn get_limits(&self) -> Result<FaucetLimits, SdkError> {
        let url = self.url("/status")?;
        let response =
            self.http
                .get(url.clone())
                .send()
                .await
                .map_err(|err| SdkError::FaucetUnavailable {
                    endpoint: url.to_string(),
                    reason: err.to_string(),
                })?;

        if response.status().is_success() {
            let status: FaucetStatusResponse = response
                .json()
                .await
                .map_err(|err| SdkError::Serialization(err.to_string()))?;

            Ok(status.limits)
        } else if response.status().as_u16() == 429 {
            Err(rate_limited_error(response).await)
        } else {
            let reason = response
                .text()
                .await
                .unwrap_or_else(|_| "request failed".to_owned());
            Err(SdkError::FaucetUnavailable {
                endpoint: url.to_string(),
                reason,
            })
        }
    }

    fn url(&self, path: &str) -> Result<Url, SdkError> {
        let joined = format!("{}{}", self.endpoint, path);
        Url::parse(&joined).map_err(|err| SdkError::Network(err.to_string()))
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct FundRequest {
    address: DAddr,
    dgt_amount: u128,
    drt_amount: u128,
}

impl FundRequest {
    fn new(address: DAddr, dgt_amount: u128, drt_amount: u128) -> Self {
        Self {
            address,
            dgt_amount,
            drt_amount,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct FaucetStatusResponse {
    limits: FaucetLimits,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct FaucetLimits {
    dgt: u128,
    drt: u128,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FaucetCheckResponse {
    #[serde(default, alias = "allowed")]
    can_request: bool,
    #[serde(default)]
    time_until_next: Option<u64>,
    #[serde(default, alias = "retry_after_seconds")]
    retry_after_seconds: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct FundResponse {
    #[serde(default = "default_balance")]
    balances: Balance,
    #[serde(default)]
    funded: FundedAmounts,
}

fn default_balance() -> Balance {
    Balance { dgt: 0, drt: 0 }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct FundedAmounts {
    #[serde(default)]
    dgt: u128,
    #[serde(default)]
    drt: u128,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FaucetRateLimitResponse {
    #[serde(default, alias = "retry_after_seconds")]
    retry_after_seconds: Option<u64>,
    #[serde(default, alias = "retry_after")]
    retry_after: Option<u64>,
    #[serde(default, alias = "cooldown_ms")]
    cooldown_ms: Option<u64>,
}

async fn rate_limited_error(response: reqwest::Response) -> SdkError {
    let headers = response.headers().clone();
    let body = response.text().await.unwrap_or_default();
    SdkError::FaucetRateLimited {
        retry_after_seconds: retry_after_seconds(&headers, Some(&body)),
    }
}

fn retry_after_seconds(headers: &reqwest::header::HeaderMap, body: Option<&str>) -> u64 {
    body.and_then(parse_retry_after_seconds)
        .or_else(|| {
            headers
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
        })
        .unwrap_or(60)
}

fn parse_retry_after_seconds(body: &str) -> Option<u64> {
    let parsed: FaucetRateLimitResponse = serde_json::from_str(body).ok()?;
    parsed
        .retry_after_seconds
        .or(parsed.retry_after)
        .or_else(|| {
            parsed
                .cooldown_ms
                .map(|milliseconds| milliseconds.div_ceil(1_000))
        })
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};

    use super::{parse_retry_after_seconds, retry_after_seconds, FaucetCheckResponse};

    #[test]
    fn faucet_check_parses_legacy_allowed_shape() {
        let parsed: FaucetCheckResponse =
            serde_json::from_str(r#"{"address":"dytallix1demo","allowed":true}"#).unwrap();
        assert!(parsed.can_request);
        assert!(parsed.time_until_next.is_none());
        assert!(parsed.retry_after_seconds.is_none());
    }

    #[test]
    fn faucet_check_parses_retry_after_seconds_shape() {
        let parsed: FaucetCheckResponse =
            serde_json::from_str(r#"{"canRequest":false,"retryAfterSeconds":120}"#).unwrap();
        assert!(!parsed.can_request);
        assert_eq!(parsed.retry_after_seconds, Some(120));
    }

    #[test]
    fn rate_limit_body_parses_retry_after_shape() {
        assert_eq!(
            parse_retry_after_seconds(r#"{"error":"IP cooldown active","retryAfter":28}"#),
            Some(28)
        );
    }

    #[test]
    fn rate_limit_body_parses_cooldown_ms_shape() {
        assert_eq!(
            parse_retry_after_seconds(r#"{"cooldownMs":60000}"#),
            Some(60)
        );
    }

    #[test]
    fn rate_limit_body_takes_priority_over_header() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("120"));

        assert_eq!(
            retry_after_seconds(&headers, Some(r#"{"retryAfter":28}"#)),
            28
        );
    }
}
