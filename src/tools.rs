//! External programs (`ffmpeg`, `ffprobe`, `tesseract`): where the path comes from and how it is checked.
//!
//! An empty field in the settings means the program is looked up in `PATH`; a non-empty one is run at that path.
//! A missing program is a state, not a crash: only its kind of work is switched off (ADR 0019, 0020).

use crate::i18n::{self, Lang};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The programs the application starts as processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    Ffprobe,
    Tesseract,
}

/// Every program in the order the interface and `doctor` show it.
pub const ALL: [Tool; 3] = [Tool::Ffmpeg, Tool::Ffprobe, Tool::Tesseract];

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Ffprobe => "ffprobe",
            Tool::Tesseract => "tesseract",
        }
    }

    /// The arguments after which the program prints its version and exits.
    pub fn version_args(self) -> &'static [&'static str] {
        match self {
            Tool::Ffmpeg | Tool::Ffprobe => &["-version"],
            Tool::Tesseract => &["--version"],
        }
    }

    /// The settings key in the DB (`settings.value`).
    pub fn setting_key(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "tool_ffmpeg",
            Tool::Ffprobe => "tool_ffprobe",
            Tool::Tesseract => "tool_tesseract",
        }
    }

    /// Why it is needed — for "Settings" and `doctor` (M21.6: the language of the interface or report).
    pub fn purpose(self, lang: Lang) -> &'static str {
        i18n::t(
            lang,
            match self {
                Tool::Ffmpeg => "video: cover frame, dHash, previews",
                Tool::Ffprobe => "video: duration and resolution",
                Tool::Tesseract => "OCR: text from documents and screenshots",
            },
        )
    }

    /// What stops working without it. The rest of the application always works.
    pub fn without(self, lang: Lang) -> &'static str {
        i18n::t(
            lang,
            match self {
                Tool::Ffmpeg => {
                    "videos in the index without previews or dHash, an error in the counter; photos are unaffected"
                }
                Tool::Ffprobe => "videos without duration or resolution",
                Tool::Tesseract => "OCR is off: the button is disabled, text search stays empty",
            },
        )
    }
}

/// The package manager that installs the programs on the running system (M26.2).
///
/// The dialog of the warning mark shows its commands and copies them to the clipboard: the app
/// itself installs nothing, downloads nothing and opens no browser (AGENTS.md §1). On Linux the
/// choice follows the package manager that is really on the machine, not a guessed name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installer {
    Pacman,
    Apt,
    Windows,
    Brew,
    /// No package manager was recognised: the official download pages are named instead.
    Unknown,
}

impl Installer {
    /// The system in words — a proper name, the same in both languages.
    pub fn system(self) -> &'static str {
        match self {
            Installer::Pacman => "Arch Linux",
            Installer::Apt => "Debian / Ubuntu",
            Installer::Windows => "Windows",
            Installer::Brew => "macOS",
            Installer::Unknown => "Linux",
        }
    }

    /// The commands that install the three programs, one per line. `ffprobe` ships with `ffmpeg`
    /// in every package named here, so a missing `ffprobe` needs no line of its own.
    pub fn commands(self) -> &'static [&'static str] {
        match self {
            Installer::Pacman => &[
                "sudo pacman -S ffmpeg tesseract tesseract-data-rus tesseract-data-eng",
            ],
            Installer::Apt => &[
                "sudo apt install ffmpeg tesseract-ocr tesseract-ocr-rus tesseract-ocr-eng",
            ],
            Installer::Windows => &[
                "winget install Gyan.FFmpeg",
                "winget install UB-Mannheim.TesseractOCR",
            ],
            Installer::Brew => &["brew install ffmpeg tesseract tesseract-lang"],
            // Nothing is guessed for an unknown system: the pages below are the honest way out.
            Installer::Unknown => &[],
        }
    }

    /// The pages the programs are downloaded from. They are printed as text and never opened.
    pub fn pages(self) -> &'static [&'static str] {
        match self {
            Installer::Windows | Installer::Unknown => &[
                "ffmpeg.org/download.html",
                "github.com/UB-Mannheim/tesseract/wiki",
            ],
            Installer::Pacman | Installer::Apt | Installer::Brew => &[],
        }
    }

    /// The one line a system needs on top of its command (`tesseract` packs elsewhere).
    pub fn note(self, lang: Lang) -> &'static str {
        match self {
            Installer::Windows => i18n::t(
                lang,
                "the tesseract installer offers the Russian and English language packs",
            ),
            Installer::Pacman | Installer::Apt | Installer::Brew => i18n::t(
                lang,
                "the command brings the Russian and English language packs of tesseract",
            ),
            Installer::Unknown => "",
        }
    }
}

