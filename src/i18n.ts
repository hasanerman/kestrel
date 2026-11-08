import type { Condition, Language } from "./types";

const en = {
  condition: {
    starting: "Measuring",
    ok: "Stable",
    degraded: "Unstable",
    down: "No internet",
  } satisfies Record<Condition, string>,
  jitter: "jitter ms",
  loss: "loss %",
  noAnswer: "no answer",
  apps: "Apps",
  appsLater: "Per-app traffic and blocking arrive in a later version.",
  sites: "Sites",
  sitesLater: "The site access checker arrives in the next version.",
  settings: "Settings",
  language: "Language",
  languageHint: "Language of the window and the tray menu.",
  theme: "Theme",
  themeHint: "Follow the system or pick one.",
  themes: { system: "System", light: "Light", dark: "Dark" },
  notifications: "Notifications",
  notificationsToggle: "Tell me when the internet drops",
  notificationsHint: "A notification when the connection drops and when it comes back.",
  autostart: "Start with system",
  autostartToggle: "Open kestrel in the tray at login",
  autostartHint: "kestrel keeps measuring in the background.",
  on: "On",
  off: "Off",
  quit: "Quit kestrel",
};

type Strings = typeof en;

const tr: Strings = {
  condition: { starting: "Ölçülüyor", ok: "Stabil", degraded: "Dengesiz", down: "İnternet yok" },
  jitter: "jitter ms",
  loss: "kayıp %",
  noAnswer: "cevap yok",
  apps: "Uygulamalar",
  appsLater: "Uygulama bazında trafik ve engelleme sonraki bir sürümde geliyor.",
  sites: "Siteler",
  sitesLater: "Site erişim kontrolü bir sonraki sürümde geliyor.",
  settings: "Ayarlar",
  language: "Dil",
  languageHint: "Pencerenin ve tepsi menüsünün dili.",
  theme: "Tema",
  themeHint: "Sistemi takip et ya da birini seç.",
  themes: { system: "Sistem", light: "Açık", dark: "Koyu" },
  notifications: "Bildirimler",
  notificationsToggle: "İnternet koptuğunda haber ver",
  notificationsHint: "Bağlantı koptuğunda ve geri geldiğinde bildirim gösterir.",
  autostart: "Sistemle başlat",
  autostartToggle: "Oturum açılınca kestrel tepside açılsın",
  autostartHint: "kestrel arka planda ölçmeye devam eder.",
  on: "Açık",
  off: "Kapalı",
  quit: "kestrel'den çık",
};

const ru: Strings = {
  condition: { starting: "Измерение", ok: "Стабильно", degraded: "Нестабильно", down: "Нет интернета" },
  jitter: "джиттер мс",
  loss: "потери %",
  noAnswer: "нет ответа",
  apps: "Приложения",
  appsLater: "Трафик и блокировка приложений появятся в следующих версиях.",
  sites: "Сайты",
  sitesLater: "Проверка доступа к сайтам появится в следующей версии.",
  settings: "Настройки",
  language: "Язык",
  languageHint: "Язык окна и меню в трее.",
  theme: "Тема",
  themeHint: "Как в системе или выбрать вручную.",
  themes: { system: "Системная", light: "Светлая", dark: "Тёмная" },
  notifications: "Уведомления",
  notificationsToggle: "Сообщать, когда пропадает интернет",
  notificationsHint: "Уведомление, когда соединение пропадает и когда возвращается.",
  autostart: "Запуск с системой",
  autostartToggle: "Открывать kestrel в трее при входе",
  autostartHint: "kestrel продолжает измерять в фоне.",
  on: "Вкл",
  off: "Выкл",
  quit: "Выйти из kestrel",
};

export const LANGUAGES: { id: Language; name: string }[] = [
  { id: "en", name: "English" },
  { id: "tr", name: "Türkçe" },
  { id: "ru", name: "Русский" },
];

export function strings(language: string): Strings {
  return language === "tr" ? tr : language === "ru" ? ru : en;
}
