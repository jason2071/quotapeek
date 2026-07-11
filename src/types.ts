// Mirrors the Rust `UsageSnapshot` serialized to the webview (serde camelCase).

export type Severity = "normal" | "warning" | "critical" | string;
export type Staleness = "live" | "stale" | "approx";
export type Status = "ok" | "reauth_needed" | "rate_limited" | "error";

export interface Bucket {
  usedPct: number;
  resetsAt: number | null; // epoch ms
  severity: Severity;
  isActive: boolean;
  tokens?: number | null; // only in the approximate (Endpoint C) fallback
}

export interface ExtraBucket {
  label: string;
  usedPct: number;
  resetsAt: number | null;
  severity: Severity;
}

export interface Credits {
  enabled: boolean;
  spent: number; // in currency units, e.g. dollars
  currency: string;
  percent: number;
  limit: number | null;
  canPurchase: boolean;
}

export interface UsageSnapshot {
  plan: string;
  fiveHour: Bucket | null;
  weekly: Bucket | null;
  extraBuckets: ExtraBucket[];
  credits: Credits | null;
  fetchedAt: number; // epoch ms
  staleness: Staleness;
  status: Status;
  error: string | null;
}

export interface Settings {
  showClaude: boolean;
  showCodex: boolean;
  autostart: boolean;
  alwaysOnTop: boolean;
  refreshSecs: number;
}
