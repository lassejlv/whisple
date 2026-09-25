#[cfg(not(target_os = "macos"))]
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: String,
    generic: Option<String>,
    path: PathBuf,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    exec: Option<String>,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    terminal: bool,
}

pub fn installed() -> Vec<App> {
    let mut apps = scan();
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    apps
}

#[cfg(target_os = "macos")]
fn scan() -> Vec<App> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/Applications/Utilities"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    let mut apps = Vec::new();
    for root in roots {
        collect_bundles(&root, 1, &mut apps);
    }
    let finder = Path::new("/System/Library/CoreServices/Finder.app");
    if finder.exists() {
        apps.push(bundle(finder.to_path_buf()));
    }
    apps
}

#[cfg(target_os = "macos")]
fn collect_bundles(dir: &Path, depth: usize, apps: &mut Vec<App>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "app") {
            apps.push(bundle(path));
        } else if depth > 0 && path.is_dir() {
            collect_bundles(&path, depth - 1, apps);
        }
    }
}

#[cfg(target_os = "macos")]
fn bundle(path: PathBuf) -> App {
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    App {
        name,
        generic: None,
        path,
        exec: None,
        terminal: false,
    }
}

#[cfg(not(target_os = "macos"))]
fn scan() -> Vec<App> {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();
    for dir in desktop_dirs() {
        collect_entries(&dir, &dir, &mut seen, &mut apps);
    }
    apps
}

#[cfg(not(target_os = "macos"))]
fn desktop_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = dirs::home_dir();
    match std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        Some(data) => dirs.push(PathBuf::from(data).join("applications")),
        None => {
            if let Some(home) = &home {
                dirs.push(home.join(".local/share/applications"));
            }
        }
    }
    if let Some(home) = &home {
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for dir in data_dirs.split(':').filter(|dir| !dir.is_empty()) {
        dirs.push(Path::new(dir).join("applications"));
    }
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs.push(PathBuf::from("/var/lib/snapd/desktop/applications"));
    let mut unique = HashSet::new();
    dirs.retain(|dir| unique.insert(dir.clone()));
    dirs
}

#[cfg(not(target_os = "macos"))]
fn collect_entries(root: &Path, dir: &Path, seen: &mut HashSet<String>, apps: &mut Vec<App>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_entries(root, &path, seen, apps);
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "desktop") {
            continue;
        }
        // The desktop id is the path under `applications` with `/` as `-`.
        let id = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('/', "-");
        if !seen.insert(id) {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(app) = parse_desktop_entry(&raw, path) {
            apps.push(app);
        }
    }
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn parse_desktop_entry(raw: &str, path: PathBuf) -> Option<App> {
    let mut in_entry = false;
    let mut name = None;
    let mut generic = None;
    let mut exec = None;
    let mut kind = None;
    let mut hidden = false;
    let mut terminal = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unescape_value(value.trim());
        match key.trim() {
            "Name" => name = Some(value),
            "GenericName" => generic = Some(value),
            "Exec" => exec = Some(value),
            "Type" => kind = Some(value),
            "NoDisplay" | "Hidden" => hidden |= value == "true",
            "Terminal" => terminal = value == "true",
            _ => {}
        }
    }
    if hidden || kind.as_deref() != Some("Application") {
        return None;
    }
    let name = name.filter(|name| !name.is_empty())?;
    Some(App {
        name,
        generic: generic.filter(|generic| !generic.is_empty()),
        path,
        exec: Some(exec.filter(|exec| !exec.trim().is_empty())?),
        terminal,
    })
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn unescape_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn exec_args(exec: &str, app: &App) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = exec.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => match chars.next() {
                Some(next) => current.push(next),
                None => return Err("The app's launch command is malformed.".into()),
            },
            ch if ch.is_whitespace() && !quoted => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '%' => {
                started = true;
                match chars.next() {
                    Some('%') => current.push('%'),
                    Some('c') => current.push_str(&app.name),
                    Some('k') => current.push_str(&app.path.to_string_lossy()),
                    // %f %F %u %U %i and the deprecated codes expand to
                    // nothing when no file is being opened.
                    Some(_) => {}
                    None => return Err("The app's launch command is malformed.".into()),
                }
            }
            ch => {
                started = true;
                current.push(ch);
            }
        }
    }
    if quoted {
        return Err("The app's launch command is malformed.".into());
    }
    if started {
        args.push(current);
    }
    args.retain(|arg| !arg.is_empty());
    if args.is_empty() {
        return Err(format!("{} has no launch command.", app.name));
    }
    Ok(args)
}

