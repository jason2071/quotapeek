import "./style.css";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  checkUpdate,
  getSettings,
  installUpdate,
  setAlwaysOnTop,
  setAutostart,
  setRefresh,
  setShow,
} from "./api";

const root = document.getElementById("settings")!;

const INTERVALS: [number, string][] = [
  [30, "30 sec"],
  [60, "1 min"],
  [90, "1.5 min"],
  [120, "2 min"],
  [300, "5 min"],
];

function bindCheck(sel: string, fn: (v: boolean) => Promise<void>): void {
  const el = root.querySelector<HTMLInputElement>(sel);
  if (el) el.addEventListener("change", () => void fn(el.checked));
}

async function runUpdateCheck(): Promise<void> {
  const btn = root.querySelector<HTMLButtonElement>(".js-check");
  const statusEl = root.querySelector<HTMLElement>(".js-update-status");
  if (!btn || !statusEl) return;

  btn.disabled = true;
  statusEl.className = "update-status js-update-status";
  statusEl.textContent = "Checking…";
  try {
    const r = await checkUpdate();
    if (r.status === "available") {
      statusEl.textContent = `v${r.version} available`;
      statusEl.classList.add("ok");
      btn.textContent = "Install & restart";
      btn.classList.add("primary");
      btn.disabled = false;
      btn.onclick = () => {
        statusEl.textContent = "Installing…";
        btn.disabled = true;
        void installUpdate().catch((e) => {
          statusEl.textContent = String(e);
          statusEl.classList.add("err");
          btn.disabled = false;
        });
      };
    } else if (r.status === "uptodate") {
      statusEl.textContent = "Up to date";
      statusEl.classList.add("ok");
      btn.disabled = false;
    } else {
      statusEl.textContent = r.message ?? "Check failed";
      statusEl.classList.add("err");
      btn.disabled = false;
    }
  } catch (e) {
    statusEl.textContent = String(e);
    statusEl.classList.add("err");
    btn.disabled = false;
  }
}

async function render(): Promise<void> {
  const s = await getSettings();
  root.innerHTML = `
    <div class="settings-card">
      <div class="settings-head" data-tauri-drag-region>
        <div class="settings-title" data-tauri-drag-region>Settings</div>
        <button class="close js-close" title="Close" aria-label="Close settings">✕</button>
      </div>

      <div class="settings-section">
        <div class="settings-section-title">Widgets</div>
        <label class="opt"><span>Show Claude widget</span><input type="checkbox" class="js-claude" ${s.showClaude ? "checked" : ""} /></label>
        <label class="opt"><span>Show Codex widget</span><input type="checkbox" class="js-codex" ${s.showCodex ? "checked" : ""} /></label>
      </div>

      <div class="settings-section">
        <div class="settings-section-title">General</div>
        <label class="opt"><span>Start at login</span><input type="checkbox" class="js-autostart" ${s.autostart ? "checked" : ""} /></label>
        <label class="opt"><span>Always on top</span><input type="checkbox" class="js-aot" ${s.alwaysOnTop ? "checked" : ""} /></label>
        <label class="opt"><span>Refresh interval</span>
          <select class="js-refresh">
            ${INTERVALS.map(([v, l]) => `<option value="${v}" ${s.refreshSecs === v ? "selected" : ""}>${l}</option>`).join("")}
          </select>
        </label>
      </div>

      <div class="settings-section">
        <div class="settings-section-title">Updates</div>
        <div class="opt"><span>Version</span><span class="version js-version muted">…</span></div>
        <div class="update-row">
          <button class="btn js-check">Check for updates</button>
          <span class="update-status js-update-status"></span>
        </div>
      </div>
    </div>`;

  root.querySelector(".js-close")?.addEventListener("click", () => void getCurrentWindow().hide());
  bindCheck(".js-claude", (v) => setShow("claude", v));
  bindCheck(".js-codex", (v) => setShow("codex", v));
  bindCheck(".js-autostart", (v) => setAutostart(v));
  bindCheck(".js-aot", (v) => setAlwaysOnTop(v));
  const sel = root.querySelector<HTMLSelectElement>(".js-refresh");
  if (sel) sel.addEventListener("change", () => void setRefresh(Number(sel.value)));

  const checkBtn = root.querySelector<HTMLButtonElement>(".js-check");
  if (checkBtn) checkBtn.onclick = () => void runUpdateCheck();

  getVersion()
    .then((v) => {
      const el = root.querySelector<HTMLElement>(".js-version");
      if (el) el.textContent = `v${v}`;
    })
    .catch(() => {});
}

void render();
