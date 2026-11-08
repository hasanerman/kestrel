export type Condition = "starting" | "ok" | "degraded" | "down";
export type Language = "en" | "tr" | "ru";
export type Theme = "system" | "light" | "dark";

export interface TargetView {
  id: string;
  name: string;
  host: string;
  last: number | null;
  jitter: number | null;
  loss: number;
  failing: boolean;
  history: (number | null)[];
}

export interface Snapshot {
  condition: Condition;
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
  onHealth(handler: (snapshot: Snapshot) => void): void;
  minimize(): Promise<void>;
  hide(): Promise<void>;
  quit(): Promise<void>;
}
