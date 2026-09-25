use std::fs;
use std::path::Path;

pub fn apply(enabled: bool) -> Result<(), String> {
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = enabled;
        return Err("Open on startup works on macOS, Windows, and Linux.".into());
    }
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        let exe = std::env::current_exe()
            .map_err(|err| err.to_string())?
            .to_string_lossy()
            .into_owned();
        #[cfg(target_os = "windows")]
        return windows_run_entry(&exe, enabled);
        #[cfg(not(target_os = "windows"))]
        apply_in(&login_dir()?, &exe, enabled)
    }
}

/// Windows starts the programs listed under the user's `Run` key at sign-in.
#[cfg(target_os = "windows")]
fn windows_run_entry(exe: &str, enabled: bool) -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::{
        RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
    };

    let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let status = if enabled {
        let command: Vec<u16> = run_command(exe).encode_utf16().chain([0]).collect();
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key,
                w!("Whisple"),
                REG_SZ.0,
                Some(command.as_ptr().cast()),
                (command.len() * size_of::<u16>()) as u32,
            )
        }
    } else {
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key, w!("Whisple")) } {
            ERROR_FILE_NOT_FOUND => return Ok(()),
            status => status,
        }
    };
    status
        .ok()
        .map_err(|err| format!("Could not update the startup apps: {err}"))
}

/// The `Run` value is a command line, so a path with spaces needs quotes.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn run_command(exe: &str) -> String {
    format!("\"{exe}\"")
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn apply_in(dir: &Path, exe: &str, enabled: bool) -> Result<(), String> {
    let path = dir.join(file_name());
    if enabled {
        fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        fs::write(&path, entry(exe)).map_err(|err| err.to_string())?;
    } else if path.exists() {
        fs::remove_file(&path).map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
fn file_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "app.whisp.plist"
    } else {
        "whisp.desktop"
    }
}

#[cfg(not(target_os = "windows"))]
fn login_dir() -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or("Could not find the home directory.")?;
        Ok(home.join("Library").join("LaunchAgents"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(dirs::config_dir()
            .ok_or("Could not find the config directory.")?
            .join("autostart"))
    }
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
fn entry(exe: &str) -> String {
    if cfg!(target_os = "macos") {
        launch_agent(exe)
    } else {
        desktop_entry(exe)
    }
}

pub fn desktop_entry(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Whisple\nComment=Local voice dictation\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_exec(exe)
    )
}

fn desktop_exec(path: &str) -> String {
    let escaped = path.replace('%', "%%");
    if escaped
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\\' | '\''))
    {
        let quoted = escaped.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{quoted}\"")
    } else {
        escaped
    }
}

pub fn launch_agent(exe: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>Label</key>\n\t<string>app.whisp</string>\n\t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{}</string>\n\t</array>\n\t<key>RunAtLoad</key>\n\t<true/>\n\t<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>\n</dict>\n</plist>\n",
        xml_escape(exe)
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_quotes_a_path_with_spaces() {
        let entry = desktop_entry("/Users/Lasse/Dev/whisp app/whisp");
        assert!(entry.contains("Exec=\"/Users/Lasse/Dev/whisp app/whisp\""));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
        assert!(entry.contains("Name=Whisple"));
    }

    #[test]
    fn desktop_entry_escapes_field_codes() {
        let entry = desktop_entry("/tmp/whisp%20");
        assert!(entry.contains("Exec=/tmp/whisp%%20"));
    }

    #[test]
    fn launch_agent_escapes_the_binary_path() {
        let agent = launch_agent("/tmp/whisp & notes");
        assert!(agent.contains("<string>/tmp/whisp &amp; notes</string>"));
        assert!(agent.contains("<key>RunAtLoad</key>"));
        assert!(agent.contains("<string>app.whisp</string>"));
    }

    #[test]
    fn run_command_quotes_a_path_with_spaces() {
        assert_eq!(
            run_command(r"C:\Program Files\Whisple\whisple.exe"),
            r#""C:\Program Files\Whisple\whisple.exe""#
        );
    }

    #[test]
    fn enabling_writes_the_login_item_and_disabling_removes_it() {
        let dir = std::env::temp_dir().join(format!("whisp-startup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        apply_in(&dir, "/tmp/whisp", true).unwrap();
        let raw = fs::read_to_string(dir.join(file_name())).unwrap();
        assert!(raw.contains("/tmp/whisp"));
        apply_in(&dir, "/tmp/whisp", false).unwrap();
        assert!(!dir.join(file_name()).exists());
        apply_in(&dir, "/tmp/whisp", false).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
