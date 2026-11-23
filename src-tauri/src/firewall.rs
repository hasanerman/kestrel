pub const GROUP: &str = "kestrel";

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
         New-NetFirewallRule -DisplayName {name} -Group {group} -Direction Inbound -Action Block -Program {program} | Out-Null"
    )
}

pub fn unblock_script(app: &str) -> String {
    format!("Remove-NetFirewallRule -DisplayName {} -ErrorAction SilentlyContinue", quoted(&rule_name(app)))
}

#[cfg(windows)]
fn powershell(script: &str) -> Result<(), String> {
    let output = crate::system::run("powershell", &["-NoProfile", "-NonInteractive", "-Command", script]).map_err(|e| e.to_string())?;
    let text = crate::system::output_text(&output);
    if output.status.success() && text.is_empty() {
        Ok(())
    } else if text.to_ascii_lowercase().contains("access is denied") || text.contains("0x80070005") {
        Err("needs_admin".into())
    } else {
        Err(text.chars().take(300).collect())
    }
}

#[cfg(windows)]
pub fn block(app: &str, path: &str) -> Result<(), String> {
    powershell(&block_script(app, path))
}

#[cfg(windows)]
pub fn unblock(app: &str) -> Result<(), String> {
    powershell(&unblock_script(app))
}

#[cfg(not(windows))]
pub fn block(_app: &str, _path: &str) -> Result<(), String> {
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
        assert_eq!(unblock_script("Discord"), "Remove-NetFirewallRule -DisplayName 'kestrel block Discord' -ErrorAction SilentlyContinue");
    }
}
