# Changelog

## 0.2.0

- Hourly view: a 7-day heat map of every hour with average ping, loss and drops, the worst hours of the day and 7-day uptime.
- Traffic tab on Windows: live download and upload per app, a feed of every connection with the site name taken from DNS, TLS and HTTP, and a packet inspector that shows the first bytes of each connection. Nothing is stored or uploaded.
- Block or allow internet access for any app with one click (Windows Firewall rules in a kestrel group, removed on uninstall).
- Average download and upload rate since kestrel started, next to the live rate.
- Speed test on the Health screen using four parallel downloads from Cloudflare's speed servers.
- Sites blocked by DNS or DPI get a "How to solve" panel that explains the block, opens the site in FreeDPI when it is installed (FreeDPI 1.0.2 or newer adds it to its list and connects), links to the FreeDPI download and shows FreeDPI on GitHub.
- Blocking an app warns when Windows Firewall is off for the current network, because the rule has no effect then.
- The GitHub link is now shown at the bottom of every screen.

## 0.1.0

- Live ping, jitter and packet loss to your router, Cloudflare and Google, measured with ICMP and a TCP fallback.
- Live graph for the last 5 minutes and history for the last hour and day.
- Outage log that tells an internet provider problem apart from a Wi-Fi or router problem.
- Site checker: tests DNS, TCP and HTTPS for each site and says whether it works, is blocked by DNS or DPI, or is down. Built-in lists plus your own sites, with a copyable report.
- Tray icon that turns green, amber or red with the connection, and notifications when the internet drops and comes back.
- Automatic updates, copyable diagnostics and a link to report problems on GitHub.
- Choose how long history is kept: 7, 30 or 90 days.
- English, Turkish and Russian, light and dark theme, start with system.
