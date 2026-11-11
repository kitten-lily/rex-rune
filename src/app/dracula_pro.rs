//! Chrome palettes for the generated Dracula Pro themes, built once from their tokens.
use super::theme::{MIDNIGHT, Palette};
use rex_linux::dracula_pro::THEMES;
use std::sync::LazyLock;

static PALETTES: LazyLock<Vec<Option<Palette>>> = LazyLock::new(|| {
    THEMES
        .iter()
        .map(|t| t.chrome.as_ref().map(Palette::from_tokens))
        .collect()
});

/// The palette of `THEMES[i]`. A theme without a chrome, or an index out of range, draws as
/// Midnight.
pub fn palette(i: u8) -> &'static Palette {
    PALETTES
        .get(i as usize)
        .and_then(Option::as_ref)
        .unwrap_or(&MIDNIGHT)
}

#[cfg(test)]
mod tests {
    use rex_linux::dracula_pro::derive;

    #[test]
    fn the_build_checks_use_the_apps_fills() {
        use super::super::theme::{KEYCAP_FILL, KEYCAP_LIT, SELECTION_FILL};
        assert_eq!(derive::KEYCAP_FILL, KEYCAP_FILL);
        assert_eq!(derive::KEYCAP_LIT, KEYCAP_LIT);
        assert_eq!(derive::SELECTION_FILL, SELECTION_FILL);
    }
}
