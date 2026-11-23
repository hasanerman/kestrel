import { strings } from "./i18n";
import type { Backend, Category, Settings, SiteResult } from "./types";

type Filter = "all" | "mine" | string;

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

export class SitesView {
  private categories: Category[] = [];
  private results = new Map<string, SiteResult>();
  private pending = new Set<string>();
  private filter: Filter = "all";
  private open = new Set<string>();
  private solving = new Set<string>();
  private freedpi = false;

  constructor(
    private backend: Backend,
    private settings: () => Settings,
    private setSettings: (next: Settings) => void,
    private version: string,
  ) {}

  async start() {
    this.categories = await this.backend.siteLists();
    this.freedpi = await this.backend.freedpiInstalled().catch(() => false);
    this.backend.onSite((result) => {
      this.results.set(result.host, result);
      this.pending.delete(result.host);
      this.render();
    });
    $("btn-check").onclick = () => void this.check();
    $("btn-copy-report").onclick = () => void this.copy();
    $<HTMLFormElement>("site-add").onsubmit = (event) => {
      event.preventDefault();
      void this.add();
    };
    this.render();
  }

  private hosts(filter: Filter = this.filter): string[] {
    const mine = this.settings().sites;
    if (filter === "mine") return mine;
    if (filter === "all") return [...this.categories.flatMap((c) => c.hosts), ...mine.filter((h) => !this.categories.some((c) => c.hosts.includes(h)))];
    return this.categories.find((c) => c.id === filter)?.hosts ?? [];
  }

  private async check() {
    const hosts = this.hosts();
    if (!hosts.length || this.pending.size) return;
    hosts.forEach((host) => {
      this.pending.add(host);
      this.results.delete(host);
    });
    this.render();
    await this.backend.checkSites(hosts);
    this.pending.clear();
    this.render();
  }

  private async add() {
    const input = $<HTMLInputElement>("site-input");
    const entries = input.value.split(/[\s,;]+/).filter(Boolean);
    if (!entries.length) return;
    const next = await this.backend.setSites([...this.settings().sites, ...entries]);
    this.setSettings(next);
    input.value = "";
    this.render();
  }

  private async remove(host: string) {
    const next = await this.backend.setSites(this.settings().sites.filter((h) => h !== host));
    this.setSettings(next);
    this.results.delete(host);
    this.render();
  }

  steps(result: SiteResult): string {
    const t = strings(this.settings().language);
    const parts: string[] = [];
    if (result.verdict === "noInternet") return t.verdicts.noInternet;
    if (result.dns !== null) parts.push(`DNS ${result.dns} ms`);
    if (result.tcp !== null) parts.push(`TCP ${result.tcp} ms`);
    if (result.https !== null) parts.push(`HTTPS ${result.status ?? ""} · ${result.https} ms`);
    if (result.failure) {
      const stage = result.dns === null ? "DNS" : result.tcp === null ? "TCP" : "HTTPS";
      parts.push(result.failure === "dns" || result.failure === "blockPage" ? t.failures[result.failure] : `${stage} ${t.failures[result.failure]}`);
    }
    if (result.dnsDiffers) parts.push(t.dnsDiffers);
    return parts.join(" · ");
  }

  private verdictClass(result: SiteResult) {
    if (result.verdict === "works") return "ok";
    if (result.verdict === "down" || result.verdict === "unreachable" || result.verdict === "noInternet") return "warn";
    return "bad";
  }

  render() {
    const t = strings(this.settings().language);
    $("lbl-sites").textContent = t.sites;
    $("btn-check").textContent = this.pending.size ? t.checking : t.check;
    $<HTMLButtonElement>("btn-check").disabled = this.pending.size > 0;
    $("btn-copy-report").textContent = t.copyReport;
    $<HTMLInputElement>("site-input").placeholder = t.addSite;
    $("site-add").hidden = this.filter !== "mine";

    const filters: { id: Filter; name: string }[] = [
      { id: "all", name: t.categories.all },
      ...this.categories.map((c) => ({ id: c.id, name: t.categories[c.id as keyof typeof t.categories] ?? c.id })),
      { id: "mine", name: t.categories.mine },
    ];
    $("site-filters").replaceChildren(
      ...filters.map((f) => {
        const button = document.createElement("button");
        button.type = "button";
        button.className = f.id === this.filter ? "on" : "";
        button.textContent = f.name;
        button.onclick = () => {
          this.filter = f.id;
          this.render();
        };
        return button;
      }),
    );

    const hosts = this.hosts();
    const list = $("site-list");
    if (this.filter === "mine" && !hosts.length) {
      const empty = document.createElement("p");
      empty.className = "empty";
      empty.textContent = t.noSites;
      list.replaceChildren(empty);
    } else {
      list.replaceChildren(...hosts.map((host) => this.row(host)));
    }

    const done = hosts.map((h) => this.results.get(h)).filter((r): r is SiteResult => !!r);
    const failing = done.filter((r) => r.verdict !== "works").length;
    $("site-summary").textContent = done.length ? t.siteSummary(done.length, failing) : t.notChecked;
    $<HTMLButtonElement>("btn-copy-report").disabled = !done.length;
  }

