//! Colour derivation for Dracula Pro. Dependency-free: `build.rs` includes this file by path.

/// Ghostty theme file, generated theme id and display name.
pub const KNOWN: &[(&str, &str, &str)] = &[
    ("pro", "dracula-pro", "Dracula Pro"),
    ("blade", "dracula-pro-blade", "Dracula Pro Blade"),
    ("buffy", "dracula-pro-buffy", "Dracula Pro Buffy"),
    ("lincoln", "dracula-pro-lincoln", "Dracula Pro Lincoln"),
    ("morbius", "dracula-pro-morbius", "Dracula Pro Morbius"),
    (
        "nosferatu",
        "dracula-pro-nosferatu",
        "Dracula Pro Nosferatu",
    ),
    (
        "van-helsing",
        "dracula-pro-van-helsing",
        "Dracula Pro Van Helsing",
    ),
    ("alucard", "dracula-pro-alucard", "Dracula Pro Alucard"),
];

/// The colours of a Ghostty theme file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub background: u32,
    pub foreground: u32,
    pub cursor: u32,
    pub selection_background: u32,
    pub palette: [u32; 16],
    pub cursor_text: Option<u32>,
    pub selection_foreground: Option<u32>,
}

/// Chrome colours derived from a [`Source`]. The card is always `ground`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeTokens {
    pub ground: u32,
    pub popover: u32,
    pub ink: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent2: u32,
    pub live: u32,
    pub warning: u32,
    pub danger: u32,
    pub on_accent: u32,
    pub terminal_selection: u32,
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

/// Parses a Ghostty theme file. Errors name the first missing or malformed required key,
/// never a colour value.
pub fn parse(text: &str) -> Result<Source, String> {
    // Each slot is None (absent) or Some(None) (present but unparsable).
    let mut background = None;
    let mut foreground = None;
    let mut cursor = None;
    let mut selection_background = None;
    let mut cursor_text = None;
    let mut selection_foreground = None;
    let mut palette: [Option<Option<u32>>; 16] = [None; 16];
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
            "palette" => {
                if let Some((index, color)) = value.split_once('=')
                    && let Ok(index) = index.trim().parse::<usize>()
                    && index < 16
                {
                    palette[index] = Some(parse_hex(color.trim()));
                }
                continue;
            }
            "background" => &mut background,
            "foreground" => &mut foreground,
            "cursor-color" => &mut cursor,
            "selection-background" => &mut selection_background,
            "cursor-text" => &mut cursor_text,
            "selection-foreground" => &mut selection_foreground,
            _ => continue,
        };
        *slot = Some(parse_hex(value));
    }
    let required = |slot: Option<Option<u32>>, name: &str| match slot {
        None => Err(format!("missing {name}")),
        Some(None) => Err(format!("bad {name}")),
        Some(Some(color)) => Ok(color),
    };
    let background = required(background, "background")?;
    let foreground = required(foreground, "foreground")?;
    let cursor = required(cursor, "cursor-color")?;
    let selection_background = required(selection_background, "selection-background")?;
    let mut colors = [0; 16];
    for (i, slot) in palette.into_iter().enumerate() {
        colors[i] = required(slot, &format!("palette {i}"))?;
    }
    Ok(Source {
        background,
        foreground,
        cursor,
        selection_background,
        palette: colors,
        cursor_text: cursor_text.flatten(),
        selection_foreground: selection_foreground.flatten(),
    })
}