/// The installed app a spoken name means, if one clearly matches.
///
/// Matching is strict on purpose: a miss means the words are typed as a note
/// instead, which is much better than opening the wrong app.
pub fn find<'a>(apps: &'a [App], spoken: &str) -> Option<&'a App> {
    let query = squash(&alias(spoken));
    if query.len() < 2 {
        return None;
    }
    apps.iter()
        .filter_map(|app| score(app, &query).map(|score| (score, app)))
        .max_by(|(a, app_a), (b, app_b)| {
            a.cmp(b)
                .then_with(|| app_b.name.len().cmp(&app_a.name.len()))
        })
        .map(|(_, app)| app)
}

fn score(app: &App, query: &str) -> Option<u8> {
    let name = squash(&app.name);
    if name == query {
        return Some(100);
    }
    let words: Vec<String> = app.name.split_whitespace().map(squash).collect();
    if words.len() >= 2 && words[1..].concat() == query {
        return Some(90);
    }
    // "Visual Studio" for "Visual Studio Code". The spoken part must end on
    // a word boundary and be long enough not to be an accident.
    if query.len() >= 4 {
        let mut prefix = String::new();
        for word in &words[..words.len().saturating_sub(1)] {
            prefix.push_str(word);
            if prefix == query {
                return Some(80);
            }
        }
    }
    if app
        .generic
        .as_deref()
        .is_some_and(|generic| squash(generic) == query)
    {
        return Some(70);
    }
    let allowed = match query.chars().count() {
        0..=4 => 0,
        5..=8 => 1,
        _ => 2,
    };
    (allowed > 0 && edit_distance(&name, query) <= allowed).then_some(60)
}

fn alias(spoken: &str) -> String {
    let lower = spoken.trim().to_lowercase();
    match squash(&lower).as_str() {
        "vscode" | "vsc" => "Visual Studio Code".into(),
        "settings" if cfg!(target_os = "macos") => "System Settings".into(),
        "systempreferences" if cfg!(target_os = "macos") => "System Settings".into(),
        "appstore" if cfg!(target_os = "macos") => "App Store".into(),
        _ => lower,
    }
}

fn squash(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitute = previous[j] + usize::from(ca != cb);
            current[j + 1] = substitute.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

pub fn launch(app: &App) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("/usr/bin/open")
            .arg("-a")
            .arg(&app.path)
            .output()
            .map_err(|err| format!("Could not open {}: {err}", app.name))?;
        if !output.status.success() {
            return Err(format!("Could not open {}.", app.name));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let exec = app
            .exec
            .as_deref()
            .ok_or_else(|| format!("{} has no launch command.", app.name))?;
        let mut args = exec_args(exec, app)?;
        if app.terminal {
            let terminal = ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"]
                .into_iter()
                .find(|program| on_path(program))
                .ok_or_else(|| {
                    format!("{} runs in a terminal, and none is installed.", app.name)
                })?;
            let separator = if terminal == "gnome-terminal" {
                "--"
            } else {
                "-e"
            };
            args.splice(0..0, [terminal.to_string(), separator.to_string()]);
        }
        spawn_detached(&args).map_err(|err| format!("Could not open {}: {err}", app.name))
    }
}

pub fn open_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Only web addresses can be opened.".into());
    }
    #[cfg(target_os = "windows")]
    return shell_open(url).map_err(|err| format!("Could not open {url}: {err}"));
    #[cfg(target_os = "macos")]
    let args = ["/usr/bin/open".to_string(), url.to_string()];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let args = ["xdg-open".to_string(), url.to_string()];
    #[cfg(not(target_os = "windows"))]
    spawn_detached(&args).map_err(|err| format!("Could not open {url}: {err}"))
}

/// Opens a web address in the default browser. The shell parses nothing, so
/// characters such as `&` in the address stay part of it.
#[cfg(target_os = "windows")]
fn shell_open(url: &str) -> windows::core::Result<()> {
    use windows::core::{w, HSTRING};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success with any value above 32.
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(windows::core::Error::from_thread())
    }
}