  private row(host: string) {
    const t = strings(this.settings().language);
    const result = this.results.get(host);
    const row = document.createElement("div");
    row.className = "site";
    const head = document.createElement("div");
    head.className = "site-head";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = host;
    const verdict = document.createElement("span");
    if (this.pending.has(host)) {
      verdict.className = "verdict pending";
      verdict.textContent = t.checking;
    } else if (result) {
      verdict.className = `verdict ${this.verdictClass(result)}`;
      verdict.textContent = t.verdicts[result.verdict];
    } else {
      verdict.className = "verdict idle";
      verdict.textContent = "–";
    }
    head.append(name, verdict);
    const blocked = result && (result.verdict === "dnsBlocked" || result.verdict === "dpiBlocked");
    if (this.filter === "mine") {
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "remove";
      remove.textContent = "×";
      remove.onclick = (event) => {
        event.stopPropagation();
        void this.remove(host);
      };
      head.append(remove);
    }
    row.append(head);
    if (result) {
      row.classList.add("has-steps");
      row.onclick = () => {
        if (this.open.has(host)) this.open.delete(host);
        else this.open.add(host);
        this.render();
      };
      if (this.open.has(host) || result.verdict !== "works") {
        const line = document.createElement("div");
        line.className = "steps-row";
        const steps = document.createElement("p");
        steps.className = "steps";
        steps.textContent = this.steps(result);
        line.append(steps);
        if (blocked) {
          const toggle = document.createElement("button");
          toggle.type = "button";
          toggle.className = this.solving.has(host) ? "solve-toggle on" : "solve-toggle";
          toggle.title = this.solving.has(host) ? t.hideSolve : t.howToSolve;
          toggle.setAttribute("aria-label", toggle.title);
          toggle.innerHTML =
            '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" /></svg>';
          toggle.onclick = (event) => {
            event.stopPropagation();
            if (this.solving.has(host)) this.solving.delete(host);
            else this.solving.add(host);
            this.render();
          };
          line.append(toggle);
        }
        row.append(line);
      }
      if (blocked && this.solving.has(host)) {
        row.append(this.solvePanel(host, result.verdict === "dnsBlocked"));
      }
    }
    return row;
  }

  private solvePanel(host: string, dns: boolean) {
    const t = strings(this.settings().language);
    const panel = document.createElement("div");
    panel.className = "solve";
    panel.onclick = (event) => event.stopPropagation();
    const why = document.createElement("p");
    why.textContent = dns ? t.solveDns : t.solveDpi;
    const how = document.createElement("p");
    how.className = "solve-how";
    how.textContent = this.freedpi ? t.solveWithFreedpi(host) : t.solveGetFreedpi;
    const actions = document.createElement("div");
    actions.className = "solve-actions";
    const primary = document.createElement("button");
    primary.type = "button";
    primary.className = "btn";
    primary.textContent = this.freedpi ? t.openInFreedpi : t.downloadFreedpi;
    primary.onclick = async () => {
      try {
        const launched = await this.backend.openFreedpi(host);
        primary.textContent = launched ? t.freedpiOpened : t.freedpiDownload;
      } catch {
        primary.textContent = t.copyFailed;
      }
    };
    const repository = document.createElement("button");
    repository.type = "button";
    repository.className = "btn quiet";
    repository.textContent = t.viewOnGithub;
    repository.title = "github.com/hasanerman/FreeDPI";
    repository.onclick = () => void this.backend.openFreedpiRepository();
    actions.append(primary, repository);
    panel.append(why, how, actions);
    return panel;
  }

  private async copy() {
    const t = strings(this.settings().language);
    const lines = this.hosts()
      .map((h) => this.results.get(h))
      .filter((r): r is SiteResult => !!r)
      .map((r) => `${r.host}  ${t.verdicts[r.verdict]}  ${this.steps(r)}`);
    const text = [`kestrel ${this.version} · ${new Date().toISOString().slice(0, 16).replace("T", " ")}`, "", ...lines].join("\n");
    try {
      await navigator.clipboard.writeText(text);
      $("btn-copy-report").textContent = t.copied;
      window.setTimeout(() => this.render(), 1500);
    } catch {
      $("btn-copy-report").textContent = t.copyFailed;
    }
  }
}
