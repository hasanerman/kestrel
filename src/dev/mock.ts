import pkg from "../../package.json";
import type { Backend, Category, Condition, Kind, Minute, Outage, Settings, SiteResult, Snapshot, TargetView } from "../types";

const TARGETS: { id: string; name: string; host: string; kind: Kind; base: number }[] = [
  { id: "router", name: "Router", host: "192.168.1.1", kind: "router", base: 2 },
  { id: "cloudflare", name: "Cloudflare", host: "1.1.1.1", kind: "internet", base: 18 },
  { id: "google", name: "Google", host: "8.8.8.8", kind: "internet", base: 21 },
];

const HOUR = 3_600_000;

const CATEGORIES: Category[] = [
  { id: "social", hosts: ["discord.com", "youtube.com", "x.com", "instagram.com", "reddit.com"] },
  { id: "gaming", hosts: ["store.steampowered.com", "roblox.com", "playvalorant.com"] },
  { id: "news", hosts: ["wikipedia.org", "bbc.com"] },
  { id: "dev", hosts: ["github.com", "npmjs.com"] },
];

function fakeSite(host: string): SiteResult {
  const base = { host, dns: 12, dnsDiffers: false, tcp: 30, https: 180, status: 200, failure: null };
  if (host === "discord.com") return { ...base, verdict: "dpiBlocked", https: null, status: null, failure: "reset" };
  if (host === "x.com") return { ...base, verdict: "dnsBlocked", tcp: null, https: null, status: null, failure: "blockPage" };
  if (host === "roblox.com") return { ...base, verdict: "dnsBlocked", dnsDiffers: true, https: null, status: null, failure: "timeout" };
  return { ...base, verdict: "works", dns: 8 + Math.round(Math.random() * 30), tcp: 15 + Math.round(Math.random() * 40) };
}

function summarize(id: string, name: string, host: string, kind: Kind, history: (number | null)[]): TargetView {
  const recent = history.slice(-60);
  const answered = recent.filter((v): v is number => v !== null);
  const lost = recent.length - answered.length;
  const diffs = answered.slice(1).map((v, i) => Math.abs(v - answered[i]));
  return {
    id,
    name,
    host,
    kind,
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
    sites: ["example.com"],
  };
  const histories = TARGETS.map(() => [] as (number | null)[]);
  const handlers: ((snapshot: Snapshot) => void)[] = [];
  const siteHandlers: ((result: SiteResult) => void)[] = [];
  const started = Date.now() - 95_000;
  const yesterday = new Date();
  yesterday.setDate(yesterday.getDate() - 1);
  yesterday.setHours(21, 14, 0, 0);
  const recent: Outage[] =
    params.get("drops") === "none"
      ? []
      : [
          { start: yesterday.getTime(), end: yesterday.getTime() + 180_000, cause: "provider" },
          { start: yesterday.getTime() - 26 * HOUR, end: yesterday.getTime() - 26 * HOUR + 40_000, cause: "local" },
        ];
  const minutesFor = (count: number): Minute[] =>
    Array.from({ length: count }, (_, i) => {
      const at = Date.now() - (count - i) * 60_000;
      const dropped = count > 100 && i > count - 180 && i < count - 176;
      return { at, ping: dropped ? null : Math.round(18 + Math.random() * 5 + (i % 240 < 20 ? 15 : 0)), loss: dropped ? 100 : 0 };
    });

  const sample = (base: number, tick: number) => {
    if (forced === "down") return base < 5 ? base : null;
    if (forced === "degraded") return Math.random() < 0.1 ? null : base + 120 + Math.round(Math.random() * 60);
    const spike = tick % 90 > 70 && tick % 90 < 75 ? 20 * Math.random() : 0;
    return Math.round(base + Math.random() * 4 + spike);
  };

  const snapshot = (): Snapshot => {
    const targets = TARGETS.map((t, i) => summarize(t.id, t.name, t.host, t.kind, histories[i]));
    const best = targets.filter((t) => t.kind === "internet" && !t.failing).sort((a, b) => (a.last ?? 1e9) - (b.last ?? 1e9))[0];
    const condition: Condition = !best ? "down" : best.loss > 5 || (best.last ?? 1e9) > 150 ? "degraded" : "ok";
    const outage: Outage | null = condition === "down" ? { start: started, end: null, cause: "provider" } : null;
    return { condition, method: "icmp", outage, recent, targets };
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
      const update = params.get("update") !== null ? { version: "0.2.0", notes: "- Speed test\n- Game server targets" } : null;
      return { settings, platform: params.get("platform") ?? "windows", version: pkg.version, health: snapshot(), update };
    },
    setLanguage: (language) => update({ language }),
    setTheme: (theme) => update({ theme }),
    setAutostart: (autostart) => update({ autostart }),
    setNotifications: (notifications) => update({ notifications }),
    async siteLists() {
      return CATEGORIES;
    },
    async checkSites(hosts) {
      const results = await Promise.all(
        hosts.map(
          (host, i) =>
            new Promise<SiteResult>((resolve) =>
              window.setTimeout(() => {
                const result = fakeSite(host);
                siteHandlers.forEach((handler) => handler(result));
                resolve(result);
              }, 300 + i * 120),
            ),
        ),
      );
      return results;
    },
    setSites: (sites) => update({ sites }),
    onSite(handler) {
      siteHandlers.push(handler);
    },
    async history(minutes) {
      return minutesFor(minutes);
    },
    onHealth(handler) {
      handlers.push(handler);
    },
    async installUpdate() {
      await new Promise((resolve) => window.setTimeout(resolve, 1500));
    },
    onUpdate() {},
    setRetention: (retentionDays) => update({ retentionDays }),
    async openRepository() {
      window.open("https://github.com/hasanerman/kestrel", "_blank");
    },
    async reportIssue() {
      window.open("https://github.com/hasanerman/kestrel/issues/new", "_blank");
    },
    async diagnostics() {
      return `kestrel ${pkg.version}\nos: windows x86_64`;
    },
    async minimize() {},
    async hide() {},
    async quit() {},
  };
}
