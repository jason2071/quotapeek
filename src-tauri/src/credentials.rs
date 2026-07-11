//! Read the Claude Code OAuth token that Claude Code keeps on disk.
//! The widget stays read-only on this file — Claude Code owns refreshing it.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct CredFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeOauth>,
}

#[derive(Debug, Deserialize)]
struct ClaudeOauth {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>, // epoch ms
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(rename = "rateLimitTier")]
    rate_limit_tier: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Credentials {
    pub access_token: String,
    pub expires_at: Option<i64>,
    pub plan: String,
}

impl Credentials {
    /// True when the stored token is past its expiry (best-effort; Claude Code
    /// may already have refreshed the file with a newer token).
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => chrono::Utc::now().timestamp_millis() >= exp,
            None => false,
        }
    }
}

fn credentials_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join(".credentials.json"))
}

fn plan_label(sub: Option<&str>, tier: Option<&str>) -> String {
    let base: String = match sub {
        Some("max") => "Max".to_string(),
        Some("pro") => "Pro".to_string(),
        Some("free") => "Free".to_string(),
        Some(other) if !other.is_empty() => {
            let mut c = other.chars();
            match c.next() {
                Some(f) => format!("{}{}", f.to_uppercase(), c.as_str()),
                None => "Claude".to_string(),
            }
        }
        _ => "Claude".to_string(),
    };

    if let Some(t) = tier {
        if t.contains("20x") {
            return format!("{} (20x)", base);
        }
        if t.contains("5x") {
            return format!("{} (5x)", base);
        }
    }
    base
}

/// Load and parse the Claude Code credentials. Returns a human-readable error
/// string on any failure so the caller can surface it in the widget.
pub fn load() -> Result<Credentials, String> {
    let path = credentials_path().ok_or("Could not resolve home directory")?;
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    let parsed: CredFile =
        serde_json::from_str(&text).map_err(|e| format!("Invalid credentials JSON: {}", e))?;
    let oauth = parsed
        .claude_ai_oauth
        .ok_or("No claudeAiOauth section — is Claude Code logged in?")?;

    if oauth.access_token.is_empty() {
        return Err("Empty access token".into());
    }

    let plan = plan_label(
        oauth.subscription_type.as_deref(),
        oauth.rate_limit_tier.as_deref(),
    );

    Ok(Credentials {
        access_token: oauth.access_token,
        expires_at: oauth.expires_at,
        plan,
    })
}
