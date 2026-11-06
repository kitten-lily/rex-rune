//! Bundled chrome fonts, a fontconfig-resolved terminal family, the user's ordered fallbacks and
//! fontconfig's own fallback chain for the terminal family.
use std::{collections::HashSet, path::PathBuf, process::Command};

pub struct Fonts {
    pub files: &'static [&'static [u8]],
    /// System font files Rune registers by path, without copying them into heap buffers.
    pub paths: &'static [PathBuf],
    pub families: (&'static str, &'static str),
    /// The resolved primary family, for Settings; the grid uses the mono slot's full stack.
    pub terminal: Option<&'static str>,
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn pattern(family: &str) -> String {
    family
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace(',', "\\,")
}

fn css_family(family: &str) -> String {
    format!("'{}'", family.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn bundled(family: &str) -> Option<&'static str> {
    [
        rune_theme_opencode::FAMILIES.0,
        rune_theme_opencode::FAMILIES.1,
    ]
    .into_iter()
    .find(|name| name.eq_ignore_ascii_case(family))
}

fn add_name(names: &mut Vec<String>, name: &str) {
    if !names
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(name))
    {
        names.push(name.to_owned());
    }
}

/// The available faces fontconfig resolves for one requested family, in four styles, without
/// accepting an unrelated substitute. Rune synthesizes a style the family does not provide.
fn resolve_family(family: &str) -> Option<(String, Vec<PathBuf>)> {
    let generic = ["monospace", "sans-serif", "serif"]
        .iter()
        .any(|name| family.eq_ignore_ascii_case(name));
    let mut name = None;
    let mut files = Vec::new();
    for style in ["", ":bold", ":italic", ":bold:italic"] {
        let out = Command::new("fc-match")
            .args([
                "-f",
                "%{family[0]}\n%{file}",
                "--",
                &format!("{}{style}", pattern(family)),
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            continue;
        }
        let out = String::from_utf8_lossy(&out.stdout);
        let Some((found, file)) = out.split_once('\n') else {
            continue;
        };
        if !generic && !found.eq_ignore_ascii_case(family) {
            continue;
        }
        let file = PathBuf::from(file.trim());
        if file.is_file() && !files.contains(&file) {
            name.get_or_insert_with(|| found.to_owned());
            files.push(file);
        }
    }
    Some((name?, files))
}

/// fontconfig's sorted fallback chain for `family` (`fc-match -s`), one `family\tfile` line per
/// font. fontconfig prunes the chain to fonts that add coverage, so it stays short.
fn sorted_fallbacks(family: &str) -> String {
    Command::new("fc-match")
        .args([
            "-s",
            "-f",
            "%{family[0]}\t%{file}\n",
            "--",
            &pattern(family),
        ])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// Appends a sorted chain's families in order, registering each file by path once. Bundled
/// families keep their embedded faces, so IBM Plex Mono stays after the system's fonts.
fn add_sorted(
    chain: &str,
    names: &mut Vec<String>,
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    for line in chain.lines() {
        let Some((family, file)) = line.split_once('\t') else {
            continue;
        };
        let file = PathBuf::from(file.trim());
        if family.is_empty() || bundled(family).is_some() || !file.is_file() {
            continue;
        }
        if seen.insert(file.clone()) {
            paths.push(file);
        }
        add_name(names, family);
    }
}

pub fn load(terminal_family: &str, fallbacks: &[String]) -> Fonts {
    load_with(terminal_family, fallbacks, sorted_fallbacks)
}

/// The terminal stack: the primary, the named fallbacks, the `sorted` chain for the primary,
/// then bundled IBM Plex Mono.
fn load_with(
    terminal_family: &str,
    fallbacks: &[String],
    sorted: impl FnOnce(&str) -> String,
) -> Fonts {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    let mut terminal = None;
    for (index, family) in std::iter::once(terminal_family)
        .chain(fallbacks.iter().map(String::as_str))
        .enumerate()
    {
        let resolved = bundled(family).map(str::to_owned).or_else(|| {
            let (name, files) = resolve_family(family)?;
            for file in files {
                if seen.insert(file.clone()) {
                    paths.push(file);
                }
            }
            Some(name)
        });
        match resolved {
            Some(name) => {
                if index == 0 {
                    terminal = Some(leak(name.clone()));
                }
                add_name(&mut names, &name);
            }
            None => {
                if index == 0 {
                    add_name(&mut names, "IBM Plex Mono");
                }
                eprintln!(
                    "Terminal font {family:?} not found; {}.",
                    if index == 0 {
                        "using IBM Plex Mono"
                    } else {
                        "skipping this fallback"
                    }
                );
            }
        }
    }
    add_sorted(&sorted(terminal_family), &mut names, &mut paths, &mut seen);
    add_name(&mut names, "IBM Plex Mono");
    let mono = names
        .iter()
        .map(|name| css_family(name))
        .collect::<Vec<_>>()
        .join(", ");
    Fonts {
        files: rune_theme_opencode::FONTS,
        paths: Box::leak(paths.into_boxed_slice()),
        families: (rune_theme_opencode::FAMILIES.0, leak(mono)),
        terminal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_stack_keeps_chrome_separate_and_preserves_fallback_order() {
        let fonts = load_with(
            "IBM Plex Mono",
            &["Inter".into(), "ibm plex mono".into()],
            |_| String::new(),
        );
        assert_eq!(fonts.families.0, rune_theme_opencode::FAMILIES.0);
        assert_eq!(fonts.families.1, "'IBM Plex Mono', 'Inter'");
        assert_eq!(fonts.terminal, Some("IBM Plex Mono"));
        assert_eq!(fonts.files, rune_theme_opencode::FONTS);
        assert!(fonts.paths.is_empty(), "bundled faces are not re-read");
    }

    #[test]
    fn fontconfig_chain_follows_the_named_families_once_per_file_before_plex() {
        let dir = std::env::temp_dir().join(format!(
            "rex-font-chain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let (symbols, emoji) = (dir.join("symbols.ttf"), dir.join("emoji.ttf"));
        std::fs::write(&symbols, b"").unwrap();
        std::fs::write(&emoji, b"").unwrap();
        let chain = [
            format!("IBM Plex Mono\t{}", symbols.display()),
            format!("Symbols Nerd Font\t{}", symbols.display()),
            format!("Noto Sans Tamil\t{}", dir.join("missing.ttf").display()),
            "no file".into(),
            format!("symbols nerd font\t{}", symbols.display()),
            format!("Noto Color Emoji\t{}", emoji.display()),
        ]
        .join("\n");
        let mut asked = None;
        let fonts = load_with("Inter", &[], |family| {
            asked = Some(family.to_owned());
            chain
        });
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(asked.as_deref(), Some("Inter"));
        assert_eq!(
            fonts.families.1,
            "'Inter', 'Symbols Nerd Font', 'Noto Color Emoji', 'IBM Plex Mono'"
        );
        assert_eq!(fonts.paths, [symbols, emoji]);
        assert_eq!(fonts.terminal, Some("Inter"));
    }

    #[test]
    fn fontconfig_and_css_names_are_escaped_as_patterns_not_syntax() {
        assert_eq!(pattern(r"A:B,C\D"), r"A\:B\,C\\D");
        assert_eq!(css_family("Joe's Mono"), "'Joe\\'s Mono'");
    }

    #[test]
    fn native_font_paths_shape_from_the_file_without_an_embedded_copy() {
        const PLEX: &[u8] =
            include_bytes!("../../vendor/rune/crates/rune/assets/IBMPlexMono-Regular.ttf");
        let path = std::env::temp_dir().join(format!(
            "rex-font-path-{}-{}.ttf",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, PLEX).unwrap();
        let style = rune::TextStyle::new(13., 400.).mono();
        let mut without_path =
            rune::text::TextSystem::new_with_paths(&[], &[], ("Inter", "IBM Plex Mono"));
        let fallback_width = without_path
            .layout_rich("MMMM", &[], style, 1., None)
            .full_width();
        let mut with_path = rune::text::TextSystem::new_with_paths(
            &[],
            std::slice::from_ref(&path),
            ("Inter", "IBM Plex Mono"),
        );
        let plex_width = with_path
            .layout_rich("MMMM", &[], style, 1., None)
            .full_width();
        std::fs::remove_file(path).unwrap();
        assert!(
            (plex_width - fallback_width).abs() > 0.5,
            "path font was not selected: {plex_width} vs {fallback_width}"
        );
    }

    #[test]
    #[ignore = "requires fontconfig and a CJK font; exercised by the Nix package check"]
    fn fontconfig_cjk_fallback_has_real_glyphs_and_keeps_ascii_metrics() {
        let fonts = load("IBM Plex Mono", &[]);
        let mut ts =
            rune::text::TextSystem::new_with_paths(fonts.files, fonts.paths, fonts.families);
        let style = rune::TextStyle::new(15., 400.).mono();
        let text = rune::text::Run::new("世界 日本語 한글");
        let shaped = ts.run_layout(&text, style, 1.);
        let mut count = 0;
        for line in shaped.layout.lines() {
            for run in line.runs() {
                for cluster in run.visual_clusters() {
                    for glyph in cluster.glyphs() {
                        assert_ne!(
                            glyph.id,
                            0,
                            "missing CJK glyph in {:?}",
                            cluster.text_range()
                        );
                        count += 1;
                    }
                }
            }
        }
        assert!(count >= 9);
        let mut bundled =
            rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
        assert_eq!(
            ts.measure("MMMM 1234", style, 1.),
            bundled.measure("MMMM 1234", style, 1.)
        );
    }
}
