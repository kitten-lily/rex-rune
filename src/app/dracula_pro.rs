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
