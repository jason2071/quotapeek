import { invoke } from "@tauri-apps/api/core";
import type { Settings, UpdateStatus, UsageSnapshot } from "./types";

export function getUsage(provider: string): Promise<UsageSnapshot> {
  return invoke<UsageSnapshot>("get_usage", { provider });
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function setShow(provider: string, visible: boolean): Promise<void> {
  return invoke("set_show", { provider, visible });
}

export function setAutostart(enabled: boolean): Promise<void> {
  return invoke("set_autostart", { enabled });
}

export function setAlwaysOnTop(enabled: boolean): Promise<void> {
  return invoke("set_always_on_top", { enabled });
}

export function setRefresh(secs: number): Promise<void> {
  return invoke("set_refresh", { secs });
}

export function checkUpdate(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("check_update");
}

export function installUpdate(): Promise<void> {
  return invoke("install_update");
}

export function reportUsage(provider: string, usedPct: number | null): Promise<void> {
  return invoke("report_usage", { provider, usedPct });
}

export function notify(title: string, body: string): Promise<void> {
  return invoke("notify", { title, body });
}

export function resetPositions(): Promise<void> {
  return invoke("reset_positions");
}
