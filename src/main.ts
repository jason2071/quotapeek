import "./style.css";
import { listen } from "@tauri-apps/api/event";
import { getSettings, getUsage, notify, reportUsage } from "./api";
import { countdownText, renderSnapshot } from "./render";
import type { UsageSnapshot } from "./types";

const provider = new URLSearchParams(location.search).get("provider") ?? "claude";
document.body.dataset.provider = provider;

const MIN_REFRESH_MS = 10_000; // don't hammer on focus/manual spam
const MAX_BACKOFF_MS = 10 * 60_000; // cap the 429 back-off

let pollMs = 90_000; // base cadence (from settings)
let backoffMs = pollMs; // current (grows on 429)

const app = document.getElementById("app")!;
let last: UsageSnapshot | null = null;
let lastFetchAttempt = 0;
let loading = false;
let timer: number | undefined;

function errorSnapshot(msg: string): UsageSnapshot {
  return {
    plan: last?.plan ?? (provider === "codex" ? "Codex" : "Claude"),
    fiveHour: last?.fiveHour ?? null,
    weekly: last?.weekly ?? null,
    extraBuckets: last?.extraBuckets ?? [],
    credits: last?.credits ?? null,
    fetchedAt: last?.fetchedAt ?? Date.now(),
    staleness: last ? "stale" : "live",
    status: "error",
    error: msg,
  };
}

function paint(s: UsageSnapshot): void {
  renderSnapshot(app, s, Date.now(), provider);
  const btn = app.querySelector<HTMLButtonElement>(".js-refresh");
  if (btn) btn.addEventListener("click", manualRefresh);
}

async function load(force = false): Promise<void> {
  const now = Date.now();
  if (loading) return;
  if (!force && now - lastFetchAttempt < MIN_REFRESH_MS) return;
  loading = true;
  lastFetchAttempt = now;
  app.classList.add("is-loading");
  try {
    const snap = await getUsage(provider);
    last = snap;
    paint(snap);
    reportTooltip(snap);
    checkLimits(snap);
  } catch (e) {
    paint(errorSnapshot(String(e)));
  } finally {
    loading = false;
    app.classList.remove("is-loading");
  }
}

function schedule(delay: number): void {
  if (timer !== undefined) clearTimeout(timer);
  timer = window.setTimeout(runPoll, delay);
}
function stop(): void {
  if (timer !== undefined) {
    clearTimeout(timer);
    timer = undefined;
  }
}

async function runPoll(): Promise<void> {
  await load(false);
  // Exponential back-off while rate-limited; reset to base otherwise.
  backoffMs = last?.status === "rate_limited" ? Math.min(backoffMs * 2, MAX_BACKOFF_MS) : pollMs;
  if (!document.hidden) schedule(backoffMs);
}

function manualRefresh(): void {
  backoffMs = pollMs;
  void load(true).then(() => {
    if (!document.hidden) schedule(pollMs);
  });
}

// Live-update countdown + "last updated" every second (no full re-render).
function tick(): void {
  const now = Date.now();
  app.querySelectorAll<HTMLElement>("[data-countdown]").forEach((row) => {
    const raw = row.getAttribute("data-countdown");
    const span = row.querySelector<HTMLElement>(".js-countdown");
    if (span && raw) span.textContent = countdownText(Number(raw), now);
  });
  if (last) {
    const upd = app.querySelector<HTMLElement>(".js-updated");
    if (upd) {
      const sec = Math.max(0, Math.floor((now - last.fetchedAt) / 1000));
      upd.textContent = sec < 5 ? "just now" : sec < 60 ? `${sec}s ago` : `${Math.floor(sec / 60)}m ago`;
    }
  }
}

// Report a headline % to the tray tooltip (max of session/weekly; null in approx).
function reportTooltip(s: UsageSnapshot): void {
  const hasData = s.fiveHour != null || s.weekly != null;
  const headline = Math.max(s.fiveHour?.usedPct ?? 0, s.weekly?.usedPct ?? 0);
  void reportUsage(provider, hasData ? headline : null);
}

// Near-limit warnings: notify once when a bucket crosses 80% / 95% (rising edge).
const notifiedLevel: Record<string, number> = { session: 0, weekly: 0 };
function levelFor(pct: number): number {
  return pct >= 95 ? 95 : pct >= 80 ? 80 : 0;
}
function checkLimits(s: UsageSnapshot): void {
  const rows: [string, number | null | undefined, string][] = [
    ["session", s.fiveHour?.tokens != null ? null : s.fiveHour?.usedPct, "5-hour session"],
    ["weekly", s.weekly?.tokens != null ? null : s.weekly?.usedPct, "weekly limit"],
  ];
  for (const [key, pct, label] of rows) {
    if (pct == null) continue;
    const lvl = levelFor(pct);
    if (lvl > notifiedLevel[key]) {
      void notify(
        `${s.plan} — ${label} at ${Math.round(pct)}%`,
        lvl >= 95 ? "Almost out — usage is very high." : "Heads up — usage is getting high.",
      );
    }
    notifiedLevel[key] = lvl;
  }
}

// Pause polling while the widget is hidden; force a fresh load when shown.
document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    stop();
  } else {
    backoffMs = pollMs;
    void load(true);
    schedule(pollMs);
  }
});

async function init(): Promise<void> {
  try {
    const settings = await getSettings();
    if (settings.refreshSecs > 0) {
      pollMs = settings.refreshSecs * 1000;
      backoffMs = pollMs;
    }
  } catch {
    // keep default
  }

  await load(true);
  if (!document.hidden) schedule(pollMs);
  setInterval(tick, 1000);
  window.addEventListener("focus", () => void load(false));

  // Refresh-interval changed in Settings → apply without a restart.
  await listen<number>("settings-changed", (e) => {
    const secs = Number(e.payload);
    if (secs > 0) {
      pollMs = secs * 1000;
      backoffMs = pollMs;
      if (!document.hidden) schedule(pollMs);
    }
  });

  // Tray "Refresh now" → force an immediate reload.
  await listen("force-refresh", () => void load(true));
}

void init();
