//! Bundled chrome fonts, fontconfig-resolved terminal families and their ordered fallbacks.
use std::{collections::HashSet, path::PathBuf, process::Command, sync::OnceLock};

pub struct Fonts {
    pub files: &'static [&'static [u8]],
    /// System font files Rune registers by path, without copying them into heap buffers.
    pub paths: &'static [PathBuf],
    pub families: (&'static str, &'static str),
    /// The resolved primary terminal family, or `None` when the bundled mono replaced it.
    pub terminal: Option<&'static str>,
}

struct OwnedFonts {
    paths: Vec<PathBuf>,
    mono: String,
    terminal: Option<String>,
}

static OWNED: OnceLock<OwnedFonts> = OnceLock::new();

fn pattern(family: &str) -> String {
    family
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace(',', "\\,")
}

fn css_family_list(names: &[String]) -> String {
    let mut list = String::new();
    for name in names {
        if !list.is_empty() {
            list.push_str(", ");
        }
        list.push('"');
        for ch in name.chars() {
            if matches!(ch, '\\' | '"') {
                list.push('\\');
            }
            list.push(ch);
        }
        list.push('"');
    }
    list
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

fn add_path(paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: &str) -> bool {
    let path = PathBuf::from(path.trim());
    if path.as_os_str().is_empty() || !path.is_file() {
        return false;
    }
    if seen.insert(path.clone()) {
        paths.push(path);
    }
    true
}

/// The available faces fontconfig resolves for one requested family. Missing styles do not
/// reject the usable faces; Rune synthesizes a style when the family does not provide one.
fn resolve_family(family: &str) -> Option<(String, Vec<PathBuf>)> {
    let generic = ["monospace", "sans-serif", "serif"]
        .iter()
        .any(|name| family.eq_ignore_ascii_case(name));
    let mut name = None;
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
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
        if add_path(&mut paths, &mut seen, file) {
            name.get_or_insert_with(|| found.to_owned());
        }
    }
    Some((name?, paths))
}

fn add_requested_family(
    family: &str,
    names: &mut Vec<String>,
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) -> Option<String> {
    if let Some(name) = bundled(family) {
        add_name(names, name);
        return Some(name.to_owned());
    }
    let (name, files) = resolve_family(family)?;
    for path in files {
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    add_name(names, &name);
    Some(name)
}

/// Append fontconfig's full fallback order after the user's explicit families.
fn add_automatic_fallbacks(
    family: &str,
    names: &mut Vec<String>,
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    let Ok(out) = Command::new("fc-match")
        .args([
            "-s",
            "-f",
            "%{family[0]}\t%{file}\n",
            "--",
            &pattern(family),
        ])
        .output()
    else {
        return;
    };
    if !out.status.success() {
        return;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((found, file)) = line.split_once('\t') else {
            continue;
        };
        if !found.is_empty() && add_path(paths, seen, file) {
            add_name(names, found);
        }
    }
}

fn resolve(terminal_family: &str, fallbacks: &[String]) -> OwnedFonts {
    let mut names = Vec::new();
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let terminal = add_requested_family(terminal_family, &mut names, &mut paths, &mut seen)
        .or_else(|| {
            eprintln!("Terminal font {terminal_family:?} not found; using IBM Plex Mono.");
            add_name(&mut names, "IBM Plex Mono");
            None
        });
    for family in fallbacks {
        if add_requested_family(family, &mut names, &mut paths, &mut seen).is_none() {
            eprintln!("Terminal fallback font {family:?} not found; skipping it.");
        }
    }
    add_automatic_fallbacks(terminal_family, &mut names, &mut paths, &mut seen);
    add_name(&mut names, "IBM Plex Mono");
    OwnedFonts {
        paths,
        mono: css_family_list(&names),
        terminal,
    }
}

pub fn load(terminal_family: &str, fallbacks: &[String]) -> Fonts {
    let owned = OWNED.get_or_init(|| resolve(terminal_family, fallbacks));
    Fonts {
        files: rune_theme_opencode::FONTS,
        paths: &owned.paths,
        families: (rune_theme_opencode::FAMILIES.0, &owned.mono),
        terminal: owned.terminal.as_deref(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fontconfig_and_css_names_are_escaped_as_patterns_not_syntax() {
        assert_eq!(pattern(r"A:B,C\D"), r"A\:B\,C\\D");
        assert_eq!(
            css_family_list(&["Joe's Mono".into(), r#"A"B\C"#.into()]),
            r#""Joe's Mono", "A\"B\\C""#
        );
    }

    #[test]
    fn explicit_bundled_fallbacks_keep_their_order() {
        let fonts = resolve("IBM Plex Mono", &["Inter".into(), "ibm plex mono".into()]);
        assert!(
            fonts.mono.starts_with(r#""IBM Plex Mono", "Inter""#),
            "{}",
            fonts.mono
        );
        assert_eq!(fonts.terminal.as_deref(), Some("IBM Plex Mono"));
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
}
