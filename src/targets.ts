import { strings } from "./i18n";
import type { Backend, GameRegion, Settings } from "./types";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const RETRY_MS = 30_000;

export class TargetsView {
  private regions: GameRegion[] = [];

  constructor(
    private backend: Backend,
    private settings: () => Settings,
    private setSettings: (next: Settings) => void,
  ) {}

  async start() {
    $("btn-add-game").onclick = () => void this.addGame();
    $<HTMLFormElement>("custom-add").onsubmit = (event) => {
      event.preventDefault();
      void this.addCustom();
    };
    await this.loadRegions();
  }

  private async loadRegions() {
    this.regions = await this.backend.gameRegions().catch(() => []);
    this.render();
    if (!this.regions.length) window.setTimeout(() => void this.loadRegions(), RETRY_MS);
  }

  private async addGame() {
    const code = $<HTMLSelectElement>("game-select").value;
    if (!code) return;
    this.setSettings(await this.backend.setGameTargets([...this.settings().gameTargets, code]));
    this.render();
  }

  private async addCustom() {
    const input = $<HTMLInputElement>("custom-input");
    const entries = input.value.split(/[\s,;]+/).filter(Boolean);
    if (!entries.length) return;
    this.setSettings(await this.backend.setCustomTargets([...this.settings().customTargets, ...entries]));
    input.value = "";
    this.render();
  }

  private async remove(kind: "game" | "custom", value: string) {
    const settings = this.settings();
    const next =
      kind === "game"
        ? await this.backend.setGameTargets(settings.gameTargets.filter((c) => c !== value))
        : await this.backend.setCustomTargets(settings.customTargets.filter((h) => h !== value));
    this.setSettings(next);
    this.render();
  }

  private regionName(code: string) {
    const region = this.regions.find((r) => r.code === code);
    return region ? `CS2 ${region.name.split(" (")[0]}` : code;
  }

  render() {
    const t = strings(this.settings().language);
    const settings = this.settings();
    $("lbl-targets").textContent = t.targets;
    $("lbl-targets-hint").textContent = t.targetsHint;
    const total = settings.gameTargets.length + settings.customTargets.length;
    $("val-targets").textContent = total ? String(total) : t.off;
    $<HTMLInputElement>("custom-input").placeholder = t.customPlaceholder;

    const select = $<HTMLSelectElement>("game-select");
    const first = document.createElement("option");
    first.value = "";
    first.textContent = this.regions.length ? t.pickGame : t.listLoading;
    const options = this.regions
      .filter((r) => !settings.gameTargets.includes(r.code))
      .map((r) => {
        const option = document.createElement("option");
        option.value = r.code;
        option.textContent = r.name;
        return option;
      });
    select.replaceChildren(first, ...options);
    select.disabled = !this.regions.length;

    const rows = [
      ...settings.gameTargets.map((code) => this.row(this.regionName(code), () => this.remove("game", code))),
      ...settings.customTargets.map((host) => this.row(host, () => this.remove("custom", host))),
    ];
    const list = $("target-list");
    if (rows.length) {
      list.replaceChildren(...rows);
    } else {
      const empty = document.createElement("p");
      empty.className = "hint";
      empty.textContent = t.noTargets;
      list.replaceChildren(empty);
    }
  }

  private row(label: string, remove: () => void) {
    const row = document.createElement("div");
    row.className = "target-row";
    const name = document.createElement("span");
    name.textContent = label;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "remove";
    button.textContent = "×";
    button.onclick = remove;
    row.append(name, button);
    return row;
  }
}
