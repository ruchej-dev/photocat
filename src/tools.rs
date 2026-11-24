//! Внешние программы (`ffmpeg`, `ffprobe`, `tesseract`, `curl`): откуда брать путь и как проверить.
//!
//! Пустое поле в настройках — программа ищется в `PATH`; непустое — запускается по указанному пути.
//! Отсутствие программы — состояние, а не падение: отключается только её вид работы (ADR 0019, 0020).

use crate::i18n::{self, Lang};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Программы, которые приложение запускает процессами.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    Ffprobe,
    Tesseract,
    Curl,
}

/// Все программы в порядке показа в интерфейсе и `doctor`.
pub const ALL: [Tool; 4] = [Tool::Ffmpeg, Tool::Ffprobe, Tool::Tesseract, Tool::Curl];

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Ffprobe => "ffprobe",
            Tool::Tesseract => "tesseract",
            Tool::Curl => "curl",
        }
    }

    /// Аргументы, после которых программа печатает версию и завершается.
    pub fn version_args(self) -> &'static [&'static str] {
        match self {
            Tool::Ffmpeg | Tool::Ffprobe => &["-version"],
            Tool::Tesseract | Tool::Curl => &["--version"],
        }
    }

    /// Ключ настройки в БД (`settings.value`).
    pub fn setting_key(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "tool_ffmpeg",
            Tool::Ffprobe => "tool_ffprobe",
            Tool::Tesseract => "tool_tesseract",
            Tool::Curl => "tool_curl",
        }
    }

    /// Зачем нужна — для «Настроек» и `doctor` (M21.6: язык интерфейса или отчёта).
    pub fn purpose(self, lang: Lang) -> &'static str {
        i18n::t(
            lang,
            match self {
                Tool::Ffmpeg => "video: cover frame, dHash, previews",
                Tool::Ffprobe => "video: duration and resolution",
                Tool::Tesseract => "OCR: text from documents and screenshots",
                Tool::Curl => "downloading model weights on consent",
            },
        )
    }

    /// Что перестанет работать без неё. Остальное приложение работает всегда.
    pub fn without(self, lang: Lang) -> &'static str {
        i18n::t(
            lang,
            match self {
                Tool::Ffmpeg => {
                    "videos in the index without previews or dHash, an error in the counter; photos are unaffected"
                }
                Tool::Ffprobe => "videos without duration or resolution",
                Tool::Tesseract => "OCR is off: the button is disabled, text search stays empty",
                Tool::Curl => "the model weights will have to be placed by hand (scripts/fetch-model.sh)",
            },
        )
    }
}

/// Плеер видео: `ffplay` рядом с настроенным `ffmpeg` или из `PATH`.
/// ffplay сам декодирует и видео, и звук — второй поток и ручной звук не нужны.
pub fn ffplay(tools: &Tools, path: &Path) -> Result<std::process::Child> {
    let name = if cfg!(windows) { "ffplay.exe" } else { "ffplay" };
    let program = tools
        .locate(Tool::Ffmpeg)
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| std::path::PathBuf::from("ffplay"));
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
        .with_context(|| format!("не запустить ffplay для {}", path.display()))
}

/// Открыть файл системным проигрывателем/просмотрщиком: приложение не декодирует видео сам.
/// Процесс запускается и не ждёт завершения; ошибка запуска возвращается, а не глотается.
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
        .with_context(|| format!("не открыть {}", path.display()))?;
    Ok(())
}

/// Явные пути к программам. Пустая строка = `PATH`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub ffmpeg: String,
    pub ffprobe: String,
    pub tesseract: String,
    pub curl: String,
}

impl Tools {
    pub fn get(&self, tool: Tool) -> &str {
        match tool {
            Tool::Ffmpeg => &self.ffmpeg,
            Tool::Ffprobe => &self.ffprobe,
            Tool::Tesseract => &self.tesseract,
            Tool::Curl => &self.curl,
        }
    }

