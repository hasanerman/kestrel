import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type { Backend, SiteResult, Snapshot, UpdateInfo } from "./types";

function tauriBackend(): Backend {
  return {
    init: (language) => invoke("init", { language }),
    setLanguage: (language) => invoke("set_language", { language }),
    setTheme: (theme) => invoke("set_theme", { theme }),
    setAutostart: (enabled) => invoke("set_autostart", { enabled }),
    setNotifications: (enabled) => invoke("set_notifications", { enabled }),
    history: (minutes) => invoke("history", { minutes }),
    siteLists: () => invoke("site_lists"),
    checkSites: (hosts) => invoke("check_sites", { hosts }),
    setSites: (sites) => invoke("set_sites", { sites }),
    onSite: (handler) => {
      void listen<SiteResult>("site", (event) => handler(event.payload));
    },
    onHealth: (handler) => {
      void listen<Snapshot>("health", (event) => handler(event.payload));
    },
    installUpdate: () => invoke("install_update"),
    onUpdate: (handler) => {
      void listen<UpdateInfo>("update", (event) => handler(event.payload));
    },
    setRetention: (days) => invoke("set_retention", { days }),
    openRepository: () => invoke("open_repository"),
    reportIssue: () => invoke("report_issue"),
    diagnostics: () => invoke("diagnostics"),
    minimize: () => getCurrentWindow().minimize(),
    hide: () => getCurrentWindow().hide(),
    quit: () => invoke("quit"),
  };
}

export async function createBackend(): Promise<Backend> {
  if ("__TAURI_INTERNALS__" in window) {
    return tauriBackend();
  }
  if (import.meta.env.DEV) {
    const { mockBackend } = await import("./dev/mock");
    return mockBackend();
  }
  throw new Error("kestrel must run inside the desktop app.");
}
