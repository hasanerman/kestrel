import "@fontsource/geist-sans/400.css";
import "@fontsource/geist-sans/500.css";
import "@fontsource/geist-sans/600.css";
import "@fontsource/geist-mono/500.css";
import "./styles.css";

import { createBackend } from "./api";
import { drawGraph } from "./graph";
import { LANGUAGES, strings } from "./i18n";
import type { Backend, Settings, Snapshot, TargetView, Theme } from "./types";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

let backend: Backend;
let settings: Settings;
let latest: Snapshot | null = null;

const THEMES: Theme[] = ["system", "light", "dark"];

function text(id: string, value: string) {
  $(id).textContent = value;
}

function css(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function best(snapshot: Snapshot): TargetView | undefined {
  return snapshot.targets
    .filter((t) => !t.failing && t.last !== null)
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
      name.textContent = target.name;
      const host = document.createElement("em");
      host.textContent = target.host;
      name.append(host);
      const value = document.createElement("span");
      value.className = `num ${pingClass(target.last)}`;
      value.textContent = target.last !== null ? String(target.last) : t.noAnswer;
      row.append(name, value);
      return row;
    }),
  );

  const graphed = snapshot.targets[0];
  const color = condition === "down" ? css("--red") : condition === "degraded" ? css("--amber") : css("--green");
  if (graphed) drawGraph($<HTMLCanvasElement>("graph"), graphed.history, color);
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
  text("lbl-sites", t.sites);
  text("lbl-sites-later", t.sitesLater);
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
  $<HTMLInputElement>("chk-notifications").checked = settings.notifications;
  $<HTMLInputElement>("chk-autostart").checked = settings.autostart;

  renderOptions("opt-language", LANGUAGES, settings.language, async (id) => {
    settings = await backend.setLanguage(id);
    renderSettings();
    renderHealth();
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
  text("version", `kestrel ${init.version}`);

  document.querySelectorAll<HTMLElement>(".tab").forEach((tab) => (tab.onclick = () => showPage(tab.dataset.page ?? "health")));
  $("btn-minimize").onclick = () => void backend.minimize();
  $("btn-hide").onclick = () => void backend.hide();
  $("btn-quit").onclick = () => void backend.quit();
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

  backend.onHealth((snapshot) => {
    latest = snapshot;
    renderHealth();
  });
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", renderHealth);
  window.addEventListener("resize", renderHealth);

  renderSettings();
  renderHealth();
  const page = new URLSearchParams(location.search).get("page");
  if (page) showPage(page);
}

void start();
