# Dracula Pro Build-Time Themes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a local checkout of the private `dracula-pro/ghostty` repo exists, compile its themes in as built-in chrome palettes plus paired terminal themes (Alucard as terminal-only), without ever committing or distributing them.

**Architecture:**
- A mise task clones or updates the repo into `~/.local/share/rex-linux/dracula-pro-ghostty`.
- `build.rs` reads the known theme files and derives chrome tokens with a dependency-free module it shares with the library (`src/dracula_pro/derive.rs`). It writes literal tables to `$OUT_DIR/dracula_pro.rs`, which `src/dracula_pro.rs` includes.
- `ChromeTheme` gains `Generated(u8)` for these palettes, and `Unavailable(&'static str)` so an unknown saved id survives every settings save.
- A committed **synthetic** fixture checkout (made-up colours) exercises the whole generated path in tests, so nothing depends on the licensed files except one final manual check.

**Tech Stack:** Rust 1.96, Cargo build script, Rune/rune-kit UI, mise tasks, bash, Python 3 scripts.

**Spec:** `docs/superpowers/specs/2026-11-11-dracula-pro-themes-design.md`

## Global Constraints

- No Dracula Pro file, colour value or generated code is committed, packaged or published. Tests and fixtures use **made-up** colours only; never copy a value from the real files into the repo, a test, a warning, a commit message or a PR. `derive` error and warning strings name keys and rules, never colour values.
- `build.rs` never touches the network. A missing, partial or bad checkout never fails the build; it produces `cargo:warning`s and fewer (or no) themes.
- Fetch location: `${XDG_DATA_HOME:-$HOME/.local/share}/rex-linux/dracula-pro-ghostty`, overridable with `REX_DRACULA_PRO_DIR`. `REX_DRACULA_PRO=0` disables generation **and** the fetch.
- Known files → ids → names, in this order:
  - `pro` → `dracula-pro` → "Dracula Pro"
  - `blade` → `dracula-pro-blade` → "Dracula Pro Blade"
  - `buffy` → `dracula-pro-buffy` → "Dracula Pro Buffy"
  - `lincoln` → `dracula-pro-lincoln` → "Dracula Pro Lincoln"
  - `morbius` → `dracula-pro-morbius` → "Dracula Pro Morbius"
  - `nosferatu` → `dracula-pro-nosferatu` → "Dracula Pro Nosferatu"
  - `van-helsing` → `dracula-pro-van-helsing` → "Dracula Pro Van Helsing"
  - `alucard` → `dracula-pro-alucard` → "Dracula Pro Alucard"

  Any other file is ignored.
- Terminal theme setting id: `rex/<id>`. Chrome setting id: `<id>` with `-` → `_` (e.g. `dracula_pro_blade`).
- Fork-only: new logic lives in new files; shared files get minimal hooks; existing upstream tests change only where the plan says so. Doc additions are separate short paragraphs.
- Clean in **both** configurations: no checkout, and `REX_DRACULA_PRO_DIR=$PWD/tests/fixtures/dracula-pro` (the synthetic fixture). Each needs `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` and `cargo test --locked --all-features`. "Run both" below means exactly this pair.
- The release marker string is exactly `Includes local Dracula Pro themes`.

## Review Focus

