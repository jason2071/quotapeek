//! Codex provider. Primary source is the live, non-inference ChatGPT backend
//! endpoint `GET /backend-api/wham/usage` (zero model quota consumed). Falls back
//! to the newest session rollout log (offline, stale) when the network fails.
//!
//! Auth is read-only: we reuse the `access_token` the Codex CLI keeps in
//! `~/.codex/auth.json` and never write that file (the CLI owns token refresh).

use crate::models::{Bucket, UsageSnapshot};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const USER_AGENT: &str = "codex_cli_rs/0.144.1 (QuotaPeek widget)";
const STALE_AFTER_MS: i64 = 15 * 60 * 1000;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn codex_home() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codex"))
}

fn severity_for(pct: f64) -> String {
    if pct >= 90.0 {
        "critical".into()
    } else if pct >= 75.0 {
        "warning".into()
    } else {
        "normal".into()
    }
}

fn plan_label(plan_type: Option<&str>) -> String {
    match plan_type {
        Some(s) if !s.is_empty() => {
            let mut c = s.chars();
            match c.next() {
                Some(f) => format!("{}{}", f.to_uppercase(), c.as_str()),
                None => "Plus".to_string(),
            }
        }
        _ => "Plus".to_string(),
    }
}

// ---------- Auth (read-only) ----------

#[derive(Deserialize)]
struct AuthFile {
    tokens: Option<AuthTokens>,
}

#[derive(Deserialize)]
struct AuthTokens {
    access_token: Option<String>,
    account_id: Option<String>,
}

fn read_auth_once(home: &Path) -> Result<(String, String), String> {
    let text = std::fs::read_to_string(home.join("auth.json"))
        .map_err(|_| "Codex not logged in — run `codex`.".to_string())?;
    let parsed: AuthFile = serde_json::from_str(&text).map_err(|e| format!("auth.json: {e}"))?;
    let tokens = parsed.tokens.ok_or("Codex not logged in — run `codex`.")?;
    let access = tokens
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or("No Codex access token — run `codex`.")?;
    let account = tokens.account_id.unwrap_or_default();
    Ok((access, account))
}

/// Read auth, retrying once — the CLI may be rewriting/rotating the token file.
fn read_auth(home: &Path) -> Result<(String, String), String> {
    match read_auth_once(home) {
        Ok(v) => Ok(v),
        Err(_) => {
            std::thread::sleep(Duration::from_millis(60));
            read_auth_once(home)
        }
    }
}

// ---------- Live endpoint (wham/usage) ----------

#[derive(Deserialize)]
struct WhamUsage {
    plan_type: Option<String>,
    rate_limit: Option<WhamRate>,
}

#[derive(Deserialize)]
struct WhamRate {
    primary_window: Option<WhamWindow>,
    secondary_window: Option<WhamWindow>,
}

#[derive(Deserialize)]
struct WhamWindow {
    #[serde(default)]
    used_percent: f64,
    #[serde(default)]
    limit_window_seconds: Option<i64>,
    #[serde(default)]
    reset_at: Option<i64>, // unix seconds
}

enum LiveErr {
    Unauthorized,
    Other,
}

// The 5-hour vs weekly windows are NOT fixed to primary/secondary — the API puts
// whichever is currently active in `primary_window` (e.g. when there's no recent
// 5h usage, `primary_window` IS the weekly one and `secondary_window` is null).
// Classify by window length instead of position.
const FIVE_HOUR_MAX_SECS: i64 = 6 * 3600;

fn window_bucket(w: &WhamWindow) -> Bucket {
    Bucket::new(
        w.used_percent,
        w.reset_at.map(|s| s * 1000),
        severity_for(w.used_percent),
        true,
    )
}

fn zero_bucket() -> Bucket {
    Bucket::new(0.0, None, "normal".into(), false)
}

