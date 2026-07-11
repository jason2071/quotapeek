//! Fetch Claude usage:
//!   A) authoritative JSON from /api/oauth/usage      (default, no quota cost)
//!   C) local transcript approximation                (offline / A failed)
//!
//! Endpoint B (rate-limit headers via a minimal /v1/messages call) was removed:
//! it consumed real inference quota on every poll and polluted the very 5h bucket
//! it measured. Our login token carries `user:profile`, so A is authoritative; if
//! A is ever blocked we degrade to the offline approximation (C) instead.

use crate::credentials::{self, Credentials};
use crate::models::{normalize, RawUsage, UsageSnapshot};
use crate::transcript;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
// The `claude-code/*` User-Agent is REQUIRED to avoid an aggressive rate bucket.
const USER_AGENT: &str = "claude-code/1.0.0";
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";
const ANTHROPIC_VERSION: &str = "2023-06-01";

enum FetchError {
    Reauth(String),
    RateLimited,
    Forbidden,
    Other(String),
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

async fn endpoint_a(creds: &Credentials, plan: &str) -> Result<UsageSnapshot, FetchError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| FetchError::Other(format!("HTTP client: {e}")))?;

    let resp = client
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("anthropic-beta", ANTHROPIC_BETA)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                FetchError::Other("Request timed out".into())
            } else if e.is_connect() {
                FetchError::Other("No connection".into())
            } else {
                FetchError::Other(format!("Network error: {e}"))
            }
        })?;

    match resp.status().as_u16() {
        200 => {
            let raw: RawUsage = resp
                .json::<RawUsage>()
                .await
                .map_err(|e| FetchError::Other(format!("Bad response: {e}")))?;
            Ok(normalize(raw, plan.to_string()))
        }
        401 => {
            let hint = if creds.is_expired() {
                "Token expired — run `claude` to refresh."
            } else {
                "Unauthorized — run `claude` to re-authenticate."
            };
            Err(FetchError::Reauth(hint.into()))
        }
        403 => Err(FetchError::Forbidden),
        429 => Err(FetchError::RateLimited),
        code => Err(FetchError::Other(format!("HTTP {code}"))),
    }
}

/// Endpoint C — transcript approximation, run off the async runtime (the scan
/// reads many files) so it doesn't block a tokio worker.
async fn endpoint_c(plan: &str) -> Option<UsageSnapshot> {
    let plan = plan.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        transcript::approximate().map(|(five, weekly)| UsageSnapshot {
            plan,
            five_hour: Some(five),
            weekly: Some(weekly),
            extra_buckets: Vec::new(),
            credits: None,
            fetched_at: now_ms(),
            staleness: "approx".into(),
            status: "ok".into(),
            error: None,
        })
    })
    .await
    .ok()
    .flatten()
}

pub async fn fetch_usage() -> UsageSnapshot {
    let creds = match credentials::load() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("claude credentials unavailable: {e}");
            return UsageSnapshot::failed("Claude".into(), "reauth_needed", e);
        }
    };
    let plan = creds.plan.clone();

    match endpoint_a(&creds, &plan).await {
        Ok(s) => {
            tracing::info!(status = %s.status, "claude usage ok");
            s
        }
        Err(FetchError::Reauth(m)) => UsageSnapshot::failed(plan, "reauth_needed", m),
        Err(FetchError::RateLimited) => {
            tracing::warn!("claude rate limited (429)");
            match endpoint_c(&plan).await {
                Some(mut s) => {
                    s.status = "rate_limited".into();
                    s.error = Some("Rate limited — showing local estimate.".into());
                    s
                }
                None => UsageSnapshot::failed(plan, "rate_limited", "Rate limited by Anthropic."),
            }
        }
        Err(FetchError::Forbidden) => {
            tracing::warn!("claude 403 — token likely missing user:profile scope");
            match endpoint_c(&plan).await {
                Some(mut s) => {
                    s.error = Some("Token missing user:profile scope — run `claude`.".into());
                    s
                }
                None => UsageSnapshot::failed(
                    plan,
                    "error",
                    "Forbidden (token missing user:profile scope).",
                ),
            }
        }
        Err(FetchError::Other(m)) => {
            tracing::warn!("claude fetch error: {m}");
            match endpoint_c(&plan).await {
                Some(mut s) => {
                    s.error = Some(format!("Server unavailable ({m}) — local estimate."));
                    s
                }
                None => UsageSnapshot::failed(plan, "error", m),
            }
        }
    }
}