/// Starts a program that outlives Whisple, in its own process group so a
/// signal to Whisple does not reach it. A thread reaps it when it exits.
fn spawn_detached(args: &[String]) -> std::io::Result<()> {
    let (program, rest) = args
        .split_first()
        .ok_or_else(|| std::io::Error::other("empty command"))?;
    let mut command = Command::new(program);
    command
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    std::thread::Builder::new()
        .name("whisp-launched".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> App {
        App {
            name: name.into(),
            generic: None,
            path: PathBuf::from(format!("/apps/{name}.desktop")),
            exec: Some("true".into()),
            terminal: false,
        }
    }

    fn catalog() -> Vec<App> {
        let mut terminal = app("GNOME Console");
        terminal.generic = Some("Terminal".into());
        vec![
            app("Spotify"),
            app("Google Chrome"),
            app("Visual Studio Code"),
            app("Microsoft Word"),
            app("Notes"),
            app("Numbers"),
            app("LibreOffice"),
            app("LibreOffice Writer"),
            terminal,
        ]
    }

    fn found(spoken: &str) -> Option<String> {
        find(&catalog(), spoken).map(|app| app.name.clone())
    }

    #[test]
    fn spoken_names_find_their_app() {
        assert_eq!(found("spotify").as_deref(), Some("Spotify"));
        assert_eq!(found("Chrome").as_deref(), Some("Google Chrome"));
        assert_eq!(found("word").as_deref(), Some("Microsoft Word"));
        assert_eq!(
            found("visual studio").as_deref(),
            Some("Visual Studio Code")
        );
        assert_eq!(found("VS Code").as_deref(), Some("Visual Studio Code"));
        assert_eq!(found("libre office").as_deref(), Some("LibreOffice"));
        assert_eq!(found("writer").as_deref(), Some("LibreOffice Writer"));
        assert_eq!(found("terminal").as_deref(), Some("GNOME Console"));
        assert_eq!(found("Spotfy").as_deref(), Some("Spotify"));
    }

    #[test]
    fn near_misses_stay_dictation() {
        assert_eq!(found("numbers again"), None);
        assert_eq!(found("meeting"), None);
        assert_eq!(found("a new tab"), None);
        assert_eq!(found("note"), None);
        assert_eq!(found("x"), None);
    }

    #[test]
    fn desktop_entries_keep_apps_and_skip_the_rest() {
        let entry = "[Desktop Entry]\nType=Application\nName=Firefox\nGenericName=Web Browser\nExec=firefox %u\n\n[Desktop Action new-window]\nName=New Window\nExec=firefox --new-window\n";
        let app = parse_desktop_entry(entry, PathBuf::from("/x/firefox.desktop")).unwrap();
        assert_eq!(app.name, "Firefox");
        assert_eq!(app.generic.as_deref(), Some("Web Browser"));
        assert_eq!(app.exec.as_deref(), Some("firefox %u"));

        let hidden =
            "[Desktop Entry]\nType=Application\nName=Helper\nExec=helper\nNoDisplay=true\n";
        assert!(parse_desktop_entry(hidden, PathBuf::from("/x/h.desktop")).is_none());
        let link = "[Desktop Entry]\nType=Link\nName=Docs\nURL=https://example.com\n";
        assert!(parse_desktop_entry(link, PathBuf::from("/x/l.desktop")).is_none());
    }

    #[test]
    fn exec_lines_drop_placeholders_and_keep_quotes() {
        let app = app("Code");
        assert_eq!(
            exec_args("code --new-window %F", &app).unwrap(),
            vec!["code", "--new-window"]
        );
        assert_eq!(
            exec_args(r#""/opt/My App/run" --name=%c "a \"b\"" 100%%"#, &app).unwrap(),
            vec!["/opt/My App/run", "--name=Code", "a \"b\"", "100%"]
        );
        assert!(exec_args("\"unterminated", &app).is_err());
        assert!(exec_args("%U", &app).is_err());
    }

    #[test]
    fn escaped_values_are_decoded() {
        assert_eq!(unescape_value(r"a\sb\\c"), r"a b\c");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_user_entry_hides_the_system_one() {
        let user = tempfile::tempdir().unwrap();
        let system = tempfile::tempdir().unwrap();
        std::fs::write(
            user.path().join("editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=mine\n",
        )
        .unwrap();
        std::fs::write(
            system.path().join("editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=theirs\n",
        )
        .unwrap();
        let mut seen = HashSet::new();
        let mut apps = Vec::new();
        collect_entries(user.path(), user.path(), &mut seen, &mut apps);
        collect_entries(system.path(), system.path(), &mut seen, &mut apps);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].exec.as_deref(), Some("mine"));
    }

    #[test]
    fn only_web_addresses_open() {
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
    }
}
