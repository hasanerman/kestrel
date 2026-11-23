pub const GROUP: &str = "kestrel";
const FIREWALL_OFF: &str = "firewall_off:";

pub fn rule_name(app: &str) -> String {
    format!("kestrel block {app}")
}

fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub fn block_script(app: &str, path: &str) -> String {
    let name = quoted(&rule_name(app));
    let program = quoted(path);
    let group = quoted(GROUP);
    format!(
        "Remove-NetFirewallRule -DisplayName {name} -ErrorAction SilentlyContinue; \
         New-NetFirewallRule -DisplayName {name} -Group {group} -Direction Outbound -Action Block -Program {program} | Out-Null; \
         New-NetFirewallRule -DisplayName {name} -Group {group} -Direction Inbound -Action Block -Program {program} | Out-Null; \
         {}",
        off_profiles_script()
    )
}

pub fn off_profiles_script() -> String {
    format!(
        "$off = Get-NetConnectionProfile | ForEach-Object {{ $c = \"$($_.NetworkCategory)\"; if ($c -eq 'DomainAuthenticated') {{ 'Domain' }} else {{ $c }} }} | \
         Select-Object -Unique | Where-Object {{ -not (Get-NetFirewallProfile -Name $_).Enabled }}; \
         if ($off) {{ Write-Output ('{FIREWALL_OFF}' + ($off -join ',')) }}"
    )
}

pub fn unblock_script(app: &str) -> String {
    format!("Remove-NetFirewallRule -DisplayName {} -ErrorAction SilentlyContinue", quoted(&rule_name(app)))
}

pub fn warning_from(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix(FIREWALL_OFF))
        .map(|profiles| format!("firewall_off:{profiles}"))
}

#[cfg(windows)]
fn powershell(script: &str) -> Result<String, String> {
    let output = crate::system::run("powershell", &["-NoProfile", "-NonInteractive", "-Command", script]).map_err(|e| e.to_string())?;
    let text = crate::system::output_text(&output);
    let lower = text.to_ascii_lowercase();
    if lower.contains("access is denied") || text.contains("0x80070005") {
        Err("needs_admin".into())
    } else if !output.status.success() || (!text.is_empty() && !text.contains(FIREWALL_OFF)) {
        Err(text.chars().take(300).collect())
    } else {
        Ok(text)
    }
}

#[cfg(windows)]
pub fn block(app: &str, path: &str) -> Result<Option<String>, String> {
    powershell(&block_script(app, path)).map(|output| warning_from(&output))
}

#[cfg(windows)]
pub fn unblock(app: &str) -> Result<(), String> {
    powershell(&unblock_script(app)).map(|_| ())
}

#[cfg(not(windows))]
pub fn block(_app: &str, _path: &str) -> Result<Option<String>, String> {
    Err("unsupported".into())
}

#[cfg(not(windows))]
pub fn unblock(_app: &str) -> Result<(), String> {
    Err("unsupported".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_quote_names_and_paths() {
        let script = block_script("O'Neil", "C:\\Program Files\\App\\app.exe");
        assert!(script.contains("-DisplayName 'kestrel block O''Neil'"));
        assert!(script.contains("-Program 'C:\\Program Files\\App\\app.exe'"));
        assert!(script.contains("-Group 'kestrel'"));
        assert!(script.contains("Get-NetFirewallProfile"));
        assert_eq!(unblock_script("Discord"), "Remove-NetFirewallRule -DisplayName 'kestrel block Discord' -ErrorAction SilentlyContinue");
    }

    #[test]
    fn reads_firewall_warning() {
        assert_eq!(warning_from("firewall_off:Private\r\n").as_deref(), Some("firewall_off:Private"));
        assert_eq!(warning_from(""), None);
    }
}
