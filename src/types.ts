export type Condition = "starting" | "ok" | "degraded" | "down";
export type Language = "en" | "tr" | "ru";
export type Theme = "system" | "light" | "dark";
export type Kind = "router" | "internet";
export type Cause = "provider" | "local";
export type Range = 5 | 60 | 1440;

export interface Outage {
  start: number;
  end: number | null;
  cause: Cause;
}

export interface Minute {
  at: number;
  ping: number | null;
  loss: number;
}

export interface TargetView {
  id: string;
  name: string;
  host: string;
  kind: Kind;
  last: number | null;
  jitter: number | null;
  loss: number;
  failing: boolean;
  history: (number | null)[];
}

export interface Snapshot {
  condition: Condition;
  method: "icmp" | "tcp";
  outage: Outage | null;
  recent: Outage[];
  targets: TargetView[];
}

export interface Settings {
  language: string;
  theme: Theme;
  autostart: boolean;
  notifications: boolean;
  retentionDays: number;
  debug: boolean;
}

export interface Init {
  settings: Settings;
  platform: string;
  version: string;
  health: Snapshot | null;
}

export interface Backend {
  init(language: string): Promise<Init>;
  setLanguage(language: string): Promise<Settings>;
  setTheme(theme: Theme): Promise<Settings>;
  setAutostart(enabled: boolean): Promise<Settings>;
  setNotifications(enabled: boolean): Promise<Settings>;
  history(minutes: number): Promise<Minute[]>;
  onHealth(handler: (snapshot: Snapshot) => void): void;
  minimize(): Promise<void>;
  hide(): Promise<void>;
  quit(): Promise<void>;
}
