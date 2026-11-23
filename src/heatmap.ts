import { strings } from "./i18n";
import type { Hour } from "./types";

const DAY_MS = 86_400_000;
const DAYS = 7;

export function uptime(hours: Hour[]): number | null {
  if (!hours.some((h) => h.samples > 0)) return null;
  const down = hours.reduce((sum, h) => sum + h.down, 0);
  return Math.max(0, 100 - (down * 100) / (DAYS * 86_400));
}

function level(hour: Hour | undefined): string {
  if (!hour) return "none";
  if (hour.down >= 60) return "down";
  if (hour.samples === 0) return hour.down > 0 ? "warn" : "none";
  const ping = hour.ping ?? 999;
  if (hour.loss > 5 || ping >= 70) return "bad";
  if (hour.loss > 1 || ping >= 40) return "warn";
  if (ping >= 25) return "good";
  return "great";
}

function startOfDay(at: number) {
  const day = new Date(at);
  return new Date(day.getFullYear(), day.getMonth(), day.getDate()).getTime();
}

function worstHours(hours: Hour[]): [number, number] | null {
  const byHour = Array.from({ length: 24 }, () => ({ sum: 0, count: 0, down: 0 }));
  hours.forEach((h) => {
    const slot = byHour[new Date(h.at).getHours()];
    if (h.ping !== null) {
      slot.sum += h.ping;
      slot.count += 1;
    }
    slot.down += h.down;
  });
  const averages = byHour.map((s) => (s.count ? s.sum / s.count : null));
  const known = averages.filter((v): v is number => v !== null).sort((a, b) => a - b);
  if (known.length < 12) return null;
  const median = known[Math.floor(known.length / 2)];
  const bad = averages.map((v, i) => (v !== null && v > median * 1.4 && v - median > 8) || byHour[i].down > 120);
  let best: [number, number] | null = null;
  for (let i = 0; i < 24; i += 1) {
    if (!bad[i]) continue;
    let end = i;
    while (end + 1 < 24 && bad[end + 1]) end += 1;
    if (!best || end - i > best[1] - best[0]) best = [i, end];
    i = end;
  }
  return best;
}

export function renderHeatmap(host: HTMLElement, hours: Hour[], language: string) {
  const t = strings(language);
  const byStart = new Map(hours.map((h) => [h.at, h]));
  const today = startOfDay(Date.now());
  const grid = document.createElement("div");
  grid.className = "heat-grid";
  const tip = document.createElement("div");
  tip.className = "heat-tip";
  tip.hidden = true;

  for (let d = DAYS - 1; d >= 0; d -= 1) {
    const dayStart = today - d * DAY_MS;
    const label = document.createElement("span");
    label.className = "heat-day";
    label.textContent = new Date(dayStart).toLocaleDateString(language, { weekday: "short" });
    grid.append(label);
    for (let h = 0; h < 24; h += 1) {
      const at = new Date(dayStart).setHours(h);
      const hour = byStart.get(at);
      const cell = document.createElement("i");
      cell.dataset.level = at > Date.now() ? "future" : level(hour);
      if (hour) {
        cell.onmouseenter = () => {
          const parts = [
            `${new Date(at).toLocaleDateString(language, { weekday: "short" })} ${String(h).padStart(2, "0")}:00`,
            hour.ping !== null ? `${hour.ping} ms` : t.noAnswer,
            `${hour.loss.toFixed(1)}% ${t.lossWord}`,
          ];
          if (hour.down > 0) parts.push(`${Math.max(1, Math.round(hour.down / 60))} ${t.units.min} ${t.dropWord}`);
          tip.textContent = parts.join(" · ");
          tip.hidden = false;
          const box = cell.getBoundingClientRect();
          const parent = host.getBoundingClientRect();
          tip.style.left = `${Math.min(Math.max(box.left - parent.left + box.width / 2, 90), parent.width - 90)}px`;
          tip.style.top = `${box.top - parent.top - 26}px`;
        };
        cell.onmouseleave = () => {
          tip.hidden = true;
        };
      }
      grid.append(cell);
    }
  }

  const axis = document.createElement("div");
  axis.className = "heat-axis";
  axis.append(document.createElement("span"));
  for (let h = 0; h < 24; h += 1) {
    const mark = document.createElement("span");
    mark.textContent = h % 6 === 0 ? String(h).padStart(2, "0") : "";
    axis.append(mark);
  }

  const legend = document.createElement("div");
  legend.className = "heat-legend";
  const scale = document.createElement("div");
  scale.className = "heat-scale";
  scale.append(t.good);
  ["great", "good", "warn", "bad", "down"].forEach((name) => {
    const swatch = document.createElement("i");
    swatch.dataset.level = name;
    scale.append(swatch);
  });
  scale.append(t.dropWord);
  const insight = document.createElement("span");
  const worst = worstHours(hours);
  const filled = hours.filter((h) => h.samples > 0).length;
  if (filled < 24) {
    insight.textContent = t.heatFilling;
  } else if (worst) {
    const end = (worst[1] + 1) % 24;
    insight.append(`${t.worstHours} `);
    const b = document.createElement("b");
    b.textContent = `${String(worst[0]).padStart(2, "0")}:00-${String(end).padStart(2, "0")}:00`;
    insight.append(b);
  } else {
    insight.textContent = t.noWorstHours;
  }
  legend.append(scale, insight);

  host.replaceChildren(grid, axis, legend, tip);
}
