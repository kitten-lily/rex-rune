//! Generates the Dracula Pro theme tables from a user's local Ghostty checkout.
//! `build.rs` includes this file by path; so does `src/dracula_pro.rs` for its tests.

use super::derive::{self, ChromeTokens, Source};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Where the checkout lives, or `None` when Dracula Pro is switched off.
pub fn source_dir(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let var = |name: &str| var(name).filter(|v| !v.is_empty());
    if var("REX_DRACULA_PRO").as_deref() == Some("0") {
        return None;
    }
    if let Some(dir) = var("REX_DRACULA_PRO_DIR") {
        return Some(PathBuf::from(dir));
    }
    let data = var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| var("HOME").map(|home| Path::new(&home).join(".local/share")))?;
    Some(data.join("rex-linux/dracula-pro-ghostty"))
}

pub struct Output {
    pub code: String,
    pub themes: usize,
    pub chromes: usize,
    pub warnings: Vec<String>,
    /// Existing paths whose changes should rerun the build.
    pub watch: Vec<PathBuf>,
}

fn source_code(s: &Source) -> String {
    let option = |v: Option<u32>| match v {
        Some(v) => format!("Some({v:#08x})"),
        None => "None".to_owned(),
    };
    let palette = s
        .palette
        .iter()
        .map(|c| format!("{c:#08x}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "crate::dracula_pro::derive::Source {{ background: {:#08x}, foreground: {:#08x}, \
         cursor: {:#08x}, selection_background: {:#08x}, palette: [{palette}], \
         cursor_text: {}, selection_foreground: {} }}",
        s.background,
        s.foreground,
        s.cursor,
        s.selection_background,
        option(s.cursor_text),
        option(s.selection_foreground),
    )
}

fn chrome_code(c: &ChromeTokens) -> String {
    format!(
        "Some(crate::dracula_pro::derive::ChromeTokens {{ ground: {:#08x}, popover: {:#08x}, \
         ink: {:#08x}, muted: {:#08x}, accent: {:#08x}, accent2: {:#08x}, live: {:#08x}, \
         warning: {:#08x}, danger: {:#08x}, on_accent: {:#08x}, terminal_selection: {:#08x} }})",
        c.ground,
        c.popover,
        c.ink,
        c.muted,
        c.accent,
        c.accent2,
        c.live,
        c.warning,
        c.danger,
        c.on_accent,
        c.terminal_selection,
    )
}

/// Reads the known theme files in `dir` and writes the `THEMES` table. A missing, partial or
/// bad checkout gives fewer themes and warnings, never an error.
pub fn generate(dir: Option<&Path>) -> Output {
    let mut out = Output {
        code: String::new(),
        themes: 0,
        chromes: 0,
        warnings: Vec::new(),
        watch: Vec::new(),
    };
    let mut entries = String::new();
    let mut builtins = String::new();
    if let Some(dir) = dir {
        if dir.is_dir() {
            out.watch.push(dir.to_owned());
        } else if let Some(parent) = dir.parent().filter(|p| p.is_dir()) {
            out.watch.push(parent.to_owned());
        }
        for &(file, id, name) in derive::KNOWN {
            let path = dir.join(file);
            if !path.is_file() {
                continue;
            }
            out.watch.push(path.clone());
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(_) => {
                    out.warnings.push(format!("{file}: unreadable, skipped"));
                    continue;
                }
            };
            let source = match derive::parse(&text) {
                Ok(source) => source,
                Err(error) => {
                    out.warnings.push(format!("{file}: {error}, skipped"));
                    continue;
                }
            };
            let chrome = if derive::is_light(source.background) {
                None
            } else {
                match derive::chrome(&source) {
                    Ok(tokens) => Some(tokens),
                    Err(error) => {
                        out.warnings
                            .push(format!("{file}: {error}, terminal theme only"));
                        None
                    }
                }
            };
            let index = out.themes;
            out.themes += 1;
            out.chromes += usize::from(chrome.is_some());
            let _ = write!(
                entries,
                "Theme {{ id: {id:?}, name: {name:?}, source: {}, chrome: {} }}, ",
                source_code(&source),
                chrome.as_ref().map_or("None".to_owned(), chrome_code),
            );
            let paired = if chrome.is_some() {
                format!("Some(crate::settings::ChromeTheme::Generated({index}))")
            } else {
                "None".to_owned()
            };
            let _ = write!(
                builtins,
                "crate::terminal_theme::Builtin {{ id: {id:?}, name: {name:?}, chrome: {paired}, theme: || crate::dracula_pro::terminal(&crate::dracula_pro::THEMES[{index}]) }}, ",
            );
        }
    }
    out.code = format!(
        "pub static THEMES: &[Theme] = &[{}];\n\
         pub static BUILTINS: &[crate::terminal_theme::Builtin] = &[{}];\n",
        entries.trim_end(),
        builtins.trim_end(),
    );
    out
}