/// `a` over `b` at `t`.
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let channel = |shift: u32| {
        let (x, y) = (((a >> shift) & 0xff) as f32, ((b >> shift) & 0xff) as f32);
        ((x * t + y * (1. - t)).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// WCAG 2 contrast ratio of two sRGB colours.
pub fn contrast(a: u32, b: u32) -> f64 {
    let luminance = |c: u32| {
        let channel = |shift: u32| {
            let v = ((c >> shift) & 0xff) as f64 / 255.;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    };
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Hue in degrees.
pub fn hue(c: u32) -> f64 {
    let [r, g, b] = [16, 8, 0].map(|shift| ((c >> shift) & 0xff) as f64 / 255.);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = if d == 0. {
        0.
    } else if max == r {
        ((g - b) / d).rem_euclid(6.)
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    h * 60.
}

/// Whether a colour reads as light: perceived brightness above half.
pub fn is_light(color: u32) -> bool {
    let (r, g, b) = ((color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff);
    (299 * r + 587 * g + 114 * b) / 1000 > 128
}

/// The chrome palette for a dark source, or why it can't have one.
pub fn chrome(s: &Source) -> Result<ChromeTokens, String> {
    if is_light(s.background) {
        return Err("light background".into());
    }
    let ground = s.background;
    let ink = s.foreground;
    let popover = mix(ink, ground, 0.06);
    let muted = (0..=20)
        .map(|step| mix(ink, s.cursor, step as f32 * 0.05))
        .find(|&c| contrast(c, ground) >= 4.5)
        .ok_or("no readable muted colour")?;
    let (warning, danger) = (s.palette[3], s.palette[1]);
    if contrast(ink, popover) < 4.5 {
        return Err("foreground too faint on popover".into());
    }
    for (name, c) in [("warning", warning), ("danger", danger)] {
        if contrast(c, ground) < 3. || contrast(c, popover) < 3. {
            return Err(format!("{name} too faint"));
        }
    }
    let gap = (hue(warning) - hue(danger)).abs();
    if gap.min(360. - gap) < 30. {
        return Err("warning and danger hues too close".into());
    }
    Ok(ChromeTokens {
        ground,
        popover,
        ink,
        muted,
        accent: s.palette[4],
        accent2: s.palette[5],
        live: s.palette[2],
        warning,
        danger,
        on_accent: ground,
        terminal_selection: s.selection_background,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARK: &str = "\
background = #101018
foreground = #f0f0f0
cursor-color = #505070
selection-background = #303060
palette = 0=#202030
palette = 1=#ff6060
palette = 2=#60ff60
palette = 3=#ffff60
palette = 4=#8060ff
palette = 5=#ff60c0
palette = 6=#60e0e0
palette = 7=#d0d0d8
palette = 8=#404058
palette = 9=#ff8080
palette = 10=#80ff80
palette = 11=#ffff80
palette = 12=#a080ff
palette = 13=#ff80d0
palette = 14=#80f0f0
palette = 15=#f8f8f8
";

    fn dark() -> Source {
        parse(DARK).unwrap()
    }

    #[test]
    fn parse_reads_every_key() {
        let text = format!(
            "# comment\n{DARK}cursor-text = cell-foreground\nselection-foreground = \"#fff\"\nbackground = 202028\nbackground = #111111\n"
        );
        let s = parse(&text).unwrap();
        assert_eq!(s.background, 0x111111);
        assert_eq!(s.foreground, 0xf0f0f0);
        assert_eq!(s.cursor, 0x505070);
        assert_eq!(s.selection_background, 0x303060);
        assert_eq!(s.selection_foreground, Some(0xffffff));
        assert_eq!(s.cursor_text, None);
        assert_eq!(s.palette[1], 0xff6060);
        assert_eq!(s.palette[15], 0xf8f8f8);
        let s = parse(&format!("{DARK}cursor-text = #123456\n")).unwrap();
        assert_eq!(s.cursor_text, Some(0x123456));
    }

    #[test]
    fn parse_rejects_a_missing_or_bad_required_key() {
        let no_seven = DARK.replace("palette = 7=#d0d0d8\n", "");
        let err = parse(&no_seven).unwrap_err();
        assert!(err.contains("palette 7"), "{err}");
        assert!(!err.contains('#'));
        let err = parse(&format!("{DARK}background = #zzzzzz\n")).unwrap_err();
        assert!(err.contains("background"), "{err}");
        assert!(!err.contains('#'));
    }

    #[test]
    fn chrome_maps_the_documented_roles() {
        let s = dark();
        let c = chrome(&s).unwrap();
        assert_eq!(c.ground, s.background);
        assert_eq!(c.ink, s.foreground);
        assert_eq!(c.on_accent, s.background);
        assert_eq!(c.accent, s.palette[4]);
        assert_eq!(c.accent2, s.palette[5]);
        assert_eq!(c.live, s.palette[2]);
        assert_eq!(c.warning, s.palette[3]);
        assert_eq!(c.danger, s.palette[1]);
        assert_eq!(c.terminal_selection, s.selection_background);
        assert_eq!(c.popover, mix(s.foreground, s.background, 0.06));
    }

    #[test]
    fn muted_is_raised_to_aa() {
        let s = dark();
        let c = chrome(&s).unwrap();
        assert!(contrast(c.muted, c.ground) >= 4.5);
        assert_ne!(c.muted, s.cursor);
        let readable = Source {
            cursor: 0x9090a0,
            ..s
        };
        assert!(contrast(readable.cursor, readable.background) >= 4.5);
        assert_eq!(chrome(&readable).unwrap().muted, readable.cursor);
    }

    #[test]
    fn chrome_rejects_an_illegible_or_light_palette() {
        let mut s = dark();
        s.palette[3] = 0xff6070; // within 30 degrees of danger
        assert!(chrome(&s).is_err());
        let light = Source {
            background: 0xf5f5f5,
            ..dark()
        };
        assert!(chrome(&light).is_err());
    }

    #[test]
    fn contrast_of_white_on_black_is_21() {
        assert!((contrast(0xffffff, 0x000000) - 21.).abs() < 1e-9);
    }

    #[test]
    fn the_fixture_parses_and_pro_and_blade_have_chromes() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dracula-pro/");
        let load = |name: &str| parse(&std::fs::read_to_string(format!("{dir}{name}")).unwrap());
        let (pro, blade, alucard) = (load("pro"), load("blade"), load("alucard"));
        assert!(chrome(&pro.unwrap()).is_ok());
        assert!(chrome(&blade.unwrap()).is_ok());
        assert!(chrome(&alucard.unwrap()).is_err());
    }
}
