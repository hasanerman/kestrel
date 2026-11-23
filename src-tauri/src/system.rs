#![cfg_attr(not(windows), allow(dead_code))]

use std::io;
use std::process::{Command, Output, Stdio};

pub const REPOSITORY: &str = "https://github.com/hasanerman/kestrel";
pub const FREEDPI: &str = "https://github.com/hasanerman/FreeDPI";

#[cfg(windows)]
fn freedpi_launcher() -> Option<Command> {
    let base = std::path::PathBuf::from(std::env::var_os("ProgramFiles")?).join("FreeDPI");
    let exe = ["freedpi.exe", "FreeDPI.exe"].iter().map(|name| base.join(name)).find(|path| path.exists())?;
    Some(Command::new(exe))
}

#[cfg(target_os = "macos")]
fn freedpi_launcher() -> Option<Command> {
    let app = std::path::Path::new("/Applications/FreeDPI.app");
    if !app.exists() {
        return None;
    }
    let mut command = Command::new("/usr/bin/open");
    command.arg(app);
    Some(command)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn freedpi_launcher() -> Option<Command> {
    None
}

pub fn open_freedpi() -> Result<bool, String> {
    if let Some(mut command) = freedpi_launcher() {
        command.spawn().map_err(|e| e.to_string())?;
        return Ok(true);
    }
    open_url(&format!("{FREEDPI}/releases/latest"))?;
    Ok(false)
}

pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "/usr/bin/open";
    #[cfg(not(any(windows, target_os = "macos")))]
    let program = "xdg-open";
    Command::new(program).arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

pub fn run(program: &str, args: &[&str]) -> io::Result<Output> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null());
    hide_window(&mut command);
    command.output()
}

#[cfg(windows)]
pub fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_window(_command: &mut Command) {}

pub fn output_text(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.trim().to_string()
}
