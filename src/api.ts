import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type { Backend, Snapshot } from "./types";

function tauriBackend(): Backend {
  return {
    init: (language) => invoke("init", { language }),
    setLanguage: (language) => invoke("set_language", { language }),
    setTheme: (theme) => invoke("set_theme", { theme }),
    setAutostart: (enabled) => invoke("set_autostart", { enabled }),
    setNotifications: (enabled) => invoke("set_notifications", { enabled }),
    onHealth: (handler) => {
      void listen<Snapshot>("health", (event) => handler(event.payload));
    },
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
