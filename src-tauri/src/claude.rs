//! Fetch Claude usage with a fallback cascade:
//!   A) authoritative JSON from /api/oauth/usage        (default, no quota cost)
//!   B) unified rate-limit response headers from a       (only if A is Forbidden;
//!      minimal /v1/messages call — costs ~1 token)       different auth path)
//!   C) local transcript approximation                   (offline / A,B failed)

use crate::credentials::{self, Credentials};
use crate::models::{normalize, Bucket, RawUsage, UsageSnapshot};
use crate::transcript;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
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

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))
}

// ---------- Endpoint A ----------

async fn endpoint_a(creds: &Credentials, plan: &str) -> Result<UsageSnapshot, FetchError> {
    let client = client().map_err(FetchError::Other)?;
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

// ---------- Endpoint B (rate-limit headers) ----------

fn parse_reset_header(v: &str) -> Option<i64> {
    // Either unix seconds or an RFC3339 timestamp.
    if let Ok(secs) = v.parse::<i64>() {
        return Some(secs * 1000);
    }
    chrono::DateTime::parse_from_rfc3339(v)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

async fn endpoint_b(creds: &Credentials, plan: &str) -> Result<UsageSnapshot, String> {
    let client = client()?;
    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 1,
        "system": "You are Claude Code, Anthropic's official CLI for Claude.",
        "messages": [{ "role": "user", "content": "." }]
    });
    let resp = client
        .post(MESSAGES_URL)
        .header("Authorization", format!("Bearer {}", creds.access_token))
        .header("anthropic-beta", ANTHROPIC_BETA)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Endpoint B network error: {e}"))?;

    let headers = resp.headers().clone();
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(String::from);

    let five_util = get("anthropic-ratelimit-unified-5h-utilization")
        .and_then(|v| v.parse::<f64>().ok())
        .ok_or("No 5h rate-limit header")?;
    let five_reset = get("anthropic-ratelimit-unified-5h-reset").and_then(|v| parse_reset_header(&v));
    // Utilization headers are 0..1 floats.
    let five = Bucket::new(five_util * 100.0, five_reset, "normal".into(), true);

    let weekly = get("anthropic-ratelimit-unified-7d-utilization")
        .and_then(|v| v.parse::<f64>().ok())
        .map(|u| {
            let reset =
                get("anthropic-ratelimit-unified-7d-reset").and_then(|v| parse_reset_header(&v));
            Bucket::new(u * 100.0, reset, "normal".into(), false)
        });

    Ok(UsageSnapshot {
        plan: plan.to_string(),
        five_hour: Some(five),
        weekly,
        extra_buckets: Vec::new(),
        credits: None,
        fetched_at: now_ms(),
        staleness: "live".into(),
        status: "ok".into(),
        error: None,
    })
}

// ---------- Endpoint C (transcript approximation) ----------

fn endpoint_c(plan: &str) -> Option<UsageSnapshot> {
    let (five, weekly) = transcript::approximate()?;
    Some(UsageSnapshot {
        plan: plan.to_string(),
        five_hour: Some(five),
        weekly: Some(weekly),
        extra_buckets: Vec::new(),
        credits: None,
        fetched_at: now_ms(),
        staleness: "approx".into(),
        status: "ok".into(),
        error: None,
    })
}

// ---------- Orchestrator ----------

pub async fn fetch_usage() -> UsageSnapshot {
    let creds = match credentials::load() {
        Ok(c) => c,
        Err(e) => return UsageSnapshot::failed("Claude".into(), "reauth_needed", e),
    };
    let plan = creds.plan.clone();

    match endpoint_a(&creds, &plan).await {
        Ok(s) => s,
        Err(FetchError::Reauth(m)) => UsageSnapshot::failed(plan, "reauth_needed", m),
        Err(FetchError::RateLimited) => {
            // Don't spend quota on B. Offer a local estimate if we have one.
            if let Some(mut s) = endpoint_c(&plan) {
                s.status = "rate_limited".into();
                s.error = Some("Rate limited — showing local estimate.".into());
                s
            } else {
                UsageSnapshot::failed(plan, "rate_limited", "Rate limited by Anthropic.")
            }
        }
        Err(FetchError::Forbidden) => {
            // A is blocked; B uses the inference auth path and may still work.
            if let Ok(s) = endpoint_b(&creds, &plan).await {
                return s;
            }
            if let Some(s) = endpoint_c(&plan) {
                return s;
            }
            UsageSnapshot::failed(plan, "error", "Forbidden (token missing user:profile scope).")
        }
        Err(FetchError::Other(m)) => {
            // Network / server error: fall back to an offline estimate.
            if let Some(mut s) = endpoint_c(&plan) {
                s.error = Some(format!("Server unavailable ({m}) — local estimate."));
                return s;
            }
            UsageSnapshot::failed(plan, "error", m)
        }
    }
}
