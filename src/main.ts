import "@fontsource/geist-sans/400.css";
import "@fontsource/geist-sans/500.css";
import "@fontsource/geist-sans/600.css";
import "@fontsource/geist-mono/500.css";
import "./styles.css";

import { createBackend } from "./api";
import { drawGraph } from "./graph";
import { LANGUAGES, strings } from "./i18n";
import { SitesView } from "./sites";
import type { Backend, Minute, Outage, Range, Settings, Snapshot, TargetView, Theme, UpdateInfo } from "./types";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

let backend: Backend;
let settings: Settings;
let latest: Snapshot | null = null;
let sites: SitesView | null = null;
let update: UpdateInfo | null = null;
let updating = false;
let range: Range = 5;
let minutes: Minute[] = [];

const RANGES: Range[] = [5, 60, 1440];
const LIVE_SLOTS = 300;

const THEMES: Theme[] = ["system", "light", "dark"];
const RETENTION = [7, 30, 90];

function text(id: string, value: string) {
  $(id).textContent = value;
}

function css(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function best(snapshot: Snapshot): TargetView | undefined {
  return snapshot.targets
    .filter((t) => t.kind === "internet" && !t.failing && t.last !== null)
    .sort((a, b) => (a.last ?? 0) - (b.last ?? 0))[0];
}

function pingClass(value: number | null) {
  if (value === null) return "bad";
  return value > 150 ? "bad" : value > 60 ? "warn" : "ok";
}

function renderHealth() {
  const t = strings(settings.language);
  const snapshot = latest;
  const condition = snapshot?.condition ?? "starting";
  document.body.dataset.condition = condition;
  text("condition", t.condition[condition]);
  if (!snapshot) return;

  const top = best(snapshot);
  text("ping", top?.last != null ? String(top.last) : "–");
  text("jitter", top?.jitter != null ? String(top.jitter) : "–");
  text("loss", top ? top.loss.toFixed(1) : "–");

  const rows = $("targets");
  rows.replaceChildren(
    ...snapshot.targets.map((target) => {
      const row = document.createElement("div");
      row.className = "row";
      const name = document.createElement("span");
      name.className = "name";
      name.textContent = target.kind === "router" ? t.router : target.name;
      const host = document.createElement("em");
      host.textContent = target.host;
      name.append(host);
      const value = document.createElement("span");
      value.className = target.last !== null ? `num ${pingClass(target.last)}` : "bad";
      value.textContent = target.last !== null ? String(target.last) : t.noAnswer;
      row.append(name, value);
      return row;
    }),
  );

  const color = condition === "down" ? css("--red") : condition === "degraded" ? css("--amber") : css("--green");
  const canvas = $<HTMLCanvasElement>("graph");
  if (range === 5) {
    const graphed = snapshot.targets.find((target) => target.kind === "internet");
    if (graphed) drawGraph(canvas, graphed.history, color, LIVE_SLOTS);
  } else {
    drawGraph(canvas, minutes.map((m) => m.ping), color, range);
  }
  renderFoot(snapshot);
}

function clock(at: number) {
  return new Date(at).toLocaleTimeString(settings.language, { hour: "2-digit", minute: "2-digit" });
}

function when(at: number) {
  const t = strings(settings.language);
  const day = new Date(at);
  const today = new Date();
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();
  const prefix = sameDay(day, today)
    ? t.today
    : sameDay(day, yesterday)
      ? t.yesterday
      : day.toLocaleDateString(settings.language, { day: "numeric", month: "short" });
  return `${prefix} ${clock(at)}`;
}

function duration(outage: Outage) {
  const t = strings(settings.language);
  const seconds = Math.max(1, Math.round(((outage.end ?? Date.now()) - outage.start) / 1000));
  if (seconds < 60) return `${seconds} ${t.units.s}`;
  if (seconds < 3600) return `${Math.round(seconds / 60)} ${t.units.min}`;
  return `${Math.floor(seconds / 3600)} ${t.units.h} ${Math.round((seconds % 3600) / 60)} ${t.units.min}`;
}

function renderFoot(snapshot: Snapshot) {
  const t = strings(settings.language);
  const foot = $("foot");
  const strong = (value: string) => {
    const b = document.createElement("b");
    b.textContent = value;
    return b;
  };
  foot.classList.toggle("down", snapshot.outage !== null);
  if (snapshot.outage) {
    foot.replaceChildren(`${t.downSince} `, strong(clock(snapshot.outage.start)), ` · ${t.causes[snapshot.outage.cause]} · ${duration(snapshot.outage)}`);
    return;
  }
  const last = snapshot.recent[0];
  if (!last) {
    foot.replaceChildren(t.noDrops);
    return;
  }
  foot.replaceChildren(`${t.lastDrop} `, strong(when(last.start)), ` · ${t.causes[last.cause]} · ${duration(last)}`);
}

async function loadMinutes() {
  if (range === 5) return;
  minutes = await backend.history(range);
  renderHealth();
}

function renderRange() {
  const t = strings(settings.language);
  $("range").replaceChildren(
    ...RANGES.map((value) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = value === range ? "on" : "";
      button.textContent = t.ranges[value];
      button.onclick = () => {
        range = value;
        renderRange();
        renderHealth();
        void loadMinutes();
      };
      return button;
    }),
  );
}

function renderOptions<T extends string>(host: string, options: { id: T; name: string }[], current: string, pick: (id: T) => void) {
  $(host).replaceChildren(
    ...options.map((option) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = option.id === current ? "opt on" : "opt";
      button.textContent = option.name;
      button.onclick = () => pick(option.id);
      return button;
    }),
  );
}

