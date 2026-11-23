import { strings } from "./i18n";
import type { AppView, Backend, FlowView, Settings, TrafficSnapshot } from "./types";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

export function bytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(value < 10 * 1024 ? 1 : 0)} KB`;
  if (value < 1024 ** 3) return `${(value / 1024 / 1024).toFixed(1)} MB`;
  return `${(value / 1024 ** 3).toFixed(2)} GB`;
}

function rate(value: number): string {
  return `${bytes(value)}/s`;
}

function mbps(value: number): string {
  return ((value * 8) / 1_000_000).toFixed(1);
}

export function hexdump(data: number[]): string {
  const lines: string[] = [];
  for (let offset = 0; offset < data.length; offset += 16) {
    const chunk = data.slice(offset, offset + 16);
    const hex = chunk.map((b) => b.toString(16).padStart(2, "0"));
    const text = chunk.map((b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : ".")).join("");
    lines.push(`${offset.toString(16).padStart(2, "0")}  ${hex.join(" ").padEnd(47)}  ${text}`);
  }
  return lines.join("\n");
}

export class TrafficView {
  private latest: TrafficSnapshot | null = null;
  private app: string | null = null;
  private selected: number | null = null;
  private visible = false;
  private busy = false;
  private error: string | null = null;

  constructor(
    private backend: Backend,
    private settings: () => Settings,
    private setSettings: (next: Settings) => void,
  ) {}

  start() {
    this.backend.onTraffic((snapshot) => {
      this.latest = snapshot;
      if (this.visible) this.render();
    });
  }

  show(visible: boolean) {
    this.visible = visible;
    if (visible) this.render();
  }

  render() {
    const t = strings(this.settings().language);
    $("lbl-traffic").textContent = t.traffic;
    const snapshot = this.latest;
    const status = snapshot?.status ?? "starting";
    const message = $("traffic-status");
    const body = $("traffic-body");
    const blocked = status === "needsAdmin" || status === "unsupported" || (status !== "running" && !snapshot?.flows.length);
    message.hidden = !blocked;
    body.hidden = blocked;
    message.textContent = t.trafficStatus[status];
    const running = snapshot && status === "running";
    $("traffic-rate").textContent = running ? `↓ ${mbps(snapshot.down)}  ↑ ${mbps(snapshot.up)} Mbps` : "";
    $("traffic-average").textContent = running ? `${t.average} ↓ ${mbps(snapshot.averageDown)}  ↑ ${mbps(snapshot.averageUp)}` : "";
    if (!snapshot || blocked) return;

    const apps = $("traffic-apps");
    const top = Math.max(1, ...snapshot.apps.map((a) => a.down + a.up));
    const all = this.appRow(t.allApps, rate(snapshot.down + snapshot.up), null, this.app === null);
    const blockedApps = this.settings().blocked;
    const listed: AppView[] = [
      ...snapshot.apps,
      ...blockedApps
        .filter((b) => !snapshot.apps.some((a) => a.name === b.name))
        .map((b) => ({ name: b.name, path: b.path, down: 0, up: 0, totalDown: 0, totalUp: 0 })),
    ];
    apps.replaceChildren(
      all,
      ...listed.map((a) => {
        const isBlocked = blockedApps.some((b) => b.name === a.name);
        const row = this.appRow(a.name, isBlocked ? t.blockedShort : rate(a.down + a.up), (a.down + a.up) / top, this.app === a.name);
        if (isBlocked) row.classList.add("blocked");
        row.title = `↓ ${bytes(a.totalDown)}  ↑ ${bytes(a.totalUp)}`;
        return row;
      }),
    );
    this.renderBlockBar(listed.find((a) => a.name === this.app) ?? null);

    const flows = snapshot.flows.filter((f) => this.app === null || f.app === this.app);
    const list = $("traffic-flows");
    if (!flows.length) {
      const empty = document.createElement("p");
      empty.className = "empty";
      empty.textContent = t.noFlows;
      list.replaceChildren(empty);
    } else {
      list.replaceChildren(...flows.map((f) => this.flowRow(f)));
    }
    this.renderInspector(flows.find((f) => f.id === this.selected) ?? null);
  }

  private renderBlockBar(app: AppView | null) {
    const t = strings(this.settings().language);
    const bar = $("block-bar");
    bar.hidden = !app;
    if (!app) return;
    const blocked = this.settings().blocked.some((b) => b.name === app.name);
    const text = document.createElement("span");
    text.className = "block-text";
    text.textContent = this.error ?? (blocked ? t.blockedNote(app.name) : app.path ?? t.noPath);
    const button = document.createElement("button");
    button.type = "button";
    button.className = blocked ? "btn quiet" : "btn danger";
    button.textContent = this.busy ? t.working : blocked ? t.allow : t.block;
    button.disabled = this.busy || (!blocked && !app.path);
    button.onclick = () => void this.toggle(app, blocked);
    bar.replaceChildren(text, button);
  }

  private async toggle(app: AppView, blocked: boolean) {
    const t = strings(this.settings().language);
    this.busy = true;
    this.error = null;
    this.render();
    try {
      const next = blocked ? await this.backend.unblockApp(app.name) : await this.backend.blockApp(app.name, app.path ?? "");
      this.setSettings(next);
    } catch (err) {
      this.error = String(err).includes("needs_admin") ? t.blockNeedsAdmin : String(err).includes("unsupported") ? t.blockUnsupported : t.blockFailed;
    }
    this.busy = false;
    this.render();
  }

  private appRow(name: string, value: string, share: number | null, active: boolean) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = active ? "app-row on" : "app-row";
    const label = document.createElement("span");
    label.className = "app-name";
    label.textContent = name;
    if (share !== null) {
      const bar = document.createElement("i");
      bar.className = "app-bar";
      const fill = document.createElement("b");
      fill.style.width = `${Math.round(share * 100)}%`;
      bar.append(fill);
      label.append(bar);
    }
    const amount = document.createElement("span");
    amount.className = "num";
    amount.textContent = value;
    row.append(label, amount);
    row.onclick = () => {
      this.app = share === null ? null : name;
      this.selected = null;
      this.render();
    };
    return row;
  }

  private flowRow(flow: FlowView) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = flow.id === this.selected ? "flow-row on" : "flow-row";
    const time = document.createElement("em");
    time.className = "num";
    time.textContent = new Date(flow.last).toLocaleTimeString(this.settings().language, { hour12: false });
    const name = document.createElement("span");
    name.className = "flow-name";
    name.textContent = flow.name ?? flow.remote;
    const label = document.createElement("span");
    label.className = "flow-label";
    label.textContent = flow.label;
    const size = document.createElement("span");
    size.className = "num";
    size.textContent = flow.down >= flow.up ? `↓ ${bytes(flow.down)}` : `↑ ${bytes(flow.up)}`;
    row.append(time, name, label, size);
    row.onclick = () => {
      this.selected = this.selected === flow.id ? null : flow.id;
      this.render();
    };
    return row;
  }

  private renderInspector(flow: FlowView | null) {
    const t = strings(this.settings().language);
    const panel = $("traffic-inspect");
    panel.hidden = !flow;
    if (!flow) return;
    const head = document.createElement("p");
    head.className = "inspect-head";
    const app = document.createElement("b");
    app.textContent = flow.app;
    head.append(app, ` ${flow.protocol.toUpperCase()} ${flow.localPort} → ${flow.remote}:${flow.remotePort} · ↓ ${bytes(flow.down)} ↑ ${bytes(flow.up)}`);
    const detail = document.createElement("p");
    detail.className = "inspect-detail";
    const encrypted = flow.label === "QUIC" || flow.label.startsWith("TLS");
    detail.textContent = [flow.detail, encrypted ? t.encrypted : null].filter(Boolean).join(" · ");
    const caption = document.createElement("p");
    caption.className = "inspect-caption";
    caption.textContent = flow.sample.length ? (flow.sampleOut ? t.firstSent : t.firstReceived) : t.noPayload;
    const dump = document.createElement("pre");
    dump.textContent = hexdump(flow.sample);
    panel.replaceChildren(head, detail, caption, ...(flow.sample.length ? [dump] : []));
  }
}