1. **A settings file naming an unknown chrome, followed by any routine save:** the id must still be in the file afterwards, and the window draws Midnight. Pinned in Task 3 (`an_unknown_chrome_survives_a_save`) and Task 5 (`a_routine_save_keeps_an_unavailable_chrome`), both with an id that can never be generated, so they also run on the licensee's machine.
2. **Offline or unauthenticated `mise run build` with an existing checkout:** finishes successfully with a warning, and the checkout is untouched. Pinned in Task 6 (`test_unreachable_remote_warns_and_keeps_checkout`, where the checkout's own remote is unreachable).
3. **A theme file that is missing a key or has a malformed colour:** that theme is skipped with a warning, and the rest generate. Pinned in Task 2 (`an_incomplete_file_is_skipped_with_a_warning`).
4. **First fetch after builds without any checkout:** the next `cargo build` picks the themes up with no source change. Pinned in Task 2 (`watch_covers_a_checkout_created_later`); the mise build always invokes Cargo (Task 6).
5. **A saved `rex/dracula-pro-…` terminal theme on a build without it:** resolves to Follow chrome with no error at startup. Pinned in Task 4 (`an_unavailable_dracula_pro_theme_follows_the_chrome`).

---

### Task 1: Dependency-free derivation module and synthetic fixture

**Files:**
- Create: `src/dracula_pro/derive.rs`, `src/dracula_pro.rs` (module shell), `tests/fixtures/dracula-pro/{pro,blade,alucard,README.md}`
- Modify: `src/lib.rs` (`pub mod dracula_pro;`)

**Interfaces:**
- Produces (`derive.rs`; no `crate::` or external imports, so `build.rs` can `#[path]`-include it):
  - `pub const KNOWN: &[(&str, &str, &str)]`: (file, id, name), the eight entries from Global Constraints, in that order.
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Source`:
    - required: `background`, `foreground`, `cursor`, `selection_background` (each `u32`) and `palette: [u32; 16]`;
    - optional: `cursor_text: Option<u32>` and `selection_foreground: Option<u32>`.
  - `pub fn parse(text: &str) -> Result<Source, String>`:
    - same line syntax as `TerminalTheme::parse`: `key = value`, `#` comments, optional quotes, `#rrggbb`/`rrggbb`/`#rgb`, `palette = N=#hex`, later lines win;
    - the error names the first missing or malformed **required** key, e.g. `"missing palette 7"` or `"bad background"`;
    - an optional key with an unparsable value (for example `cell-foreground`) is `None`.
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct ChromeTokens { pub ground, pub popover, pub ink, pub muted, pub accent, pub accent2, pub live, pub warning, pub danger, pub on_accent, pub terminal_selection: u32 }`. The card is always `ground`.
  - `pub fn chrome(s: &Source) -> Result<ChromeTokens, String>`
  - `pub fn mix(a: u32, b: u32, t: f32) -> u32`: `a` over `b` at `t`, the same formula as `app/theme.rs::mix`.
  - `pub fn contrast(a: u32, b: u32) -> f64` (WCAG), `pub fn hue(c: u32) -> f64` (degrees), `pub fn is_light(c: u32) -> bool` (the `terminal_theme::is_light` formula).
- Fixture: `tests/fixtures/dracula-pro/`
  - `pro` and `blade`: dark, legible, made-up colours.
  - `alucard`: light background `#f5f5f5`.
  - `README.md`: says the colours are invented, and why they exist.

- [ ] **Step 1: Write failing tests** in `derive.rs` `#[cfg(test)] mod tests`. Use a made-up dark source:
  - background `#101018`, foreground `#f0f0f0`, cursor `#505070`;
  - palette 1 `#ff6060`, 2 `#60ff60`, 3 `#ffff60`, 4 `#8060ff`, 5 `#ff60c0`, the rest any valid colour.

  Tests:
  - `parse_reads_every_key`: every field matches; a later duplicate line wins; `cursor-text = cell-foreground` gives `cursor_text == None` with `Ok`.
  - `parse_rejects_a_missing_or_bad_required_key`:
    - without `palette = 7=…`, `Err` contains `"palette 7"`;
    - with `background = #zzzzzz`, `Err` contains `"background"`;
    - the `Err` text contains no `#`.
  - `chrome_maps_the_documented_roles`:
    - `ground == background`, `ink == foreground`, `on_accent == background`;
    - `accent == palette[4]`, `accent2 == palette[5]`, `live == palette[2]`, `warning == palette[3]`, `danger == palette[1]`;
    - `terminal_selection == selection_background`;
    - `popover == mix(foreground, background, 0.06)`.
  - `muted_is_raised_to_aa`: with cursor `#505070`, `contrast(muted, ground) >= 4.5` and `muted != cursor`; with a cursor already ≥ 4.5:1, `muted == cursor`.
  - `chrome_rejects_an_illegible_or_light_palette`: warning and danger with hues less than 30° apart give `Err`, and so does background `#f5f5f5`.
  - `contrast_of_white_on_black_is_21`.
  - `the_fixture_parses_and_pro_and_blade_have_chromes`: `include_str!` the three fixture files; all parse; `chrome` is `Ok` for pro and blade and `Err` for alucard.
- [ ] **Step 2: Run** `cargo test --locked --all-features dracula_pro::derive` and confirm it fails.
- [ ] **Step 3: Implement.** `chrome` works in this order:
  1. Reject when `is_light(background)`.
  2. Set `muted`: the first `t` in 0.0, 0.05, …, 1.0 for which `mix(foreground, cursor, t)` reaches `contrast(·, background) >= 4.5`.
  3. Reject unless all of these hold: `contrast(ink, popover) >= 4.5`; warning and danger are each ≥ 3.0 on both `ground` and `popover`; the circular hue gap between warning and danger is ≥ 30°.

  In `src/dracula_pro.rs`, declare `#[allow(dead_code)] pub mod derive;` with a comment explaining that parts are used only by `build.rs` or tests.
- [ ] **Step 4: Run** the tests and clippy, and confirm both pass.
- [ ] **Step 5: Commit** `feat: add Dracula Pro palette derivation and a synthetic fixture`

### Task 2: Build-time generation of `THEMES`

**Files:**
- Create: `build/dracula_pro.rs`
- Modify: `build.rs`. Add `#[allow(dead_code)] #[path = "src/dracula_pro/derive.rs"] mod derive;` and `#[path = "build/dracula_pro.rs"] mod dracula_pro;`, plus one call at the end of `main`.
- Modify: `src/dracula_pro.rs`

**Interfaces:**
- Consumes: Task 1's `derive`. The generator refers to it as `super::derive`. Both `build.rs` and `src/dracula_pro.rs` declare `derive` as a sibling of the generator module.
- Produces in `build/dracula_pro.rs`:
  - `pub fn source_dir(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf>`:
    - `None` when `REX_DRACULA_PRO == "0"`;
    - otherwise `REX_DRACULA_PRO_DIR`, then `$XDG_DATA_HOME/rex-linux/dracula-pro-ghostty` (only if absolute), then `$HOME/.local/share/rex-linux/dracula-pro-ghostty`;
    - empty values count as unset.
  - `pub struct Output { pub code: String, pub themes: usize, pub chromes: usize, pub warnings: Vec<String>, pub watch: Vec<PathBuf> }`
  - `pub fn generate(dir: Option<&Path>) -> Output`
- Produces in `src/dracula_pro.rs`:
  - `pub struct Theme { pub id: &'static str, pub name: &'static str, pub source: derive::Source, pub chrome: Option<derive::ChromeTokens> }`
  - `pub static THEMES: &[Theme]`, from `include!(concat!(env!("OUT_DIR"), "/dracula_pro.rs"))`.
  - `pub fn terminal(t: &Theme) -> TerminalTheme`: every colour from `source`, including palette 0–15. `scheme` is `"light"` when `is_light(background)`, otherwise `"dark"`.
  - `pub fn chrome_index(setting_id: &str) -> Option<u8>`: the `THEMES` index whose `id.replace('-', "_") == setting_id` and whose `chrome` is `Some`.
- Generated code uses fully qualified paths (`crate::dracula_pro::derive::Source { … }`), so an empty or Alucard-only table leaves no unused imports.

- [ ] **Step 1: Write failing tests** in `src/dracula_pro.rs` under `#[cfg(test)] #[path = "../build/dracula_pro.rs"] mod generate;`.
  - Temp checkouts go in `std::env::temp_dir().join(format!("rex-dracula-test-{}-{name}", std::process::id()))`, removed at the end of each test.
  - Fixture files are copied in, or written with made-up colours.

  Tests:
  - `no_directory_generates_empty_tables`: `generate(None)` gives 0 themes and 0 chromes; the code contains `THEMES: &[Theme] = &[]`.
  - `known_files_become_themes_in_order`: `blade` and `pro` from the fixture give 2 themes and 2 chromes, with `"dracula-pro"` before `"dracula-pro-blade"` in the code.
  - `a_light_file_is_terminal_only`: the fixture's `alucard` gives 1 theme, 0 chromes, `chrome: None` and no warning.
  - `an_incomplete_file_is_skipped_with_a_warning`: `pro` without `palette = 3` gives 0 themes and a warning containing `"pro"` and `"palette 3"`.
  - `an_illegible_dark_file_is_terminal_only_with_a_warning`: 1 theme, 0 chromes, 1 warning.
  - `unknown_files_are_ignored`: a file named `dracula-plus` gives 0 themes and no warning.
  - `source_dir_resolution`:
    - `REX_DRACULA_PRO=0` gives `None`;
    - the override wins;
    - a relative `XDG_DATA_HOME` is ignored in favour of `HOME`.
  - `watch_lists_only_existing_paths`:
    - `watch` holds the directory and the files that exist, never a missing path;
    - a missing directory with an existing parent gives `[parent]`;
    - neither existing gives `[]`.
  - `watch_covers_a_checkout_created_later`: with only the parent present, `watch == [parent]`. Creating the directory inside it changes the parent's mtime, which is what reruns `build.rs`. Assert the mtime changed.
- [ ] **Step 2: Run** `cargo test --locked --all-features dracula_pro` and confirm it fails.
- [ ] **Step 3: Implement.** Generated shape:

  ```rust
  pub static THEMES: &[Theme] = &[Theme { id: "dracula-pro", name: "Dracula Pro", source: crate::dracula_pro::derive::Source { … }, chrome: Some(crate::dracula_pro::derive::ChromeTokens { … }) }, …];
  ```

  In `build.rs`:
  - print `cargo:rerun-if-env-changed=` for `REX_DRACULA_PRO`, `REX_DRACULA_PRO_DIR`, `XDG_DATA_HOME` and `HOME`;
  - print `cargo:rerun-if-changed=build` and each `watch` path;
  - print each warning as `cargo:warning=Dracula Pro: …`;
  - write `$OUT_DIR/dracula_pro.rs` only when its content differs;
  - print `cargo::rustc-check-cfg=cfg(rex_dracula_pro)` and `cargo::rustc-check-cfg=cfg(rex_dracula_pro_chrome)`;
  - print `cargo:rustc-cfg=rex_dracula_pro` when themes > 0, and `cargo:rustc-cfg=rex_dracula_pro_chrome` when chromes > 0.

  Known limitation, to document in README (Task 8): with neither the directory nor its parent present, a plain `cargo build` won't notice a later fetch until `build.rs` reruns for another reason. The fetch script always creates the parent (Task 6).
- [ ] **Step 4: Run both** configurations (Global Constraints) and confirm both pass.
- [ ] **Step 5: Commit** `feat: generate Dracula Pro themes from a local checkout at build time`

### Task 3: ChromeTheme keeps unknown ids and knows generated palettes

**Files:**
- Modify: `src/settings.rs:344-371` (enum and impl), `src/settings.rs:1054-1078` (test), `src/app/theme.rs:144-151` (`palette_of` arms), `src/terminal_theme.rs:92-96` (`paired`)

**Interfaces:**
- Produces:
  - `pub enum ChromeTheme { RosePine, RosePineMoon, GruvboxDark, #[default] Midnight, Generated(u8), Unavailable(&'static str) }`, still `Clone, Copy, Debug, Default, PartialEq, Eq, Hash`. Serde uses `#[serde(from = "String", into = "String")]`, which replaces `rename_all` and `#[serde(other)]`.
  - `impl From<String> for ChromeTheme`: `midnight`, `rose_pine`, `rose_pine_moon` and `gruvbox_dark` map to their variants; then `crate::dracula_pro::chrome_index` gives `Generated`; anything else is `Unavailable(intern(value))`.
  - `impl From<ChromeTheme> for String`: the inverse. `Generated(i)` is `THEMES[i].id` with `-` → `_`; `Unavailable(s)` is `s`.
  - `fn intern(s: String) -> &'static str`: a `static Mutex<Vec<&'static str>>` that leaks each distinct string once.
  - `pub fn all() -> Vec<ChromeTheme>`: `ALL`, then each `Generated(i)` whose theme has a chrome. Keep `pub const ALL` for the four shipped palettes.
  - `pub fn effective(self) -> ChromeTheme`: `Unavailable(_)` → `Midnight`; others unchanged.
  - `name()`: `Generated(i)` → `THEMES[i].name`; `Unavailable(_)` → `"Midnight"`.
  - **Mandatory in this task** so everything compiles and nothing panics:
    - `palette_of` gets `ChromeTheme::Generated(_) | ChromeTheme::Unavailable(_) => &MIDNIGHT`. Task 5 replaces `Generated`'s arm.
    - `paired(chrome)` matches on `chrome.effective()`, with `Generated` falling back to Midnight's builtin until Task 4.

- [ ] **Step 1: Update the existing test and write new failing tests** in `settings.rs`:
  - Rename `the_chrome_theme_round_trips_and_unknown_values_are_midnight` to `the_chrome_theme_round_trips_and_unknown_values_draw_as_midnight`. `"solarized_light"` now gives `Unavailable("solarized_light")`, with `effective() == Midnight` and `name() == "Midnight"`. The four shipped round trips are unchanged.
  - `an_unknown_chrome_survives_a_save`: load `{"chrome_theme":"not_a_real_chrome"}`, run `Settings::save` to a temp path, and reload; the JSON still has `"chrome_theme": "not_a_real_chrome"`. No skip.
  - `generated_chromes_round_trip`: for each `Generated` in `all()`, serializing and deserializing gives back the same value. Under the fixture, assert `all().len() == 6`.
- [ ] **Step 2: Run** `cargo test --locked --all-features settings::` and confirm it fails.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run both** configurations and confirm both pass.
- [ ] **Step 5: Commit** `feat: keep unknown chrome ids through saves and add generated chromes`

### Task 4: Terminal built-ins include generated themes

**Files:**
- Modify: `build/dracula_pro.rs` (emit `BUILTINS`), `src/dracula_pro.rs`
- Modify: `src/terminal_theme.rs:47-96` (`Builtin`, `builtin`, `paired`), `:324-341` (`resolve`)
- Modify: `src/app/panel.rs:6,593-600`, `src/app/workspace.rs:2020-2028` (`theme_label`)
- Modify the test at `src/app/tests.rs:2346` (theme picker rows)

**Interfaces:**
- Consumes: `ChromeTheme::{Generated, effective}`.
- Produces:
  - `Builtin.chrome: Option<ChromeTheme>`; the four shipped entries become `Some(..)`.
  - Generated `pub static BUILTINS: &[crate::terminal_theme::Builtin]`, appended to the same generated file. Each element is `crate::terminal_theme::Builtin { id, name, chrome: Some(crate::settings::ChromeTheme::Generated(i)) /* or None */, theme: || crate::dracula_pro::terminal(&THEMES[i]) }`. These closures don't capture, so they coerce to `fn` pointers.
  - `pub fn builtins() -> impl Iterator<Item = &'static Builtin>`: `BUILTINS` chained with `dracula_pro::BUILTINS`.
  - `builtin(id)` and `paired(chrome)` search `builtins()`, and `paired` matches `Some(chrome.effective())`.
  - `resolve`: `Named(id)` where `id` starts with `"rex/dracula-pro"` and `builtin(id)` is `None` resolves to `paired(chrome)`'s theme, with no error. Other unknown `rex/…` ids keep erroring; the upstream test at `terminal_theme.rs:~497` is unchanged.
  - `theme_label`: an unavailable `rex/dracula-pro…` id shows "Follow chrome".

- [ ] **Step 1: Write failing tests:**
  - `builtins_pair_chromes_by_index` (generator test): fixture `pro` plus `alucard` produce `Some(crate::settings::ChromeTheme::Generated(0))` and `chrome: None` in the code.
  - `an_unavailable_dracula_pro_theme_follows_the_chrome`: `resolve(&Named("rex/dracula-pro-nonexistent".into()), ChromeTheme::RosePine)` gives `Ok(Some(TerminalTheme::rose_pine()))`.
  - `unavailable_chrome_pairs_with_midnight`: `paired(ChromeTheme::Unavailable("x")).id == "midnight"`.
  - `follow_chrome_declares_the_palettes_paired_theme`: change it to loop over `ChromeTheme::all()`.
  - `every_generated_builtin_is_found_by_id`: for each `b` in `dracula_pro::BUILTINS`, `builtin(&format!("rex/{}", b.id))` is `Some`. Under the fixture there are 3 of them.
  - Theme picker test (`tests.rs:2346`): build the expected "Built in" rows from `builtins()` names after the shipped four, so it passes in both configurations.
- [ ] **Step 2: Run** `cargo test --locked --all-features terminal_theme` and confirm it fails.
- [ ] **Step 3: Implement.** In `panel.rs::theme_entries`, iterate `builtins()`.
- [ ] **Step 4: Run both** configurations and confirm both pass.
- [ ] **Step 5: Commit** `feat: list Dracula Pro terminal themes as built-ins`

### Task 5: Chrome palettes, picker and the About marker

**Files:**
- Create: `src/app/dracula_pro.rs`
- Modify: `src/app/theme.rs:57-151` (`Palette::from_tokens`, `palette_of`), the test at `:529`
- Modify: `src/app/panel.rs:636-641`, `src/app/mod.rs` (`mod dracula_pro;`), `src/app/about.rs` (`version_info` and the About view, **not** `format_info`)
- Test: `src/app/tests.rs` (including the chrome picker test at `:4526`)

**Interfaces:**
- Consumes: `rex_linux::dracula_pro::{THEMES, derive::ChromeTokens}` and `ChromeTheme::{all, effective}`.
- Produces:
  - `impl Palette { pub fn from_tokens(t: &ChromeTokens) -> Palette }`, with `card = t.ground`.
  - `pub fn palette(i: u8) -> &'static Palette` in `app/dracula_pro.rs`, backed by `static PALETTES: LazyLock<Vec<Option<Palette>>>`. A `None` entry or an out-of-range index gives `&MIDNIGHT`.
  - `palette_of`: `Generated(i)` → `dracula_pro::palette(i)`; `Unavailable(_)` stays `&MIDNIGHT`.
  - Chrome picker: rows come from `ChromeTheme::all()`, and `current` compares against `settings.chrome_theme.effective()`, so Midnight shows as current while the saved id is unavailable.
  - `#[cfg(rex_dracula_pro)] const LOCAL_THEMES: &str = "Includes local Dracula Pro themes";`. `version_info` appends `"{LOCAL_THEMES}\n"`, and the About section shows it as a muted line. Both are under `cfg(rex_dracula_pro)`.

- [ ] **Step 1: Write failing tests:**
  - `theme.rs`: change `every_palette_is_legible_and_pairs_with_its_terminal_theme` to loop over `ChromeTheme::all()`.
  - `theme.rs`: `from_tokens_copies_every_role`, with made-up tokens.
  - `tests.rs`: `a_routine_save_keeps_an_unavailable_chrome`.
    - Write a settings file with `"chrome_theme": "not_a_real_chrome"` and start the workspace with the existing `relaunch()` helper (`tests.rs:62`).
    - The active palette's ground is `0x001528`.
    - After `s.save()`, the file still reads `"not_a_real_chrome"`.
    - The chrome picker marks Midnight as current.
  - `tests.rs:4526` (chrome picker rows): the expected rows are the shipped four followed by the generated names from `all()`. This passes in both configurations; under the fixture, 6 rows.
  - `tests.rs`, `#[cfg(rex_dracula_pro_chrome)]`: `a_dracula_pro_chrome_applies_and_brings_its_terminal_theme`. Mirror the test at `tests.rs:4506`, using the first `Generated` in `all()`.
  - `about.rs`, `#[cfg(rex_dracula_pro)]`: `version_info_names_local_themes`, which checks that `version_info(..)` contains the literal `"Includes local Dracula Pro themes"`. The existing `format_info` tests are unchanged.
- [ ] **Step 2: Run** `cargo test --locked --all-features` and confirm the new tests fail.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run both** configurations and confirm both pass.
- [ ] **Step 5: Automate the marker survival check.** Add `scripts/test-dracula-pro-marker.sh`. It:
  1. builds `REX_DRACULA_PRO_DIR=$PWD/tests/fixtures/dracula-pro cargo build --locked --release --bin rex-linux` into a separate `CARGO_TARGET_DIR` under `target/`;
  2. strips a copy and checks `grep -c "Includes local Dracula Pro themes"` ≥ 1;
  3. builds again with `REX_DRACULA_PRO=0` and checks the count is 0.

  Run it.
- [ ] **Step 6: Commit** `feat: draw Dracula Pro chrome palettes and mark local builds`

### Task 6: Fetch script and mise wiring

**Files:**
- Create: `scripts/fetch-dracula-pro.sh`, `scripts/test-fetch-dracula-pro.py`
- Modify: `mise.toml` (`[tasks.fetch-dracula-pro]`; `[tasks.build]` gets `depends` and loses `sources`/`outputs`; `[tasks.check]` gets the fixture pass and the script tests)

**Interfaces:**
- Environment:
  - `REX_DRACULA_PRO`: `0` exits immediately with no output.
  - `REX_DRACULA_PRO_DIR`: same default as `source_dir`.
  - `REX_DRACULA_PRO_URL`: default `https://github.com/dracula-pro/ghostty.git`, used for clone and for every pull.
- Behaviour:
  - No `set -e`. Every path ends in `exit 0`.
  - Refuse, with a warning, a directory inside the current git work tree (`git rev-parse --show-toplevel`).
  - Always `mkdir -p` the checkout's parent.
  - Git runs with `GIT_TERMINAL_PROMPT=0 GIT_ASKPASS= SSH_ASKPASS= GCM_INTERACTIVE=never` and `-c http.lowSpeedLimit=1000 -c http.lowSpeedTime=15`, under `timeout 60`.
  - Missing checkout: clone into `<dir>.tmp.$$`, then `mv` into place. On failure, remove the tmp dir.
  - Existing checkout: `git -C <dir> pull --ff-only --quiet "$url" main`.
  - Any failure prints this to stderr:

    ```
    warning: Dracula Pro themes not fetched (<reason>); building with <the existing checkout|no Dracula Pro themes>. Check access with: gh auth status && gh auth setup-git
    ```
- mise:
  - `[tasks.fetch-dracula-pro]`: description "Fetch Dracula Pro Ghostty themes (licensees; uses your GitHub credentials)", `run = "bash scripts/fetch-dracula-pro.sh"`.
  - `[tasks.build]`: `depends = ["fetch-dracula-pro"]`. Remove `sources` and `outputs` so Cargo always runs and decides; Cargo is incremental, and only Cargo sees env and checkout changes.
  - `[tasks.check]`: add `python3 scripts/test-fetch-dracula-pro.py`, plus the fixture configuration's clippy and test commands.

- [ ] **Step 1: Write failing tests** in `scripts/test-fetch-dracula-pro.py` (`unittest`, temp dirs only):
  - Use a local bare repo made in the test, holding a made-up `pro` file.
  - Commit with `-c user.name=test -c user.email=test@example.invalid`.

  Tests:
  - `test_first_fetch_clones`
  - `test_second_fetch_pulls_new_commit`
  - `test_unreachable_remote_warns_and_keeps_checkout`: clone first, then move the bare repo away so the checkout's URL is gone. The fetch exits 0, prints `warning:` on stderr, and leaves the checkout's files and `HEAD` unchanged.
  - `test_failed_first_clone_leaves_nothing`: with `REX_DRACULA_PRO_URL=file:///nonexistent`, nothing is left: no dir and no `*.tmp.*`. Exit 0.
  - `test_disabled_does_nothing`: `REX_DRACULA_PRO=0` exits 0 and creates nothing.
  - `test_refuses_a_directory_inside_the_repo`
- [ ] **Step 2: Run** `python3 scripts/test-fetch-dracula-pro.py` and confirm it fails.
- [ ] **Step 3: Implement** the script and the mise changes.
- [ ] **Step 4: Run** the script tests and confirm they pass. Then run `REX_DRACULA_PRO_DIR=$(mktemp -d)/x REX_DRACULA_PRO_URL=file:///nonexistent mise run build`: it succeeds with the warning and Cargo runs.
- [ ] **Step 5: Commit** `feat: add mise task to fetch Dracula Pro themes before building`

### Task 7: Keep it out of distributed builds

**Files:**
- Create: `scripts/local_themes.py` (the shared check)
- Modify: `scripts/package-release.py` (after locating `binary`), `scripts/package-arch.py` (before repackaging each tarball), `scripts/test-release.py`
- Modify: `scripts/docs-captures.py`, `scripts/readme-screenshot.py`, `scripts/demo-record.py` (refuse a marked binary; they write tracked images)
- Modify: `.github/workflows/release.yml` (`REX_DRACULA_PRO: "0"` in the `build` job's existing `env:` block at `:48`)
- Modify: `nix/package.nix` (`REX_DRACULA_PRO = "0";` next to `REX_LINUX_CHANNEL` at `:87`)

**Interfaces:**
- `local_themes.py`:
  - `MARKER = b"Includes local Dracula Pro themes"`
  - `def refuse_local_themes(data: bytes, what: str) -> None`: raises `SystemExit(f"{what} contains local Dracula Pro themes and must not be packaged or published. Rebuild with: REX_DRACULA_PRO=0 cargo build --locked --release --bin rex-linux")`.
  - `def refuse_in_tarball(path: Path) -> None`: checks the `*/bin/rex-linux` member.

- [ ] **Step 1: Write failing tests** in `test-release.py`: `test_packaging_refuses_a_local_dracula_pro_build`. A fake binary and a fake tarball containing the marker are refused; the same files without the marker pass.
- [ ] **Step 2: Run** `python3 scripts/test-release.py` and confirm it fails.
- [ ] **Step 3: Implement** the guards and the env settings.
- [ ] **Step 4: Run** `python3 scripts/test-release.py` and confirm it passes.
- [ ] **Step 5: Commit** `fix: refuse to package or publish builds with local Dracula Pro themes`

### Task 8: Docs and the end-to-end check on this machine

**Files:**
- Modify: `README.md` (a new short section), `DESIGN.md` (one separate paragraph after the palette paragraph, and update "Palettes are dark only" to cover Alucard being terminal-only), `AGENTS.md` (one Conventions line)

- [ ] **Step 1: Write the docs.**
  - **README:** a section "Optional: Dracula Pro themes (licensees only)". Cover:
    - `mise run build` fetches the themes with your GitHub credentials;
    - `mise run fetch-dracula-pro` fetches them on its own;
    - where they're stored;
    - `REX_DRACULA_PRO=0` turns them off;
    - the plain-`cargo build` limitation from Task 2;
    - they stay on your machine and are never in releases.
  - **DESIGN.md:** the token mapping as a formula, with no values.
  - **AGENTS.md:** "Never commit, package or quote Dracula Pro theme files, colours or generated code; tests use the synthetic fixture."
- [ ] **Step 2: Run** `mise run check` and confirm it passes (both configurations and the script tests).
- [ ] **Step 3: Real checkout.** **This needs the user's GitHub access. If the fetch prints a warning, pause and ask.**
  - Run `mise run build`. Expect no `Dracula Pro:` warnings, 8 themes and 7 chromes (count rows in the pickers).
  - Run `cargo test --locked --all-features` and clippy.
- [ ] **Step 4: Check the marker and the guard.**
  - `grep -c "Includes local Dracula Pro themes" target/release/rex-linux` is ≥ 1.
  - `python3 scripts/package-release.py` refuses with the guard message.
  - Don't rebuild with `REX_DRACULA_PRO=0` into `target/release`.
- [ ] **Step 5: Look at it.**
  - Run `target/release/rex-linux --snap <scratch>/dracula.png` with the chrome set to Dracula Pro and a popover open.
  - Check the popover lift, muted text and hover fills by eye.
  - Keep the screenshot in the scratchpad only; it shows licensed colours.
- [ ] **Step 6: Run** `git status` and `git diff --stat master`, and confirm no checkout, screenshot or generated file is tracked.
- [ ] **Step 7: Commit** `docs: document optional Dracula Pro themes`

## Deviations from the spec (decided here)

- `build.rs` rejects illegible palettes before compiling them, so the existing palette test can't fail on a real checkout.
- The marker is a `version_info`/About line, so it is referenced by code and survives linking and stripping. Task 5 checks this automatically against the fixture.
- The mise build drops its `sources`/`outputs` shortcut, so Cargo sees env and checkout changes. Before, an unchanged tree skipped Cargo.
- `Source.cursor_text` and `selection_foreground` are optional, matching the spec's required-key list.
