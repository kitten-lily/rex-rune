//! Fonts: the kit's bundled Inter and IBM Plex Mono for chrome, plus the user's terminal font
//! and its fontconfig fallback chain.
use std::{path::PathBuf, process::Command, sync::OnceLock};

pub struct Fonts {
    pub files: &'static [&'static [u8]],
    pub families: (&'static str, &'static str),
    /// The primary terminal family, or `None` for the bundled mono.
    pub terminal: Option<&'static str>,
}

struct Resolved {
    primary: String,
    families: String,
    files: Vec<Vec<u8>>,
}

struct OwnedFonts {
    files: Vec<Box<[u8]>>,
    mono: String,
    terminal: Option<String>,
}

static OWNED: OnceLock<OwnedFonts> = OnceLock::new();
static FILES: OnceLock<Vec<&'static [u8]>> = OnceLock::new();

fn load_file(path: &str, seen: &mut Vec<PathBuf>, files: &mut Vec<Vec<u8>>) -> bool {
    let path = PathBuf::from(path.trim());
    if path.as_os_str().is_empty() {
        return false;
    }
    if seen.contains(&path) {
        return true;
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return false;
    };
    seen.push(path);
    files.push(bytes);
    true
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

/// Every face fontconfig resolves for `family`, followed by its ordered fallback chain.
/// Returns nothing when fontconfig substitutes another primary family.
fn resolve(family: &str) -> Option<Resolved> {
    let mut primary = None;
    let mut files = Vec::new();
    let mut seen = Vec::new();
    for style in ["", ":bold", ":italic", ":bold:italic"] {
        let out = Command::new("fc-match")
            .args(["-f", "%{family[0]}\n%{file}", &format!("{family}{style}")])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let out = String::from_utf8_lossy(&out.stdout);
        let (found, file) = out.split_once('\n')?;
        if !found.eq_ignore_ascii_case(family) {
            return None;
        }
        primary.get_or_insert_with(|| found.to_owned());
        load_file(file, &mut seen, &mut files);
    }
    if files.is_empty() {
        return None;
    }

    let primary = primary?;
    let mut families = vec![primary.clone()];
    if let Ok(out) = Command::new("fc-match")
        .args(["-s", "-f", "%{family[0]}\t%{file}\n", family])
        .output()
        && out.status.success()
    {
        let out = String::from_utf8_lossy(&out.stdout);
        for line in out.lines() {
            let Some((found, file)) = line.split_once('\t') else {
                continue;
            };
            if !found.is_empty()
                && load_file(file, &mut seen, &mut files)
                && !families.iter().any(|name| name.eq_ignore_ascii_case(found))
            {
                families.push(found.to_owned());
            }
        }
    }
    if !families
        .iter()
        .any(|name| name.eq_ignore_ascii_case("IBM Plex Mono"))
    {
        families.push("IBM Plex Mono".to_owned());
    }

    Some(Resolved {
        primary,
        families: css_family_list(&families),
        files,
    })
}

pub fn load(terminal_family: &str) -> Fonts {
    let owned = OWNED.get_or_init(|| match resolve(terminal_family) {
        Some(resolved) => OwnedFonts {
            files: resolved
                .files
                .into_iter()
                .map(Vec::into_boxed_slice)
                .collect(),
            mono: resolved.families,
            terminal: Some(resolved.primary),
        },
        None => {
            eprintln!("Terminal font {terminal_family:?} not found; using IBM Plex Mono.");
            OwnedFonts {
                files: Vec::new(),
                mono: rune_theme_opencode::FAMILIES.1.to_owned(),
                terminal: None,
            }
        }
    });
    let files = FILES.get_or_init(|| {
        rune_theme_opencode::FONTS
            .iter()
            .copied()
            .chain(owned.files.iter().map(|file| file.as_ref()))
            .collect()
    });
    Fonts {
        files,
        families: (rune_theme_opencode::FAMILIES.0, &owned.mono),
        terminal: owned.terminal.as_deref(),
    }
}
