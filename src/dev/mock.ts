import pkg from "../../package.json";
import type { Backend, Condition, Settings, Snapshot, TargetView } from "../types";

const TARGETS = [
  { id: "cloudflare", name: "Cloudflare", host: "1.1.1.1", base: 18 },
  { id: "google", name: "Google", host: "8.8.8.8", base: 21 },
];

function summarize(id: string, name: string, host: string, history: (number | null)[]): TargetView {
  const recent = history.slice(-60);
  const answered = recent.filter((v): v is number => v !== null);
  const lost = recent.length - answered.length;
  const diffs = answered.slice(1).map((v, i) => Math.abs(v - answered[i]));
  return {
    id,
    name,
    host,
    last: history.at(-1) ?? null,
    jitter: diffs.length ? Math.round(diffs.reduce((a, b) => a + b, 0) / diffs.length) : null,
    loss: recent.length ? (lost * 100) / recent.length : 0,
    failing: history.length >= 3 && history.slice(-3).every((v) => v === null),
    history,
  };
}

export function mockBackend(): Backend {
  const params = new URLSearchParams(location.search);
  const forced = params.get("condition") as Condition | null;
  let settings: Settings = {
    language: params.get("lang") ?? "",
    theme: (params.get("theme") as Settings["theme"]) ?? "system",
    autostart: false,
    notifications: true,
    retentionDays: 30,
    debug: false,
  };
  const histories = TARGETS.map(() => [] as (number | null)[]);
  const handlers: ((snapshot: Snapshot) => void)[] = [];

  const sample = (base: number, tick: number) => {
    if (forced === "down") return null;
    if (forced === "degraded") return Math.random() < 0.1 ? null : base + 120 + Math.round(Math.random() * 60);
    const spike = tick % 90 > 70 && tick % 90 < 75 ? 20 * Math.random() : 0;
    return Math.round(base + Math.random() * 4 + spike);
  };

  const snapshot = (): Snapshot => {
    const targets = TARGETS.map((t, i) => summarize(t.id, t.name, t.host, histories[i]));
    const best = targets.filter((t) => !t.failing).sort((a, b) => (a.last ?? 1e9) - (b.last ?? 1e9))[0];
    const condition: Condition = !best ? "down" : best.loss > 5 || (best.last ?? 1e9) > 150 ? "degraded" : "ok";
    return { condition, targets };
  };

  for (let tick = 0; tick < 120; tick += 1) {
    TARGETS.forEach((t, i) => histories[i].push(sample(t.base, tick)));
  }
  let tick = 120;
  window.setInterval(() => {
    tick += 1;
    TARGETS.forEach((t, i) => {
      histories[i].push(sample(t.base, tick));
      if (histories[i].length > 300) histories[i].shift();
    });
    const next = snapshot();
    handlers.forEach((handler) => handler(next));
  }, 1000);

  const update = async (next: Partial<Settings>) => {
    settings = { ...settings, ...next };
    return settings;
  };

  return {
    async init(language) {
      if (!settings.language) {
        const lower = language.toLowerCase();
        settings.language = lower.startsWith("tr") ? "tr" : lower.startsWith("ru") ? "ru" : "en";
      }
      return { settings, platform: params.get("platform") ?? "windows", version: pkg.version, health: snapshot() };
    },
    setLanguage: (language) => update({ language }),
    setTheme: (theme) => update({ theme }),
    setAutostart: (autostart) => update({ autostart }),
    setNotifications: (notifications) => update({ notifications }),
    onHealth(handler) {
      handlers.push(handler);
    },
    async minimize() {},
    async hide() {},
    async quit() {},
  };
}
