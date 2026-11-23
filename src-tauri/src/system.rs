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

const ELEVATION_REQUIRED: i32 = 740;

#[cfg(windows)]
fn launch_elevated(command: &Command, error: &std::io::Error) -> Result<(), String> {
    if error.raw_os_error() != Some(ELEVATION_REQUIRED) {
        return Err(error.to_string());
    }
    let quote = |value: &str| format!("'{}'", value.replace('\'', "''"));
    let program = quote(&command.get_program().to_string_lossy());
    let args: Vec<String> = command.get_args().map(|a| quote(&a.to_string_lossy())).collect();
    let mut script = format!("Start-Process -FilePath {program} -Verb RunAs");
    if !args.is_empty() {
        script.push_str(&format!(" -ArgumentList {}", args.join(",")));
    }
    let mut elevate = Command::new("powershell");
    elevate.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    hide_window(&mut elevate);
    elevate.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn launch_elevated(_command: &Command, error: &std::io::Error) -> Result<(), String> {
    let _ = ELEVATION_REQUIRED;
    Err(error.to_string())
}

pub fn open_freedpi_repository() -> Result<(), String> {
    open_url(FREEDPI)
}

pub fn freedpi_installed() -> bool {
    freedpi_launcher().is_some()
}

pub fn open_freedpi(host: Option<&str>) -> Result<bool, String> {
    if let Some(mut command) = freedpi_launcher() {
        if let Some(host) = host {
            if cfg!(target_os = "macos") {
                command.arg("--args");
            }
            command.args(["--add-site", host]);
        }
        if let Err(error) = command.spawn() {
            launch_elevated(&command, &error)?;
        }
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
