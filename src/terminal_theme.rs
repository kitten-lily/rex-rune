//! Terminal colour themes: the built-in Midnight and Ghostty theme files.
//!
//! A theme is declared to the server with `terminal.set_theme`, which applies it while this
//! client owns a block and sends it back on the block's stream; the client never recolours
//! output itself. Ghostty's cursor text and selection colours have no protocol field and are
//! drawn locally.
use anyhow::{Result, bail, ensure};
use rexrs::api::terminal::SetTheme;
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// Midnight's background: the navy the chrome is drawn in.
pub const MIDNIGHT_BACKGROUND: u32 = 0x001528;

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalTheme {
    /// The Ghostty theme's name; none is the built-in Midnight.
    pub name: Option<String>,
    pub background: u32,
    pub foreground: Option<u32>,
    pub cursor: Option<u32>,
    pub cursor_text: Option<u32>,
    pub selection_background: Option<u32>,
    pub selection_foreground: Option<u32>,
    /// Changes to the 256-colour palette, by index.
    pub palette: Vec<(u8, u32)>,
}

impl TerminalTheme {
    pub fn midnight() -> Self {
        Self {
            name: None,
            background: MIDNIGHT_BACKGROUND,
            foreground: None,
            cursor: None,
            cursor_text: None,
            selection_background: None,
            selection_foreground: None,
            palette: Vec::new(),
        }
    }

    /// A Ghostty theme file's colours. Later lines win, keys other than colours are ignored,
    /// and a colour this parser does not read (an X11 name, `cell-foreground`) leaves its
    /// default in place.
    pub fn parse(name: &str, text: &str) -> Result<Self> {
        let mut theme = Self {
            name: Some(name.to_owned()),
            ..Self::midnight()
        };
        let mut known = false;
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value);
            let slot = match key.trim() {
                "background" => {
                    known = true;
                    if let Some(color) = parse_hex(value) {
                        theme.background = color;
                    }
                    continue;
                }
                "palette" => {
                    known = true;
                    if let Some((index, color)) = value.split_once('=')
                        && let (Ok(index), Some(color)) =
                            (index.trim().parse::<u8>(), parse_hex(color.trim()))
                    {
                        theme.palette.retain(|&(i, _)| i != index);
                        theme.palette.push((index, color));
                    }
                    continue;
                }
                "foreground" => &mut theme.foreground,
                "cursor-color" => &mut theme.cursor,
                "cursor-text" => &mut theme.cursor_text,
                "selection-background" => &mut theme.selection_background,
                "selection-foreground" => &mut theme.selection_foreground,
                _ => continue,
            };
            known = true;
            if let Some(color) = parse_hex(value) {
                *slot = Some(color);
            }
        }
        ensure!(known, "“{name}” is not a Ghostty theme");
        theme.palette.sort_unstable_by_key(|&(i, _)| i);
        Ok(theme)
    }

    /// The `terminal.set_theme` arguments. Midnight says it is dark; a Ghostty theme leaves
    /// that to the server, which judges its background's brightness.
    pub fn args(&self) -> Value {
        let mut set = SetTheme::new().background(hex(self.background));
        if let Some(color) = self.foreground {
            set = set.foreground(hex(color));
        }
        if let Some(color) = self.cursor {
            set = set.cursor(hex(color));
        }
        if !self.palette.is_empty() {
            set = set.palette(
                self.palette
                    .iter()
                    .map(|&(i, color)| format!("{i}={}", hex(color))),
            );
        }
        if self.name.is_none() {
            set = set.scheme("dark");
        }
        serde_json::to_value(set).expect("theme serializes")
    }
}

/// Whether a colour reads as light: perceived brightness above half.
pub fn is_light(color: u32) -> bool {
    let (r, g, b) = ((color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff);
    (299 * r + 587 * g + 114 * b) / 1000 > 128
}

/// Where Ghostty themes are found, in Ghostty's order: the user's own, then Ghostty's.
pub struct ThemeDirs {
    pub user: Option<PathBuf>,
    pub system: Vec<PathBuf>,
}

impl ThemeDirs {
    pub fn from_env() -> Self {
        let var = |key| std::env::var_os(key).filter(|v| !v.is_empty());
        let user = var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".config")))
            .map(|config| config.join("ghostty/themes"));
        let mut system: Vec<PathBuf> = var("GHOSTTY_RESOURCES_DIR")
            .map(|dir| PathBuf::from(dir).join("themes"))
            .into_iter()
            .collect();
        let data = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        system.extend(
            std::env::split_paths(&data)
                .filter(|dir| dir.is_absolute())
                .map(|dir| dir.join("ghostty/themes")),
        );
        Self { user, system }
    }

    fn all(&self) -> impl Iterator<Item = (&Path, bool)> {
        let user = self.user.iter().map(|dir| (dir.as_path(), true));
        user.chain(self.system.iter().map(|dir| (dir.as_path(), false)))
    }

    /// Every readable theme, a name found twice taking the first directory's file, sorted
    /// by name regardless of case.
    pub fn list(&self) -> Vec<ThemeInfo> {
        let mut seen = HashSet::new();
        let mut themes = Vec::new();
        for (dir, custom) in self.all() {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if !valid_name(&name) || seen.contains(&name) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(entry.path()) else {
                    continue;
                };
                if let Ok(theme) = TerminalTheme::parse(&name, &text) {
                    seen.insert(name.clone());
                    themes.push(ThemeInfo {
                        light: is_light(theme.background),
                        name,
                        custom,
                    });
                }
            }
        }
        themes.sort_by_cached_key(|t| t.name.to_lowercase());
        themes
    }

    /// Ghostty's theme by name, from the first directory that has it.
    pub fn load(&self, name: &str) -> Result<TerminalTheme> {
        ensure!(valid_name(name), "“{name}” is not a theme name");
        for (dir, _) in self.all() {
            if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
                return TerminalTheme::parse(name, &text);
            }
        }
        bail!("Theme “{name}” not found")
    }
}