/// The package manager of the running system (M26.2): chosen by the target and, on Linux, by what
/// is really installed — `/etc/arch-release` or `pacman` means Arch, `apt` means Debian/Ubuntu.
/// An unknown system is not a failure: the official pages are named instead of a command.
pub fn installer() -> Installer {
    if cfg!(target_os = "windows") {
        return Installer::Windows;
    }
    if cfg!(target_os = "macos") {
        return Installer::Brew;
    }
    let there = |path: &str| Path::new(path).is_file();
    if there("/etc/arch-release") || there("/usr/bin/pacman") {
        return Installer::Pacman;
    }
    if there("/usr/bin/apt") || there("/usr/bin/apt-get") {
        return Installer::Apt;
    }
    Installer::Unknown
}

/// Where `ffplay` is: next to the configured `ffmpeg`, or the first one in `PATH`. Nothing is
/// started — the viewer offers it explicitly when the built-in player cannot open a file (M26.12).
pub fn find_ffplay(tools: &Tools) -> Option<PathBuf> {
    let name = if cfg!(windows) { "ffplay.exe" } else { "ffplay" };
    if let Some(sibling) = tools
        .locate(Tool::Ffmpeg)
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .filter(|p| p.is_file())
    {
        return Some(sibling);
    }
    search_path(name)
}

