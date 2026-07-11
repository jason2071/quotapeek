//! Data model: raw Anthropic `/api/oauth/usage` response + the normalized
//! snapshot we hand to the webview.

use serde::{Deserialize, Serialize};

// ---------- Normalized output (serialized to the frontend, camelCase) ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub used_pct: f64,
    pub resets_at: Option<i64>, // epoch ms
    pub severity: String,
    pub is_active: bool,
    /// Token count for the window — only set in the transcript-approximation
    /// fallback (Endpoint C), where a true server % is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<i64>,
}

impl Bucket {
    pub fn new(used_pct: f64, resets_at: Option<i64>, severity: String, is_active: bool) -> Self {
        Bucket {
            used_pct,
            resets_at,
            severity,
            is_active,
            tokens: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraBucket {
    pub label: String,
    pub used_pct: f64,
    pub resets_at: Option<i64>,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credits {
    pub enabled: bool,
    pub spent: f64,
    pub currency: String,
    pub percent: f64,
    pub limit: Option<f64>,
    pub can_purchase: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub plan: String,
    pub five_hour: Option<Bucket>,
    pub weekly: Option<Bucket>,
    pub extra_buckets: Vec<ExtraBucket>,
    pub credits: Option<Credits>,
    pub fetched_at: i64,
    pub staleness: String,
    pub status: String,
    pub error: Option<String>,
}

impl UsageSnapshot {
    pub fn failed(plan: String, status: &str, error: impl Into<String>) -> Self {
        UsageSnapshot {
            plan,
            five_hour: None,
            weekly: None,
            extra_buckets: Vec::new(),
            credits: None,
            fetched_at: chrono::Utc::now().timestamp_millis(),
            staleness: "live".into(),
            status: status.into(),
            error: Some(error.into()),
        }
    }
}

// ---------- Raw API response ----------

#[derive(Debug, Deserialize)]
pub struct RawUsage {
    #[serde(default)]
    pub limits: Vec<RawLimit>,
    #[serde(default)]
    pub spend: Option<RawSpend>,
    #[serde(default)]
    pub five_hour: Option<RawBucket>,
    #[serde(default)]
    pub seven_day: Option<RawBucket>,
}

#[derive(Debug, Deserialize)]
pub struct RawLimit {
    pub kind: String, // "session" | "weekly_all" | "weekly_scoped"
    #[serde(default)]
    pub percent: f64,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub scope: Option<RawScope>,
    #[serde(default)]
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RawScope {
    #[serde(default)]
    pub model: Option<RawModel>,
}

#[derive(Debug, Deserialize)]
pub struct RawModel {
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawBucket {
    #[serde(default)]
    pub utilization: Option<f64>,
    #[serde(default)]
    pub resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawSpend {
    #[serde(default)]
    pub used: Option<RawSpendUsed>,
    #[serde(default)]
    pub limit: Option<serde_json::Value>,
    #[serde(default)]
    pub percent: Option<f64>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub can_purchase_credits: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RawSpendUsed {
    #[serde(default)]
    pub amount_minor: i64,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub exponent: i32,
}

// ---------- Normalization ----------

fn parse_reset(s: &Option<String>) -> Option<i64> {
    let raw = s.as_ref()?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn sev(s: &Option<String>) -> String {
    s.clone().unwrap_or_else(|| "normal".into())
}

pub fn normalize(raw: RawUsage, plan: String) -> UsageSnapshot {
    let mut five_hour: Option<Bucket> = None;
    let mut weekly: Option<Bucket> = None;
    let mut extra_buckets: Vec<ExtraBucket> = Vec::new();

    for l in &raw.limits {
        match l.kind.as_str() {
            "session" => {
                five_hour = Some(Bucket::new(
                    l.percent,
                    parse_reset(&l.resets_at),
                    sev(&l.severity),
                    l.is_active.unwrap_or(false),
                ));
            }
            "weekly_all" => {
                weekly = Some(Bucket::new(
                    l.percent,
                    parse_reset(&l.resets_at),
                    sev(&l.severity),
                    l.is_active.unwrap_or(false),
                ));
            }
            "weekly_scoped" => {
                let label = l
                    .scope
                    .as_ref()
                    .and_then(|s| s.model.as_ref())
                    .and_then(|m| m.display_name.clone())
                    .unwrap_or_else(|| "Model".into());
                extra_buckets.push(ExtraBucket {
                    label,
                    used_pct: l.percent,
                    resets_at: parse_reset(&l.resets_at),
                    severity: sev(&l.severity),
                });
            }
            _ => {}
        }
    }

    // Fallback to the flat top-level buckets if the `limits` array was absent.
    if five_hour.is_none() {
        if let Some(b) = &raw.five_hour {
            five_hour = Some(Bucket::new(
                b.utilization.unwrap_or(0.0),
                parse_reset(&b.resets_at),
                "normal".into(),
                true,
            ));
        }
    }
    if weekly.is_none() {
        if let Some(b) = &raw.seven_day {
            weekly = Some(Bucket::new(
                b.utilization.unwrap_or(0.0),
                parse_reset(&b.resets_at),
                "normal".into(),
                false,
            ));
        }
    }

    let credits = raw.spend.as_ref().map(|s| {
        let (spent, currency) = s
            .used
            .as_ref()
            .map(|u| {
                let div = 10f64.powi(u.exponent.max(0));
                (u.amount_minor as f64 / div, u.currency.clone())
            })
            .unwrap_or((0.0, "USD".into()));
        Credits {
            enabled: s.enabled.unwrap_or(false),
            spent,
            currency: if currency.is_empty() { "USD".into() } else { currency },
            percent: s.percent.unwrap_or(0.0),
            limit: s.limit.as_ref().and_then(|v| v.as_f64()),
            can_purchase: s.can_purchase_credits.unwrap_or(false),
        }
    });

    UsageSnapshot {
        plan,
        five_hour,
        weekly,
        extra_buckets,
        credits,
        fetched_at: chrono::Utc::now().timestamp_millis(),
        staleness: "live".into(),
        status: "ok".into(),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../fixtures/claude-usage.json");

    #[test]
    fn parses_real_response() {
        let raw: RawUsage = serde_json::from_str(FIXTURE).expect("fixture parses");
        let snap = normalize(raw, "Max (5x)".into());

        let five = snap.five_hour.expect("session bucket present");
        assert_eq!(five.used_pct, 11.0);
        assert!(five.is_active);
        assert!(five.resets_at.is_some());

        let weekly = snap.weekly.expect("weekly bucket present");
        assert_eq!(weekly.used_pct, 6.0);
        assert!(weekly.resets_at.is_some());

        // The scoped weekly bucket carries the rotating model codename ("Fable").
        assert_eq!(snap.extra_buckets.len(), 1);
        assert_eq!(snap.extra_buckets[0].label, "Fable");
        assert_eq!(snap.extra_buckets[0].used_pct, 0.0);

        let credits = snap.credits.expect("credits present");
        assert_eq!(credits.spent, 0.0);
        assert_eq!(credits.currency, "USD");
        assert!(!credits.enabled);

        assert_eq!(snap.status, "ok");
    }

    #[test]
    fn reset_parses_to_epoch_ms() {
        let s = Some("2026-07-16T05:00:00.043802+00:00".to_string());
        let ms = parse_reset(&s).expect("valid rfc3339");
        // 2026-07-16T05:00:00Z ≈ 1784178000000 ms
        assert!(ms > 1_780_000_000_000 && ms < 1_790_000_000_000);
    }
}