function renderSettings() {
  const t = strings(settings.language);
  document.documentElement.lang = settings.language;
  if (settings.theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = settings.theme;

  text("lbl-jitter", t.jitter);
  text("lbl-loss", t.loss);
  text("lbl-apps", t.apps);
  text("lbl-apps-later", t.appsLater);
  text("lbl-settings", t.settings);
  text("lbl-language", t.language);
  text("lbl-language-hint", t.languageHint);
  text("val-language", LANGUAGES.find((l) => l.id === settings.language)?.name ?? "");
  text("lbl-theme", t.theme);
  text("lbl-theme-hint", t.themeHint);
  text("val-theme", t.themes[settings.theme]);
  text("lbl-notifications", t.notifications);
  text("lbl-notifications-toggle", t.notificationsToggle);
  text("lbl-notifications-hint", t.notificationsHint);
  text("val-notifications", settings.notifications ? t.on : t.off);
  text("lbl-autostart", t.autostart);
  text("lbl-autostart-toggle", t.autostartToggle);
  text("lbl-autostart-hint", t.autostartHint);
  text("val-autostart", settings.autostart ? t.on : t.off);
  text("btn-quit", t.quit);
  text("lbl-data", t.data);
  text("lbl-data-hint", t.dataHint);
  text("val-data", t.days(settings.retentionDays));
  text("lbl-help", t.help);
  text("lbl-help-hint", t.helpHint);
  text("btn-diagnostics", t.copyDiagnostics);
  text("btn-report", t.reportIssue);
  renderOptions(
    "opt-data",
    RETENTION.map((days) => ({ id: String(days), name: t.days(days) })),
    String(settings.retentionDays),
    async (id) => {
      settings = await backend.setRetention(Number(id));
      renderSettings();
    },
  );
  renderUpdate();
  renderRange();
  $<HTMLInputElement>("chk-notifications").checked = settings.notifications;
  $<HTMLInputElement>("chk-autostart").checked = settings.autostart;

  renderOptions("opt-language", LANGUAGES, settings.language, async (id) => {
    settings = await backend.setLanguage(id);
    renderSettings();
    renderHealth();
    sites?.render();
  });
  renderOptions(
    "opt-theme",
    THEMES.map((id) => ({ id, name: t.themes[id] })),
    settings.theme,
    async (id) => {
      settings = await backend.setTheme(id);
      renderSettings();
      renderHealth();
    },
  );
}

function renderUpdate() {
  const t = strings(settings.language);
  $("update").hidden = !update;
  if (!update) return;
  text("update-text", t.updateReady(update.version));
  text("btn-update", updating ? t.updateInstalling : t.updateInstall);
  $<HTMLButtonElement>("btn-update").disabled = updating;
  text("btn-notes", t.updateNotes);
  $("btn-notes").hidden = !update.notes;
  text("update-notes", update.notes ?? "");
}

async function installUpdate() {
  if (updating) return;
  updating = true;
  renderUpdate();
  try {
    await backend.installUpdate();
  } catch {
    updating = false;
    renderUpdate();
    text("update-text", strings(settings.language).updateFailed);
  }
}

function showPage(page: string) {
  document.querySelectorAll<HTMLElement>(".page").forEach((p) => p.classList.toggle("on", p.id === `page-${page}`));
  document.querySelectorAll<HTMLElement>(".tab").forEach((b) => b.classList.toggle("on", b.dataset.page === page));
  if (page === "health") renderHealth();
}

async function start() {
  backend = await createBackend();
  const init = await backend.init(navigator.language);
  settings = init.settings;
  latest = init.health;
  document.body.dataset.platform = init.platform;

  document.querySelectorAll<HTMLElement>(".tab").forEach((tab) => (tab.onclick = () => showPage(tab.dataset.page ?? "health")));
  $("btn-minimize").onclick = () => void backend.minimize();
  $("btn-hide").onclick = () => void backend.hide();
  $("btn-quit").onclick = () => void backend.quit();
  $("btn-source").onclick = () => void backend.openRepository();
  $("btn-report").onclick = () => void backend.reportIssue();
  $("btn-diagnostics").onclick = async () => {
    const t = strings(settings.language);
    try {
      await navigator.clipboard.writeText(await backend.diagnostics());
      text("btn-diagnostics", t.copied);
    } catch {
      text("btn-diagnostics", t.copyFailed);
    }
    window.setTimeout(() => text("btn-diagnostics", strings(settings.language).copyDiagnostics), 1500);
  };
  $<HTMLInputElement>("chk-notifications").onchange = async (event) => {
    settings = await backend.setNotifications((event.target as HTMLInputElement).checked);
    renderSettings();
  };
  $<HTMLInputElement>("chk-autostart").onchange = async (event) => {
    const box = event.target as HTMLInputElement;
    try {
      settings = await backend.setAutostart(box.checked);
    } catch {
      box.checked = !box.checked;
    }
    renderSettings();
  };

  update = init.update;
  backend.onUpdate((info) => {
    update = info;
    renderUpdate();
  });
  $("btn-update").onclick = () => void installUpdate();
  $("btn-notes").onclick = () => {
    $("update-notes").hidden = !$("update-notes").hidden;
  };

  backend.onHealth((snapshot) => {
    latest = snapshot;
    renderHealth();
  });
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", renderHealth);
  window.addEventListener("resize", renderHealth);
  window.setInterval(() => void loadMinutes(), 30_000);

  sites = new SitesView(
    backend,
    () => settings,
    (next) => {
      settings = next;
    },
    init.version,
  );
  await sites.start();

  renderSettings();
  renderHealth();
  const page = new URLSearchParams(location.search).get("page");
  if (page) showPage(page);
}

void start();
