import pkg from "../../package.json";
import type { Backend, Category, FlowView, TrafficSnapshot, Condition, Kind, Minute, Outage, Settings, SiteResult, Snapshot, TargetView } from "../types";

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
    blocked: [],
    gameTargets: params.get("games") !== null ? ["vie", "fra"] : [],
    customTargets: params.get("games") !== null ? ["185.40.65.1"] : [],
  };
  const histories = TARGETS.map(() => [] as (number | null)[]);
  const handlers: ((snapshot: Snapshot) => void)[] = [];
  const siteHandlers: ((result: SiteResult) => void)[] = [];
  const speedHandlers: ((progress: { phase: "download" | "upload"; mbps: number }) => void)[] = [];
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
    const extra: TargetView[] = [
      ...settings.gameTargets.map((code, i) => summarize(`game:${code}`, code === "vie" ? "CS2 Vienna" : "CS2 Frankfurt", code === "vie" ? "146.66.155.66" : "155.133.226.68", "game", histories[1].map((v) => (v === null ? null : v + 44 + i * 8)))),
      ...settings.customTargets.map((host) => summarize(`custom:${host}`, host, host, "custom", histories[1].map((v) => (v === null ? null : v + 31)))),
    ];
    const spikeNow = params.get("spike") !== null ? { start: Date.now() - 140_000, end: null, peak: 837, source: "other" as const } : null;
    const spikes = [{ start: Date.now() - 3 * HOUR, end: Date.now() - 3 * HOUR + 240_000, peak: 640, source: "other" as const }];
    return { condition, method: "icmp", outage, recent, spike: spikeNow, spikes, targets: [...targets, ...extra] };
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
    async hours(days) {
      const now = Date.now();
      const first = Math.floor((now - days * 86_400_000) / HOUR) * HOUR;
      const list = [];
      let seed = 5;
      const random = () => (seed = (seed * 9301 + 49297) % 233280) / 233280;
      for (let at = first; at <= now; at += HOUR) {
        const hour = new Date(at).getHours();
        const evening = hour >= 21 && hour <= 23 ? 22 + random() * 30 : 0;
        const down = at === yesterday.getTime() - (yesterday.getTime() % HOUR) ? 180 : random() < 0.01 ? 240 : 0;
        list.push({ at, ping: Math.round(17 + random() * 8 + evening), loss: evening ? random() * 3 : 0, down, samples: 60 });
      }
      return list;
    },
    async gameRegions() {
      return [
        { code: "ams", name: "Amsterdam (Netherlands)", ip: "155.133.248.36" },
        { code: "dxb", name: "Dubai (United Arab Emirates)", ip: "185.25.183.163" },
        { code: "fra", name: "Frankfurt (Germany)", ip: "155.133.226.68" },
        { code: "lhr", name: "London (England)", ip: "162.254.196.66" },
        { code: "vie", name: "Vienna (Austria)", ip: "146.66.155.66" },
        { code: "waw", name: "Warsaw (Poland)", ip: "155.133.230.100" },
      ];
    },
    setGameTargets: (gameTargets) => update({ gameTargets }),
    setCustomTargets: (customTargets) => update({ customTargets }),
    async history(minutes) {
      return minutesFor(minutes);
    },
    async openFreedpi() {
      return params.get("freedpi") !== null;
    },
    async openFreedpiRepository() {
      window.open("https://github.com/hasanerman/FreeDPI", "_blank");
    },
    async freedpiInstalled() {
      return params.get("freedpi") !== null;
    },
    async blockApp(name, path) {
      const next = await update({ blocked: [...settings.blocked.filter((b) => b.name !== name), { name, path }] });
      return { settings: next, warning: params.get("firewall") === "off" ? "firewall_off:Private" : null };
    },
    async unblockApp(name) {
      return update({ blocked: settings.blocked.filter((b) => b.name !== name) });
    },
    async speedTest() {
      for (let i = 1; i <= 8; i += 1) {
        await new Promise((resolve) => window.setTimeout(resolve, 250));
        speedHandlers.forEach((h) => h({ phase: i <= 5 ? "download" : "upload", mbps: i <= 5 ? 70 + i * 4 : 18 + i }));
      }
      return { download: 94.2, upload: 21.3 };
    },
    onSpeed(handler) {
      speedHandlers.push(handler);
    },
    onTraffic(handler) {
      const status = (params.get("traffic") ?? "running") as TrafficSnapshot["status"];
      const dnsQuery = [0xa1, 0x2f, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0, 7, ...Array.from("discord", (c) => c.charCodeAt(0)), 3, 99, 111, 109, 0, 0, 1, 0, 1];
      const hello = [0x16, 0x03, 0x01, 0x02, 0x00, 0x01, 0x00, 0x01, 0xfc, 0x03, 0x03, 0x5b, 0x91, 0x2e, 0x07, 0xaa, 0x13, 0x40, 0xc2, 0x77, 0x0e, 0x19, 0x84, 0x3d, 0xd1, 0x60, 0x2b, 0x91, 0x4f, 0x08, 0xe5, 0x6a, 0x33, 0x7c, 0x12, 0x99, 0x01, 0x2d, 0xbe, 0x55, 0x6e, 0x0a, 0x20];
      const base = Date.now();
      const flows: FlowView[] = [
        { id: 1, app: "Discord", protocol: "tcp", label: "TLS", remote: "162.159.135.234", remotePort: 443, localPort: 51320, name: "gateway.discord.gg", detail: "TLS ClientHello · SNI gateway.discord.gg", down: 18_400, up: 3_100, last: base, sample: hello, sampleOut: true },
        { id: 2, app: "Discord", protocol: "udp", label: "DNS", remote: "192.168.8.1", remotePort: 53, localPort: 51544, name: "discord.com", detail: "A discord.com → 162.159.137.232, 162.159.138.232", down: 94, up: 74, last: base - 1000, sample: dnsQuery, sampleOut: true },
        { id: 3, app: "Discord", protocol: "tcp", label: "TLS", remote: "162.159.130.233", remotePort: 443, localPort: 51322, name: "media.discordapp.net", detail: "TLS ClientHello · SNI media.discordapp.net", down: 1_180_000, up: 9_400, last: base - 2000, sample: hello, sampleOut: true },
        { id: 4, app: "Discord", protocol: "udp", label: "UDP", remote: "66.22.196.12", remotePort: 50002, localPort: 61001, name: null, detail: null, down: 40_200, up: 38_900, last: base - 2500, sample: [0x90, 0x78, 0x11, 0x2c, 0x00, 0x08, 0xa3, 0x10, 0x9b, 0x2e, 0x55, 0x01], sampleOut: false },
        { id: 5, app: "chrome", protocol: "udp", label: "QUIC", remote: "142.250.187.110", remotePort: 443, localPort: 60211, name: "www.youtube.com", detail: null, down: 412_000, up: 22_000, last: base - 3000, sample: [0xc3, 0x00, 0x00, 0x00, 0x01, 0x08, 0x5e, 0x11], sampleOut: true },
        { id: 6, app: "steam", protocol: "tcp", label: "HTTP", remote: "23.45.110.8", remotePort: 80, localPort: 50888, name: "cdn.steamstatic.com", detail: "GET /steam/apps/730/header.jpg HTTP/1.1", down: 96_000, up: 410, last: base - 4500, sample: Array.from("GET /steam/apps/730/header.jpg HTTP/1.1\r\nHost: cdn.steamstatic.com", (c) => c.charCodeAt(0)).slice(0, 64), sampleOut: true },
      ];
      const emit = () =>
        handler({
          status,
          averageDown: 610_000,
          averageUp: 42_000,
          down: 1_400_000 + Math.round(Math.random() * 200_000),
          up: 80_000,
          apps: [
            { name: "Discord", path: "C:\\Users\\you\\AppData\\Local\\Discord\\Discord.exe", down: 1_200_000, up: 52_000, totalDown: 412_000_000, totalUp: 31_000_000 },
            { name: "chrome", path: "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe", down: 410_000, up: 20_000, totalDown: 1_200_000_000, totalUp: 64_000_000 },
            { name: "steam", path: "C:\\Program Files (x86)\\Steam\\steam.exe", down: 96_000, up: 2_000, totalDown: 2_400_000_000, totalUp: 9_000_000 },
            { name: "svchost", path: "C:\\Windows\\System32\\svchost.exe", down: 4_000, up: 1_000, totalDown: 12_000_000, totalUp: 4_000_000 },
            { name: "Spotify", path: null, down: 2_000, up: 400, totalDown: 88_000_000, totalUp: 1_000_000 },
          ],
          flows: flows.map((f) => ({ ...f, last: f.id === 1 ? Date.now() : f.last })),
        });
      emit();
      window.setInterval(emit, 1000);
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