/// The video player: `ffplay` next to the configured `ffmpeg`, or from `PATH`.
/// ffplay decodes both video and sound itself — no second stream and no manual audio are needed.
pub fn ffplay(tools: &Tools, path: &Path) -> Result<std::process::Child> {
    let program = find_ffplay(tools).unwrap_or_else(|| std::path::PathBuf::from("ffplay"));
    let mut cmd = Command::new(&program);
    cmd.args(["-autoexit", "-loglevel", "error", "-window_title", "photocat"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .with_context(|| format!("cannot start ffplay for {}", path.display()))
}

/// Open a file in the system player/viewer: the application does not decode video itself.
/// The process is started without waiting for it to finish; a failure to start is returned, not swallowed.
pub fn open_path(path: &Path) -> Result<()> {
    let mut cmd = {
        #[cfg(target_os = "windows")]
        {
            let mut c = Command::new("cmd");
            c.args(["/C", "start", ""]);
            c
        }
        #[cfg(target_os = "macos")]
        {
            Command::new("open")
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            Command::new("xdg-open")
        }
    };
    cmd.arg(path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .with_context(|| format!("cannot open {}", path.display()))?;
    Ok(())
}

/// Explicit paths to the programs. An empty string means `PATH`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub ffmpeg: String,
    pub ffprobe: String,
    pub tesseract: String,
}

impl Tools {
    pub fn get(&self, tool: Tool) -> &str {
        match tool {
            Tool::Ffmpeg => &self.ffmpeg,
            Tool::Ffprobe => &self.ffprobe,
            Tool::Tesseract => &self.tesseract,
        }
    }

    pub fn get_mut(&mut self, tool: Tool) -> &mut String {
        match tool {
            Tool::Ffmpeg => &mut self.ffmpeg,
            Tool::Ffprobe => &mut self.ffprobe,
            Tool::Tesseract => &mut self.tesseract,
        }
    }

    /// The owner set the path explicitly (empty means a lookup in `PATH`).
    pub fn configured(&self, tool: Tool) -> bool {
        !self.get(tool).trim().is_empty()
    }

    /// What goes into `Command::new`: an explicit path, or the program name to search in `PATH`.
    pub fn program(&self, tool: Tool) -> &str {
        let custom = self.get(tool).trim();
        if custom.is_empty() { tool.name() } else { custom }
    }

    /// Where the program came from, in words for the interface and `doctor`.
    pub fn source(&self, tool: Tool, lang: Lang) -> &'static str {
        i18n::t(
            lang,
            if self.configured(tool) {
                "set in the settings"
            } else {
                "from PATH"
            },
        )
    }

    /// Where the program really lies: the explicit path, or the first one found in `PATH`.
    pub fn locate(&self, tool: Tool) -> Option<PathBuf> {
        let program = self.program(tool);
        let path = Path::new(program);
        if path.components().count() > 1 {
            return path.is_file().then(|| path.to_path_buf());
        }
        search_path(program)
    }

    /// A command with the console window suppressed on Windows (ADR 0019, item 3).
    pub fn command(&self, tool: Tool) -> Command {
        #[allow(unused_mut)]
        let mut cmd = Command::new(self.program(tool));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        cmd
    }

    /// Check a path: run the program and take the first line of its version.
    /// The error carries both the path and the reason — the status shows it, not a console.
    pub fn version(&self, tool: Tool) -> Result<String> {
        let program = self.program(tool);
        let out = self
            .command(tool)
            .args(tool.version_args())
            .output()
            .with_context(|| format!("{}: cannot start “{program}”", tool.name()))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let detail = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
            bail!(
                "{} «{program}»: {}{}",
                tool.name(),
                match out.status.code() {
                    Some(code) => format!("exit code {code}"),
                    None => "the process was interrupted".to_string(),
                },
                if detail.is_empty() { String::new() } else { format!(" — {detail}") }
            );
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let line = stdout.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        if line.is_empty() {
            bail!("{} “{program}”: started but reported no version", tool.name());
        }
        Ok(line.to_string())
    }

    /// The version, or `None`: for "does it work" checks without noise about an error.
    pub fn probe(&self, tool: Tool) -> Option<String> {
        self.version(tool).ok()
    }
}

/// A lookup of the program name in `PATH`. Explicit paths never get here.
fn search_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            // On Windows `Command` appends `.exe` itself, but for showing the path we do it.
            for ext in ["exe", "cmd", "bat"] {
                let candidate = dir.join(format!("{program}.{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_setting_means_name_from_path() {
        let t = Tools::default();
        for tool in ALL {
            assert_eq!(t.program(tool), tool.name());
            assert_eq!(t.source(tool, Lang::Ru), "из PATH");
            assert!(!t.configured(tool));
        }
    }

    /// M26.12: `ffplay` is found next to the configured `ffmpeg` (the usual Windows layout of a
    /// portable ffmpeg folder) and nothing is started by the lookup.
    #[test]
    fn ffplay_is_found_next_to_ffmpeg() {
        let dir = std::env::temp_dir().join("photocat-tools-ffplay");
        std::fs::create_dir_all(&dir).expect("the test folder");
        let ffmpeg = dir.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
        let ffplay = dir.join(if cfg!(windows) { "ffplay.exe" } else { "ffplay" });
        std::fs::write(&ffmpeg, b"x").expect("the test program");
        std::fs::write(&ffplay, b"x").expect("the test program");
        let t = Tools {
            ffmpeg: ffmpeg.to_string_lossy().into_owned(),
            ..Tools::default()
        };
        assert_eq!(
            find_ffplay(&t).as_deref(),
            Some(ffplay.as_path()),
            "ffplay is not found next to ffmpeg"
        );
    }

    /// M26.2: the lines the info dialog shows are the right ones for the running system, and
    /// every command names both programs — `ffprobe` comes with `ffmpeg` and needs no line.
    #[test]
    fn the_install_lines_follow_the_running_system() {
        let found = installer();
        if cfg!(target_os = "windows") {
            assert_eq!(found, Installer::Windows);
        } else if cfg!(target_os = "macos") {
            assert_eq!(found, Installer::Brew);
        } else {
            assert!(
                matches!(found, Installer::Pacman | Installer::Apt | Installer::Unknown),
                "not a Linux package manager: {found:?}"
            );
        }
        for i in [
            Installer::Pacman,
            Installer::Apt,
            Installer::Windows,
            Installer::Brew,
        ] {
            assert!(!i.commands().is_empty(), "{i:?} shows no command");
            // Windows names its packages in CamelCase (`Gyan.FFmpeg`): the check ignores case.
            let text = i.commands().join(" ").to_ascii_lowercase();
            assert!(
                text.contains("ffmpeg"),
                "{i:?} does not install ffmpeg: {:?}",
                i.commands()
            );
            assert!(
                text.contains("tesseract"),
                "{i:?} does not install tesseract: {:?}",
                i.commands()
            );
            assert!(
                !i.commands().iter().any(|c| c.contains("ffprobe")),
                "{i:?} installs ffprobe on its own instead of with ffmpeg"
            );
            assert!(!i.note(Lang::Ru).is_empty(), "{i:?} explains no language packs");
        }
        // Nothing is installed on an unknown system: the pages are named, not a guessed command.
        assert!(Installer::Unknown.commands().is_empty());
        assert!(!Installer::Unknown.pages().is_empty());
        // The command and the pages are shown in both languages with the system's own name.
        assert_eq!(Installer::Pacman.system(), "Arch Linux");
        assert!(Installer::Windows.note(Lang::En).contains("language packs"));
        assert!(Installer::Windows.note(Lang::Ru).contains("языковые пакеты"));
    }

    #[test]
    fn explicit_path_wins_over_name() {
        let mut t = Tools::default();
        *t.get_mut(Tool::Tesseract) = "/opt/ocr/bin/tesseract".into();
        assert_eq!(t.program(Tool::Tesseract), "/opt/ocr/bin/tesseract");
        assert_eq!(t.source(Tool::Tesseract, Lang::Ru), "задано в настройках");
        // Spaces around a path must not break the launch.
        *t.get_mut(Tool::Ffmpeg) = "  ffmpeg  ".into();
        assert_eq!(t.program(Tool::Ffmpeg), "ffmpeg");
        assert!(t.configured(Tool::Ffmpeg));
    }

    #[test]
    fn missing_path_is_an_error_not_a_panic() {
        let mut t = Tools::default();
        *t.get_mut(Tool::Tesseract) = "/nonexistent/photocat-tesseract".into();
        let err = t.version(Tool::Tesseract).unwrap_err().to_string();
        assert!(err.contains("tesseract"), "{err}");
        assert!(err.contains("photocat-tesseract"), "{err}");
        assert!(t.locate(Tool::Tesseract).is_none());
    }

    #[test]
    fn every_tool_asks_for_version_the_way_it_understands() {
        assert_eq!(Tool::Ffmpeg.version_args(), &["-version"]);
        assert_eq!(Tool::Ffprobe.version_args(), &["-version"]);
        assert_eq!(Tool::Tesseract.version_args(), &["--version"]);
    }

    #[test]
    fn path_search_finds_nothing_for_unknown_name() {
        assert!(search_path("photocat-no-such-program-xyz").is_none());
    }

    #[test]
    fn locate_checks_explicit_file() {
        let dir = std::env::temp_dir().join(format!(
            "pc-tools-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("ffmpeg");
        std::fs::write(&fake, b"").unwrap();
        let mut t = Tools::default();
        *t.get_mut(Tool::Ffmpeg) = fake.to_string_lossy().to_string();
        assert_eq!(t.locate(Tool::Ffmpeg), Some(fake.clone()));
        // The file exists but is not a program: a launch error, not a silent success.
        assert!(t.version(Tool::Ffmpeg).is_err());
    }
}
