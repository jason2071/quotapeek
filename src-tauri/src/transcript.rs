//! Endpoint C (last-resort fallback): approximate usage from local Claude Code
//! transcripts when the server endpoints are unreachable.
//!
//! This CANNOT reproduce Anthropic's true % (which is cost/tier-weighted) or the
//! server's weekly reset time. It reports the *token counts* it can measure and a
//! 5-hour block reset it can derive, and the caller flags the whole snapshot as
//! "approximate". No fabricated percentages.

use crate::models::Bucket;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const FIVE_HOUR_MS: i64 = 5 * 60 * 60 * 1000;
const SEVEN_DAY_MS: i64 = 7 * 24 * 60 * 60 * 1000;

struct Event {
    ts_ms: i64,
    tokens: i64,
    request_id: String,
}

fn projects_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

/// Collect `*.jsonl` files under `dir` modified within the last 7 days.
fn collect_jsonl(dir: &Path, cutoff: SystemTime, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, cutoff, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let recent = entry
                .metadata()
                .and_then(|m| m.modified())
                .map(|mt| mt >= cutoff)
                .unwrap_or(true);
            if recent {
                out.push(path);
            }
        }
    }
}

fn parse_line(line: &str) -> Option<Event> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return None;
    }
    let ts = v.get("timestamp").and_then(|t| t.as_str())?;
    let ts_ms = chrono::DateTime::parse_from_rfc3339(ts)
        .ok()?
        .timestamp_millis();
    let usage = v.get("message").and_then(|m| m.get("usage"))?;
    let get = |k: &str| usage.get(k).and_then(|n| n.as_i64()).unwrap_or(0);
    // Exclude cache_read (cheap ~0.1x reads dominate the raw count and would
    // wildly overstate usage). Count the tokens that represent real work.
    let tokens = get("input_tokens") + get("output_tokens") + get("cache_creation_input_tokens");
    // Dedup key: requestId (retries repeat it); fall back to message.id.
    let request_id = v
        .get("requestId")
        .and_then(|r| r.as_str())
        .or_else(|| v.get("message").and_then(|m| m.get("id")).and_then(|i| i.as_str()))
        .unwrap_or("")
        .to_string();
    Some(Event {
        ts_ms,
        tokens,
        request_id,
    })
}

/// Returns `(five_hour, weekly)` approximate buckets, or `None` if no transcript
/// data is available. Buckets carry `tokens` and (for 5h) a derived reset time;
/// `used_pct` stays 0 because a real ceiling is unknown.
pub fn approximate() -> Option<(Bucket, Bucket)> {
    let dir = projects_dir()?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(7 * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut files = Vec::new();
    collect_jsonl(&dir, cutoff, &mut files);
    if files.is_empty() {
        return None;
    }

    let mut events: Vec<Event> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for path in files {
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if line.is_empty() {
                continue;
            }
            if let Some(ev) = parse_line(&line) {
                if !ev.request_id.is_empty() && !seen.insert(ev.request_id.clone()) {
                    continue; // duplicate request
                }
                events.push(ev);
            }
        }
    }
    if events.is_empty() {
        return None;
    }

    // Weekly: rolling 7-day token sum. Reset is server-anchored and not derivable.
    let weekly_tokens: i64 = events
        .iter()
        .filter(|e| e.ts_ms >= now_ms - SEVEN_DAY_MS)
        .map(|e| e.tokens)
        .sum();

    // 5-hour: rolling window; block start = earliest activity still inside it.
    let in_5h: Vec<&Event> = events
        .iter()
        .filter(|e| e.ts_ms >= now_ms - FIVE_HOUR_MS)
        .collect();
    let five_tokens: i64 = in_5h.iter().map(|e| e.tokens).sum();
    let block_start = in_5h.iter().map(|e| e.ts_ms).min();
    let five_reset = block_start.map(|s| s + FIVE_HOUR_MS);

    let mut five = Bucket::new(0.0, five_reset, "normal".into(), !in_5h.is_empty());
    five.tokens = Some(five_tokens);

    let mut weekly = Bucket::new(0.0, None, "normal".into(), false);
    weekly.tokens = Some(weekly_tokens);

    Some((five, weekly))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Exercises the recursive walk + JSONL parse against the real transcripts on
    // this machine. Must not panic; token counts must be non-negative.
    #[test]
    fn approximate_is_safe() {
        if let Some((five, weekly)) = approximate() {
            assert!(five.tokens.unwrap_or(0) >= 0);
            assert!(weekly.tokens.unwrap_or(0) >= 0);
            // 5h tokens can't exceed the 7d rolling sum.
            assert!(five.tokens.unwrap_or(0) <= weekly.tokens.unwrap_or(0));
            println!(
                "approx 5h={} tok (reset {:?}), 7d={} tok",
                five.tokens.unwrap_or(0),
                five.resets_at,
                weekly.tokens.unwrap_or(0)
            );
        } else {
            println!("no transcript data (fallback returns None)");
        }
    }
}