    pub fn get_mut(&mut self, tool: Tool) -> &mut String {
        match tool {
            Tool::Ffmpeg => &mut self.ffmpeg,
            Tool::Ffprobe => &mut self.ffprobe,
            Tool::Tesseract => &mut self.tesseract,
            Tool::Curl => &mut self.curl,
        }
    }

    /// Владелец задал путь явно (пусто — ищем в `PATH`).
    pub fn configured(&self, tool: Tool) -> bool {
        !self.get(tool).trim().is_empty()
    }

    /// Что передаётся в `Command::new`: явный путь или имя программы для поиска в `PATH`.
    pub fn program(&self, tool: Tool) -> &str {
        let custom = self.get(tool).trim();
        if custom.is_empty() { tool.name() } else { custom }
    }

    /// Откуда взята программа — словами для интерфейса и `doctor`.
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

    /// Где программа лежит на самом деле: явный путь или первый найденный в `PATH`.
    pub fn locate(&self, tool: Tool) -> Option<PathBuf> {
        let program = self.program(tool);
        let path = Path::new(program);
        if path.components().count() > 1 {
            return path.is_file().then(|| path.to_path_buf());
        }
        search_path(program)
    }

    /// Команда с подавленным консольным окном на Windows (ADR 0019, п. 3).
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

    /// Проверить путь: запустить программу и получить первую строку версии.
    /// Ошибка содержит и путь, и причину — её показывает статус, а не консоль.
    pub fn version(&self, tool: Tool) -> Result<String> {
        let program = self.program(tool);
        let out = self
            .command(tool)
            .args(tool.version_args())
            .output()
            .with_context(|| format!("{}: не запустить «{program}»", tool.name()))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let detail = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
            bail!(
                "{} «{program}»: {}{}",
                tool.name(),
                match out.status.code() {
                    Some(code) => format!("код выхода {code}"),
                    None => "процесс прерван".to_string(),
                },
                if detail.is_empty() { String::new() } else { format!(" — {detail}") }
            );
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let line = stdout.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        if line.is_empty() {
            bail!("{} «{program}»: запустилась, но версию не сообщила", tool.name());
        }
        Ok(line.to_string())
    }

    /// Версия или `None`: для проверок «работает ли» без шума об ошибке.
    pub fn probe(&self, tool: Tool) -> Option<String> {
        self.version(tool).ok()
    }
}

/// Поиск имени программы в `PATH`. Явные пути сюда не попадают.
fn search_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            // На Windows `Command` подставляет `.exe` сам, а для показа пути это делаем мы.
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

    #[test]
    fn explicit_path_wins_over_name() {
        let mut t = Tools::default();
        *t.get_mut(Tool::Tesseract) = "/opt/ocr/bin/tesseract".into();
        assert_eq!(t.program(Tool::Tesseract), "/opt/ocr/bin/tesseract");
        assert_eq!(t.source(Tool::Tesseract, Lang::Ru), "задано в настройках");
        // Пробелы вокруг пути не должны ломать запуск.
        *t.get_mut(Tool::Ffmpeg) = "  ffmpeg  ".into();
        assert_eq!(t.program(Tool::Ffmpeg), "ffmpeg");
        assert!(t.configured(Tool::Ffmpeg));
    }

    #[test]
    fn missing_path_is_an_error_not_a_panic() {
        let mut t = Tools::default();
        *t.get_mut(Tool::Curl) = "/nonexistent/photocat-curl".into();
        let err = t.version(Tool::Curl).unwrap_err().to_string();
        assert!(err.contains("curl"), "{err}");
        assert!(err.contains("photocat-curl"), "{err}");
        assert!(t.locate(Tool::Curl).is_none());
    }

    #[test]
    fn every_tool_asks_for_version_the_way_it_understands() {
        assert_eq!(Tool::Ffmpeg.version_args(), &["-version"]);
        assert_eq!(Tool::Ffprobe.version_args(), &["-version"]);
        assert_eq!(Tool::Tesseract.version_args(), &["--version"]);
        assert_eq!(Tool::Curl.version_args(), &["--version"]);
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
        // Файл есть, но это не программа: ошибка запуска, а не тихий успех.
        assert!(t.version(Tool::Ffmpeg).is_err());
    }
}
