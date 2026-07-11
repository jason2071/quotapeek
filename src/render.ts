import type { Bucket, ExtraBucket, Severity, UsageSnapshot } from "./types";

const WEEKDAY = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

function clampPct(n: number): number {
  return Math.max(0, Math.min(100, n));
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

// Right-aligned label for a bucket: server "% used", or "≈ N tok" in approx mode.
function pctLabel(usedPct: number, tokens?: number | null): string {
  if (tokens != null) return `≈ ${formatTokens(tokens)} tok`;
  return `${Math.round(usedPct)}% used`;
}

function severityClass(sev: Severity): string {
  if (sev === "critical") return "sev-critical";
  if (sev === "warning") return "sev-warning";
  return "sev-normal";
}

// "Resets in 4hr 31min" from an epoch-ms reset time.
export function countdownText(resetsAt: number | null, now: number): string {
  if (resetsAt == null) return "";
  const ms = resetsAt - now;
  if (ms <= 0) return "Resets now";
  const totalMin = Math.floor(ms / 60000);
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  if (h >= 24) {
    const d = Math.floor(h / 24);
    const rh = h % 24;
    return `Resets in ${d}d ${rh}hr`;
  }
  if (h > 0) return `Resets in ${h}hr ${m}min`;
  return `Resets in ${m}min`;
}

// "Resets Thu 12:00 PM" — absolute local time, used for the weekly window.
export function resetAbsoluteText(resetsAt: number | null): string {
  if (resetsAt == null) return "";
  const d = new Date(resetsAt);
  let h = d.getHours();
  const min = d.getMinutes().toString().padStart(2, "0");
  const ampm = h >= 12 ? "PM" : "AM";
  h = h % 12;
  if (h === 0) h = 12;
  return `Resets ${WEEKDAY[d.getDay()]} ${h}:${min} ${ampm}`;
}

function barRow(opts: {
  label: string;
  sub: string;
  usedPct: number;
  severity: Severity;
  tokens?: number | null;
  variant?: string;
}): string {
  const approx = opts.tokens != null;
  const variant = opts.variant ?? "weekly";
  return `
    <div class="row">
      <div class="row-head">
        <div class="row-label">${opts.label}</div>
        <div class="row-pct">${pctLabel(opts.usedPct, opts.tokens)}</div>
      </div>
      <div class="bar bar-${variant} ${severityClass(opts.severity)}${approx ? " bar-approx" : ""}">
        <div class="bar-fill" style="width:${approx ? 0 : clampPct(opts.usedPct)}%"></div>
      </div>
      <div class="row-sub">${opts.sub}</div>
    </div>`;
}

function sessionRow(b: Bucket | null, now: number): string {
  if (!b) {
    return `<div class="row"><div class="row-head"><div class="row-label">Current session</div><div class="row-pct muted">—</div></div><div class="bar sev-normal"><div class="bar-fill" style="width:0%"></div></div><div class="row-sub muted">No data</div></div>`;
  }
  const approx = b.tokens != null;
  return `
    <div class="row" data-countdown="${b.resetsAt ?? ""}">
      <div class="row-head">
        <div class="row-label">Current session</div>
        <div class="row-pct">${pctLabel(b.usedPct, b.tokens)}</div>
      </div>
      <div class="bar bar-session ${severityClass(b.severity)}${approx ? " bar-approx" : ""}">
        <div class="bar-fill" style="width:${approx ? 0 : clampPct(b.usedPct)}%"></div>
      </div>
      <div class="row-sub js-countdown">${countdownText(b.resetsAt, now)}</div>
    </div>`;
}

function extraRow(e: ExtraBucket): string {
  const sub = e.resetsAt != null ? resetAbsoluteText(e.resetsAt) : `You haven't used ${e.label} yet`;
  return barRow({
    label: e.label,
    sub: e.usedPct > 0 ? resetAbsoluteText(e.resetsAt) : sub,
    usedPct: e.usedPct,
    severity: e.severity,
    variant: "extra",
  });
}

function statusBanner(s: UsageSnapshot): string {
  if (s.status === "reauth_needed") {
    return `<div class="banner banner-warn">Re-auth needed — run <code>claude</code> to refresh the token.</div>`;
  }
  if (s.status === "rate_limited") {
    return `<div class="banner banner-warn">Rate limited — backing off. Showing last known values.</div>`;
  }
  if (s.status === "error") {
    return `<div class="banner banner-err">${s.error ?? "Failed to load usage"}</div>`;
  }
  return "";
}

function staleBadge(s: UsageSnapshot): string {
  if (s.staleness === "approx") return `<span class="badge badge-approx">approximate</span>`;
  if (s.staleness === "stale") return `<span class="badge badge-stale">stale</span>`;
  return "";
}

function relativeTime(fetchedAt: number, now: number): string {
  const sec = Math.max(0, Math.floor((now - fetchedAt) / 1000));
  if (sec < 5) return "just now";
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  return `${Math.floor(min / 60)}h ago`;
}

export function renderSnapshot(
  root: HTMLElement,
  s: UsageSnapshot,
  now: number,
  provider: string,
): void {
  const isCodex = provider === "codex";
  const title = isCodex ? "Codex usage limits" : "Plan usage limits";
  const weekly = s.weekly;
  const weeklySub = weekly
    ? weekly.tokens != null && weekly.resetsAt == null
      ? "rolling 7 days"
      : resetAbsoluteText(weekly.resetsAt)
    : "";
  const weeklyRow = weekly
    ? barRow({
        label: isCodex ? "Weekly limit" : "All models",
        sub: weeklySub,
        usedPct: weekly.usedPct,
        severity: weekly.severity,
        tokens: weekly.tokens,
      })
    : "";

  root.innerHTML = `
    <div class="card">
      <div class="titlebar" data-tauri-drag-region>
        <div class="title" data-tauri-drag-region>${title}</div>
        <div class="plan">${s.plan}</div>
      </div>

      ${statusBanner(s)}
      ${approxNote(s)}

      <div class="section">
        ${sessionRow(s.fiveHour, now)}
      </div>

      <div class="section">
        <div class="section-title">Weekly limits</div>
        ${weeklyRow}
        ${s.extraBuckets.map(extraRow).join("")}
      </div>

      ${creditsSection(s)}

      <div class="footer">
        <div class="footer-row">
          <div class="updated">Last updated: <span class="js-updated">${relativeTime(s.fetchedAt, now)}</span> ${staleBadge(s)}</div>
          <button class="refresh js-refresh" title="Refresh">⟳</button>
        </div>
      </div>
    </div>`;
}

function approxNote(s: UsageSnapshot): string {
  if (s.staleness === "approx") {
    return `<div class="note">Estimated from local transcripts — server unavailable. Percentages unknown.</div>`;
  }
  return "";
}

function creditsSection(s: UsageSnapshot): string {
  const c = s.credits;
  if (!c) return "";
  const amount = `${c.currency === "USD" ? "$" : ""}${c.spent.toFixed(2)}`;
  return `
    <div class="section credits">
      <div class="section-title">Usage credits</div>
      <div class="row-head">
        <div class="row-label muted">${amount} spent</div>
        <div class="row-pct muted">${c.enabled ? `${Math.round(c.percent)}% used` : "off"}</div>
      </div>
    </div>`;
}
