//! Dracula Pro themes built from a user's local, licensed Ghostty checkout.

// Parts of `derive` are used only by `build.rs` (which includes the file by path) or tests.
#[allow(dead_code)]
pub mod derive;

use crate::terminal_theme::TerminalTheme;

/// A Dracula Pro theme read from the local checkout at build time.
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub source: derive::Source,
    /// `None` for light themes and for dark ones whose colours can't be made legible.
    pub chrome: Option<derive::ChromeTokens>,
}

include!(concat!(env!("OUT_DIR"), "/dracula_pro.rs"));

/// The terminal theme for `t`: every colour straight from its source file.
pub fn terminal(t: &Theme) -> TerminalTheme {
    let s = &t.source;
    TerminalTheme {
        background: Some(s.background),
        foreground: Some(s.foreground),
        cursor: Some(s.cursor),
        cursor_text: s.cursor_text,
        selection_background: Some(s.selection_background),
        selection_foreground: s.selection_foreground,
        palette: s
            .palette
            .iter()
            .enumerate()
            .map(|(i, &c)| (i as u8, c))
            .collect(),
        scheme: Some(if derive::is_light(s.background) {
            "light"
        } else {
            "dark"
        }),
    }
}

/// The `THEMES` index of the theme with a chrome palette whose setting id is `setting_id`.
pub fn chrome_index(setting_id: &str) -> Option<u8> {
    THEMES
        .iter()
        .position(|t| t.chrome.is_some() && t.id.replace('-', "_") == setting_id)
        .map(|i| i as u8)
}

#[cfg(test)]
#[path = "../build/dracula_pro.rs"]
mod generate;

#[cfg(test)]
mod generate_tests {
    use super::generate::{generate, source_dir};
    use std::path::{Path, PathBuf};

    fn fixture(file: &str) -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/dracula-pro")
                .join(file),
        )
        .unwrap()
    }

    /// A temp checkout holding the given files, removed on drop.
    struct Checkout(PathBuf);

    impl Checkout {
        fn new(name: &str, files: &[(&str, &str)]) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("rex-dracula-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            for (file, text) in files {
                std::fs::write(dir.join(file), text).unwrap();
            }
            Self(dir)
        }
    }

    impl Drop for Checkout {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_directory_generates_empty_tables() {
        let out = generate(None);
        assert_eq!((out.themes, out.chromes), (0, 0));
        assert!(out.code.contains("THEMES: &[Theme] = &[]"));
        assert!(out.watch.is_empty());
    }

    #[test]
    fn known_files_become_themes_in_order() {
        let (pro, blade) = (fixture("pro"), fixture("blade"));
        let c = Checkout::new("order", &[("blade", &blade), ("pro", &pro)]);
        let out = generate(Some(&c.0));
        assert_eq!((out.themes, out.chromes), (2, 2));
        let first = out.code.find("\"dracula-pro\"").unwrap();
        let second = out.code.find("\"dracula-pro-blade\"").unwrap();
        assert!(first < second);
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn a_light_file_is_terminal_only() {
        let c = Checkout::new("light", &[("alucard", &fixture("alucard"))]);
        let out = generate(Some(&c.0));
        assert_eq!((out.themes, out.chromes), (1, 0));
        assert!(out.code.contains("chrome: None"));
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn an_incomplete_file_is_skipped_with_a_warning() {
        let text: String = fixture("pro")
            .lines()
            .filter(|l| !l.starts_with("palette = 3="))
            .map(|l| format!("{l}\n"))
            .collect();
        let c = Checkout::new("incomplete", &[("pro", &text)]);
        let out = generate(Some(&c.0));
        assert_eq!(out.themes, 0);
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].contains("pro") && out.warnings[0].contains("palette 3"));
    }

    #[test]
    fn an_illegible_dark_file_is_terminal_only_with_a_warning() {
        // Foreground nearly equal to the background: no legible chrome.
        let text: String = fixture("pro")
            .lines()
            .map(|l| {
                if l.starts_with("foreground") {
                    "foreground = #121219".to_owned()
                } else {
                    l.to_owned()
                }
            })
            .map(|l| format!("{l}\n"))
            .collect();
        let c = Checkout::new("illegible", &[("pro", &text)]);
        let out = generate(Some(&c.0));
        assert_eq!((out.themes, out.chromes), (1, 0));
        assert_eq!(out.warnings.len(), 1);
    }

    #[test]
    fn unknown_files_are_ignored() {
        let c = Checkout::new("unknown", &[("dracula-plus", &fixture("pro"))]);
        let out = generate(Some(&c.0));
        assert_eq!(out.themes, 0);
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn source_dir_resolution() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            source_dir(env(&[
                ("REX_DRACULA_PRO", "0"),
                ("REX_DRACULA_PRO_DIR", "/x")
            ])),
            None
        );
        assert_eq!(
            source_dir(env(&[
                ("REX_DRACULA_PRO_DIR", "/override"),
                ("XDG_DATA_HOME", "/data"),
                ("HOME", "/home/u"),
            ])),
            Some(PathBuf::from("/override"))
        );
        assert_eq!(
            source_dir(env(&[("XDG_DATA_HOME", "/data"), ("HOME", "/home/u")])),
            Some(PathBuf::from("/data/rex-linux/dracula-pro-ghostty"))
        );
        assert_eq!(
            source_dir(env(&[("XDG_DATA_HOME", "relative"), ("HOME", "/home/u")])),
            Some(PathBuf::from(
                "/home/u/.local/share/rex-linux/dracula-pro-ghostty"
            ))
        );
        assert_eq!(
            source_dir(env(&[("REX_DRACULA_PRO_DIR", ""), ("HOME", "/home/u")])),
            Some(PathBuf::from(
                "/home/u/.local/share/rex-linux/dracula-pro-ghostty"
            ))
        );
        assert_eq!(source_dir(env(&[])), None);
    }

    #[test]
    fn watch_lists_only_existing_paths() {
        let c = Checkout::new("watch", &[("pro", &fixture("pro"))]);
        let out = generate(Some(&c.0));
        assert_eq!(out.watch, [c.0.clone(), c.0.join("pro")]);

        let missing = c.0.join("missing");
        assert_eq!(generate(Some(&missing)).watch, std::slice::from_ref(&c.0));

        let nowhere = c.0.join("a/b");
        assert!(generate(Some(&nowhere)).watch.is_empty());
    }

    #[test]
    fn watch_covers_a_checkout_created_later() {
        let c = Checkout::new("later", &[]);
        let dir = c.0.join("checkout");
        let mtime = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
        assert_eq!(generate(Some(&dir)).watch, std::slice::from_ref(&c.0));
        let before = mtime(&c.0);
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::create_dir(&dir).unwrap();
        assert_ne!(mtime(&c.0), before);
    }
}
