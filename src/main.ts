import "./style.css";
import { getSettings, getUsage } from "./api";
import { countdownText, renderSnapshot } from "./render";
import type { UsageSnapshot } from "./types";

const provider = new URLSearchParams(location.search).get("provider") ?? "claude";
document.body.dataset.provider = provider;

const MIN_REFRESH_MS = 15_000; // don't hammer on focus/manual spam
let pollMs = 90_000; // overridden from settings

const app = document.getElementById("app")!;
let last: UsageSnapshot | null = null;
let lastFetchAttempt = 0;
let loading = false;

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
  if (btn) btn.addEventListener("click", () => void load(true));
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
  } catch (e) {
    paint(errorSnapshot(String(e)));
  } finally {
    loading = false;
    app.classList.remove("is-loading");
  }
}

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

async function init(): Promise<void> {
  try {
    const settings = await getSettings();
    if (settings.refreshSecs > 0) pollMs = settings.refreshSecs * 1000;
  } catch {
    // keep default
  }
  void load(true);
  setInterval(() => void load(false), pollMs);
  setInterval(tick, 1000);
  window.addEventListener("focus", () => void load(false));
}

void init();
