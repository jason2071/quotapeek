//! Codex provider: read OpenAI Codex CLI usage offline from the newest session
//! rollout log. No network — the rate-limit snapshot is written by the Codex CLI
//! on each turn, so it's only as fresh as the last Codex activity.

use crate::models::{Bucket, UsageSnapshot};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Newest `rollout-*.jsonl` under `~/.codex/sessions`, with its modified time (ms).
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

fn bucket_from(limit: &serde_json::Value) -> Option<Bucket> {
    let pct = limit.get("used_percent")?.as_f64()?;
    let resets_at = limit
        .get("resets_at")
        .and_then(|r| r.as_i64())
        .map(|secs| secs * 1000);
    Some(Bucket::new(pct, resets_at, severity_for(pct), true))
}

pub fn fetch_usage() -> UsageSnapshot {
    let home = match codex_home() {
        Some(h) => h,
        None => return UsageSnapshot::failed("Codex".into(), "error", "No home directory"),
    };

    if !home.join("auth.json").exists() {
        return UsageSnapshot::failed(
            "Codex".into(),
            "reauth_needed",
            "Codex not logged in — run `codex`.",
        );
    }

    let (path, mtime_ms) = match newest_rollout(&home.join("sessions")) {
        Some(v) => v,
        None => {
            return UsageSnapshot::failed("Codex".into(), "error", "No Codex sessions yet.")
        }
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return UsageSnapshot::failed("Codex".into(), "error", format!("Read error: {e}")),
    };

    // Reverse-scan for the last line carrying a rate_limits object.
    let line = text
        .lines()
        .rev()
        .find(|l| l.contains("\"rate_limits\""));
    let line = match line {
        Some(l) => l,
        None => {
            return UsageSnapshot::failed(
                "Codex".into(),
                "error",
                "No usage data in latest session.",
            )
        }
    };

    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return UsageSnapshot::failed("Codex".into(), "error", format!("Parse error: {e}")),
    };

    let rl = match v.pointer("/payload/rate_limits") {
        Some(rl) => rl,
        None => {
            return UsageSnapshot::failed("Codex".into(), "error", "Malformed rate_limits.")
        }
    };

    let plan = rl
        .get("plan_type")
        .and_then(|p| p.as_str())
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                Some(f) => format!("{}{}", f.to_uppercase(), c.as_str()),
                None => "Plus".to_string(),
            }
        })
        .unwrap_or_else(|| "Plus".into());

    let five_hour = rl.get("primary").and_then(bucket_from);
    let weekly = rl.get("secondary").and_then(bucket_from);

    let age = now_ms() - mtime_ms;
    let staleness = if age > STALE_AFTER_MS { "stale" } else { "live" };

    UsageSnapshot {
        plan,
        five_hour,
        weekly,
        extra_buckets: Vec::new(),
        credits: None,
        fetched_at: mtime_ms, // Codex data is as fresh as the last turn, not "now".
        staleness: staleness.into(),
        status: "ok".into(),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rate_limits_fixture() {
        let fixture = include_str!("../../fixtures/codex-rollout.jsonl");
        let line = fixture
            .lines()
            .rev()
            .find(|l| l.contains("\"rate_limits\""))
            .expect("fixture has a rate_limits line");
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let rl = v.pointer("/payload/rate_limits").expect("rate_limits present");

        let five = rl.get("primary").and_then(bucket_from).expect("primary");
        assert_eq!(five.used_pct, 92.0);
        assert_eq!(five.severity, "critical"); // 92% → critical
        assert!(five.resets_at.is_some());

        let weekly = rl.get("secondary").and_then(bucket_from).expect("secondary");
        assert_eq!(weekly.used_pct, 14.0);
        assert_eq!(weekly.severity, "normal");
    }
}
