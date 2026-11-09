use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Category {
    pub id: &'static str,
    pub hosts: &'static [&'static str],
}

pub const CATEGORIES: &[Category] = &[
    Category {
        id: "social",
        hosts: &[
            "discord.com",
            "youtube.com",
            "x.com",
            "instagram.com",
            "facebook.com",
            "reddit.com",
            "tiktok.com",
            "telegram.org",
            "whatsapp.com",
        ],
    },
    Category {
        id: "gaming",
        hosts: &[
            "store.steampowered.com",
            "roblox.com",
            "epicgames.com",
            "playvalorant.com",
            "twitch.tv",
            "ea.com",
            "battle.net",
        ],
    },
    Category {
        id: "news",
        hosts: &["wikipedia.org", "bbc.com", "reuters.com", "theguardian.com", "dw.com", "nytimes.com"],
    },
    Category {
        id: "dev",
        hosts: &["github.com", "gitlab.com", "stackoverflow.com", "npmjs.com", "crates.io", "pypi.org"],
    },
];