/// One theme the picker offers.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeInfo {
    pub name: String,
    pub light: bool,
    /// Found in the user's own theme directory.
    pub custom: bool,
}

/// A file name in a theme directory: never a path that leaves it.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\'])
}

fn parse_hex(value: &str) -> Option<u32> {
    let digits = value.strip_prefix('#').unwrap_or(value);
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        6 => u32::from_str_radix(digits, 16).ok(),
        3 => {
            let short = u32::from_str_radix(digits, 16).ok()?;
            let (r, g, b) = ((short >> 8) & 0xf, (short >> 4) & 0xf, short & 0xf);
            Some(((r * 0x11) << 16) | ((g * 0x11) << 8) | (b * 0x11))
        }
        _ => None,
    }
}

fn hex(color: u32) -> String {
    format!("#{color:06x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MOCHA: &str = "\
palette = 0=#45475a
palette = 1=#f38ba8
palette = 15=#a6adc8
background = #1e1e2e
foreground = #cdd6f4
cursor-color = #f5e0dc
cursor-text = #1e1e2e
selection-background = #f5e0dc
selection-foreground = #1e1e2e
";

    #[test]
    fn midnight_declares_the_navy_and_dark() {
        assert_eq!(
            TerminalTheme::midnight().args(),
            json!({"background":"#001528","scheme":"dark"})
        );
    }

    #[test]
    fn a_ghostty_theme_becomes_set_theme_arguments() {
        let theme = TerminalTheme::parse("Catppuccin Mocha", MOCHA).unwrap();
        assert_eq!(
            theme.args(),
            json!({
                "background":"#1e1e2e",
                "foreground":"#cdd6f4",
                "cursor":"#f5e0dc",
                "palette":["0=#45475a","1=#f38ba8","15=#a6adc8"],
            })
        );
        assert_eq!(theme.cursor_text, Some(0x1e1e2e));
        assert_eq!(
            (theme.selection_background, theme.selection_foreground),
            (Some(0xf5e0dc), Some(0x1e1e2e))
        );
    }

    #[test]
    fn parsing_follows_ghostty_config_rules() {
        let theme = TerminalTheme::parse(
            "t",
            "# a comment\nbackground= #171219\nforeground = \"eee4ff\"\nfont-size = 13\n\
             palette = 3=#abc\npalette = 3=#010203\npalette = 300=#ffffff\npalette = 4=red\n\
             cursor-color = cell-foreground\n",
        )
        .unwrap();
        assert_eq!(theme.background, 0x171219);
        assert_eq!(theme.foreground, Some(0xeee4ff));
        assert_eq!(theme.palette, [(3, 0x010203)], "later lines win");
        assert_eq!(theme.cursor, None, "unreadable colours keep the default");
        assert!(TerminalTheme::parse("x", "font-size = 13\n").is_err());
        assert_eq!(parse_hex("#abc"), Some(0xaabbcc));
    }

    #[test]
    fn a_users_theme_overrides_ghosttys_and_names_never_leave_the_directory() {
        let root = std::env::temp_dir().join(format!("rex-linux-themes-{}", std::process::id()));
        let (user, system) = (root.join("user"), root.join("system"));
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(user.join("Shared"), "background = #ffffff\n").unwrap();
        std::fs::write(system.join("Shared"), "background = #000000\n").unwrap();
        std::fs::write(system.join("only system"), "background = #101010\n").unwrap();
        std::fs::write(system.join("broken"), "font-size = 13\n").unwrap();
        std::fs::write(root.join("outside"), "background = #123456\n").unwrap();
        let dirs = ThemeDirs {
            user: Some(user.clone()),
            system: vec![system.clone(), root.join("missing")],
        };

        assert_eq!(dirs.load("Shared").unwrap().background, 0xffffff);
        assert_eq!(
            dirs.list(),
            [
                ThemeInfo {
                    name: "only system".into(),
                    light: false,
                    custom: false
                },
                ThemeInfo {
                    name: "Shared".into(),
                    light: true,
                    custom: true
                },
            ]
        );
        assert!(dirs.load("../outside").is_err());
        assert!(dirs.load("missing").is_err());
        std::fs::remove_dir_all(root).ok();
    }
}