/// Sort the available windows into (five_hour, weekly) by their length.
fn classify(windows: &[Option<&WhamWindow>]) -> (Bucket, Bucket) {
    let mut five: Option<Bucket> = None;
    let mut weekly: Option<Bucket> = None;
    for w in windows.iter().copied().flatten() {
        let secs = w.limit_window_seconds.unwrap_or(0);
        if secs > 0 && secs <= FIVE_HOUR_MAX_SECS {
            five.get_or_insert_with(|| window_bucket(w));
        } else if secs > FIVE_HOUR_MAX_SECS {
            weekly.get_or_insert_with(|| window_bucket(w));
        }
    }
    (five.unwrap_or_else(zero_bucket), weekly.unwrap_or_else(zero_bucket))
}

async fn fetch_live(access: &str, account: &str) -> Result<UsageSnapshot, LiveErr> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| LiveErr::Other)?;

    let resp = client
        .get(USAGE_URL)
        .header("Authorization", format!("Bearer {access}"))
        .header("ChatGPT-Account-ID", account)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|_| LiveErr::Other)?;

    match resp.status().as_u16() {
        200 => {
            let u: WhamUsage = resp.json().await.map_err(|_| LiveErr::Other)?;
            let rate = u.rate_limit.ok_or(LiveErr::Other)?;
            let (five_hour, weekly) = classify(&[
                rate.primary_window.as_ref(),
                rate.secondary_window.as_ref(),
            ]);
            Ok(UsageSnapshot {
                plan: plan_label(u.plan_type.as_deref()),
                five_hour: Some(five_hour),
                weekly: Some(weekly),
                extra_buckets: Vec::new(),
                credits: None,
                fetched_at: now_ms(),
                staleness: "live".into(),
                status: "ok".into(),
                error: None,
            })
        }
        401 | 403 => Err(LiveErr::Unauthorized),
        _ => Err(LiveErr::Other),
    }
}

// ---------- Offline fallback (newest rollout log) ----------

fn newest_rollout(sessions: &Path) -> Option<(PathBuf, i64)> {
    let mut best: Option<(PathBuf, SystemTime)> = None;
    fn walk(dir: &Path, best: &mut Option<(PathBuf, SystemTime)>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, best);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
                .unwrap_or(false)
            {
                if let Ok(mt) = entry.metadata().and_then(|m| m.modified()) {
                    if best.as_ref().map(|(_, b)| mt > *b).unwrap_or(true) {
                        *best = Some((path, mt));
                    }
                }
            }
        }
    }
    walk(sessions, &mut best);
    best.map(|(p, mt)| {
        let ms = mt
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(|_| now_ms());
        (p, ms)
    })
}

/// Parse a rollout window: returns (window_minutes, Bucket) for classification.
fn rollout_window(limit: &serde_json::Value) -> Option<(i64, Bucket)> {
    let pct = limit.get("used_percent")?.as_f64()?;
    let wmin = limit.get("window_minutes").and_then(|x| x.as_i64()).unwrap_or(0);
    let resets_at = limit.get("resets_at").and_then(|r| r.as_i64()).map(|s| s * 1000);
    Some((wmin, Bucket::new(pct, resets_at, severity_for(pct), true)))
}

fn read_offline(home: &Path) -> Option<UsageSnapshot> {
    let (path, mtime_ms) = newest_rollout(&home.join("sessions"))?;
    let text = std::fs::read_to_string(&path).ok()?;
    let line = text.lines().rev().find(|l| l.contains("\"rate_limits\""))?;
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let rl = v.pointer("/payload/rate_limits")?;

    let plan = plan_label(rl.get("plan_type").and_then(|p| p.as_str()));
    // Classify by window length (same reasoning as the live path).
    let mut five: Option<Bucket> = None;
    let mut weekly: Option<Bucket> = None;
    for key in ["primary", "secondary"] {
        if let Some((wmin, b)) = rl.get(key).and_then(rollout_window) {
            if wmin > 0 && wmin <= 360 {
                five.get_or_insert(b);
            } else if wmin > 360 {
                weekly.get_or_insert(b);
            }
        }
    }
    let age = now_ms() - mtime_ms;

    Some(UsageSnapshot {
        plan,
        five_hour: Some(five.unwrap_or_else(zero_bucket)),
        weekly: Some(weekly.unwrap_or_else(zero_bucket)),
        extra_buckets: Vec::new(),
        credits: None,
        fetched_at: mtime_ms,
        staleness: if age > STALE_AFTER_MS { "stale" } else { "live" }.into(),
        status: "ok".into(),
        error: None,
    })
}

