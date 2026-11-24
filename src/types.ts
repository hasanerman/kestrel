export type Condition = "starting" | "ok" | "degraded" | "down";
export type Language = "en" | "tr" | "ru";
export type Theme = "system" | "light" | "dark";
export type Kind = "router" | "internet" | "game" | "custom";
export type SpikeSource = "other" | "own" | "unknown";

export interface Spike {
  start: number;
  end: number | null;
  peak: number;
  source: SpikeSource;
}

export interface GameRegion {
  code: string;
  name: string;
  ip: string;
}
export type Cause = "provider" | "local";
export type Range = 5 | 60 | 1440 | 10080;

export type TrafficStatus = "starting" | "running" | "needsAdmin" | "unsupported" | "failed";

export interface AppView {
  name: string;
  path: string | null;
  down: number;
  up: number;
  totalDown: number;
  totalUp: number;
}

export interface FlowView {
  id: number;
  app: string;
  protocol: "tcp" | "udp";
  label: string;
  remote: string;
  remotePort: number;
  localPort: number;
  name: string | null;
  detail: string | null;
  down: number;
  up: number;
  last: number;
  sample: number[];
  sampleOut: boolean;
}

export interface TrafficSnapshot {
  status: TrafficStatus;
  down: number;
  up: number;
  averageDown: number;
  averageUp: number;
  apps: AppView[];
  flows: FlowView[];
}

export interface Hour {
  at: number;
  ping: number | null;
  loss: number;
  down: number;
  samples: number;
}

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

export type Verdict = "works" | "dnsBlocked" | "dpiBlocked" | "certificate" | "unreachable" | "down" | "noInternet";
export type Failure = "dns" | "blockPage" | "timeout" | "reset" | "refused" | "closed" | "certificate" | "server" | "other";

export interface Category {
  id: string;
  hosts: string[];
}

export interface SiteResult {
  host: string;
  verdict: Verdict;
  dns: number | null;
  dnsDiffers: boolean;
  tcp: number | null;
  https: number | null;
  status: number | null;
  failure: Failure | null;
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
  spike: Spike | null;
  spikes: Spike[];
  targets: TargetView[];
}

export interface Settings {
  language: string;
  theme: Theme;
  autostart: boolean;
  notifications: boolean;
  retentionDays: number;
  debug: boolean;
  sites: string[];
  blocked: BlockedApp[];
  gameTargets: string[];
  customTargets: string[];
}

export interface BlockOutcome {
  settings: Settings;
  warning: string | null;
}

export interface BlockedApp {
  name: string;
  path: string;
}

export interface SpeedResult {
  download: number;
  upload: number;
}

export interface SpeedProgress {
  phase: "download" | "upload";
  mbps: number;
}

export interface UpdateInfo {
  version: string;
  notes: string | null;
}

export interface Init {
  settings: Settings;
  platform: string;
  version: string;
  health: Snapshot | null;
  update: UpdateInfo | null;
}

export interface Backend {
  init(language: string): Promise<Init>;
  setLanguage(language: string): Promise<Settings>;
  setTheme(theme: Theme): Promise<Settings>;
  setAutostart(enabled: boolean): Promise<Settings>;
  setNotifications(enabled: boolean): Promise<Settings>;
  history(minutes: number): Promise<Minute[]>;
  gameRegions(): Promise<GameRegion[]>;
  setGameTargets(codes: string[]): Promise<Settings>;
  setCustomTargets(hosts: string[]): Promise<Settings>;
  hours(days: number): Promise<Hour[]>;
  siteLists(): Promise<Category[]>;
  checkSites(hosts: string[]): Promise<SiteResult[]>;
  setSites(sites: string[]): Promise<Settings>;
  onSite(handler: (result: SiteResult) => void): void;
  openFreedpi(host?: string): Promise<boolean>;
  freedpiInstalled(): Promise<boolean>;
  openFreedpiRepository(): Promise<void>;
  blockApp(name: string, path: string): Promise<BlockOutcome>;
  unblockApp(name: string): Promise<Settings>;
  speedTest(): Promise<SpeedResult>;
  onSpeed(handler: (progress: SpeedProgress) => void): void;
  onTraffic(handler: (snapshot: TrafficSnapshot) => void): void;
  onHealth(handler: (snapshot: Snapshot) => void): void;
  installUpdate(): Promise<void>;
  onUpdate(handler: (info: UpdateInfo) => void): void;
  setRetention(days: number): Promise<Settings>;
  openRepository(): Promise<void>;
  reportIssue(): Promise<void>;
  diagnostics(): Promise<string>;
  minimize(): Promise<void>;
  hide(): Promise<void>;
  quit(): Promise<void>;
}
