use crate::health::Condition;

pub struct Labels {
    pub starting: &'static str,
    pub ok: &'static str,
    pub degraded: &'static str,
    pub down: &'static str,
    pub open: &'static str,
    pub autostart: &'static str,
    pub quit: &'static str,
    pub lost: &'static str,
    pub lost_local: &'static str,
    pub restored: &'static str,
    pub busy_other: &'static str,
}

const EN: Labels = Labels {
    starting: "Measuring…",
    ok: "Connection is stable",
    degraded: "Connection is unstable",
    down: "No internet",
    open: "Open kestrel",
    autostart: "Start with system",
    quit: "Quit",
    lost: "Internet connection dropped. Your router answers, so the problem is likely your provider.",
    lost_local: "Your Wi-Fi or router is not responding.",
    restored: "Internet is back.",
    busy_other: "Ping is high, but not because of this computer. Another device on your network is probably using the line.",
};

const TR: Labels = Labels {
    starting: "Ölçülüyor…",
    ok: "Bağlantı stabil",
    degraded: "Bağlantı dengesiz",
    down: "İnternet yok",
    open: "kestrel'i aç",
    autostart: "Sistemle birlikte başlat",
    quit: "Çıkış",
    lost: "İnternet koptu. Modem cevap veriyor, sorun büyük ihtimalle servis sağlayıcında.",
    lost_local: "Wi-Fi ya da modem cevap vermiyor.",
    restored: "İnternet geri geldi.",
    busy_other: "Ping yüksek ama sebebi bu bilgisayar değil. Ağdaki başka bir cihaz büyük ihtimalle hattı dolduruyor.",
};

const RU: Labels = Labels {
    starting: "Измерение…",
    ok: "Соединение стабильно",
    degraded: "Соединение нестабильно",
    down: "Нет интернета",
    open: "Открыть kestrel",
    autostart: "Запускать вместе с системой",
    quit: "Выход",
    lost: "Интернет пропал. Роутер отвечает, скорее всего проблема у провайдера.",
    lost_local: "Wi-Fi или роутер не отвечает.",
    restored: "Интернет снова работает.",
    busy_other: "Пинг высокий, но не из-за этого компьютера. Скорее всего, канал занимает другое устройство в сети.",
};

pub fn labels(language: &str) -> &'static Labels {
    match language {
        "tr" => &TR,
        "ru" => &RU,
        _ => &EN,
    }
}

impl Labels {
    pub fn condition(&self, condition: Condition) -> &'static str {
        match condition {
            Condition::Starting => self.starting,
            Condition::Ok => self.ok,
            Condition::Degraded => self.degraded,
            Condition::Down => self.down,
        }
    }
}