// ---------- Orchestrator ----------

/// Offline read on a blocking thread (the rollout scan/read is sync + heavy).
async fn offline(home: PathBuf) -> Option<UsageSnapshot> {
    tauri::async_runtime::spawn_blocking(move || read_offline(&home))
        .await
        .ok()
        .flatten()
}

pub async fn fetch_usage() -> UsageSnapshot {
    let home = match codex_home() {
        Some(h) => h,
        None => return UsageSnapshot::failed("Codex".into(), "error", "No home directory"),
    };

    let (access, account) = match read_auth(&home) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("codex auth unavailable: {e}");
            return UsageSnapshot::failed("Codex".into(), "reauth_needed", e);
        }
    };

    match fetch_live(&access, &account).await {
        Ok(snap) => {
            tracing::info!("codex usage live");
            snap
        }
        Err(LiveErr::Unauthorized) => {
            tracing::warn!("codex 401 — token expired, falling back to offline");
            match offline(home.clone()).await {
                Some(mut s) => {
                    s.error =
                        Some("Live token expired — run `codex`. Showing local snapshot.".into());
                    s
                }
                None => UsageSnapshot::failed(
                    "Codex".into(),
                    "reauth_needed",
                    "Codex token expired — run `codex`.",
                ),
            }
        }
        Err(LiveErr::Other) => {
            tracing::warn!("codex live fetch failed, falling back to offline");
            offline(home.clone())
                .await
                .unwrap_or_else(|| UsageSnapshot::failed("Codex".into(), "error", "Codex usage unavailable."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wham_classifies_windows_by_length() {
        let fixture = include_str!("../../fixtures/codex-usage.json");
        let u: WhamUsage = serde_json::from_str(fixture).expect("wham usage parses");
        let rate = u.rate_limit.expect("rate_limit present");
        let (five, weekly) =
            classify(&[rate.primary_window.as_ref(), rate.secondary_window.as_ref()]);
        assert_eq!(five.used_pct, 1.0); // 18000s window
        assert_eq!(five.resets_at, Some(1783783197_000));
        assert_eq!(weekly.used_pct, 0.0); // 604800s window
        assert_eq!(plan_label(u.plan_type.as_deref()), "Plus");
    }

    // Regression: when there's no active 5h window the API puts the WEEKLY window
    // in `primary_window` and leaves `secondary_window` null. It must map to weekly,
    // not to "Current session".
    #[test]
    fn wham_weekly_only_maps_to_weekly() {
        let json = r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":3,"limit_window_seconds":604800,"reset_at":1784542515},"secondary_window":null}}"#;
        let u: WhamUsage = serde_json::from_str(json).unwrap();
        let rate = u.rate_limit.unwrap();
        let (five, weekly) =
            classify(&[rate.primary_window.as_ref(), rate.secondary_window.as_ref()]);
        assert_eq!(weekly.used_pct, 3.0);
        assert_eq!(weekly.resets_at, Some(1784542515_000));
        assert_eq!(five.used_pct, 0.0); // no 5h window → 0%
        assert_eq!(five.resets_at, None);
    }

    #[test]
    fn parses_rollout_fixture() {
        let fixture = include_str!("../../fixtures/codex-rollout.jsonl");
        let line = fixture
            .lines()
            .rev()
            .find(|l| l.contains("\"rate_limits\""))
            .expect("rollout has rate_limits");
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let rl = v.pointer("/payload/rate_limits").expect("rate_limits");
        let (wmin, five) = rl.get("primary").and_then(rollout_window).expect("primary");
        assert_eq!(wmin, 300);
        assert_eq!(five.used_pct, 92.0);
        assert_eq!(five.severity, "critical");
        let (wmin2, weekly) = rl.get("secondary").and_then(rollout_window).expect("secondary");
        assert_eq!(wmin2, 10080);
        assert_eq!(weekly.used_pct, 14.0);
    }
}
