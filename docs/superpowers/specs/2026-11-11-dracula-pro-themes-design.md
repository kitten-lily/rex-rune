# Dracula Pro built-in themes (build-time, local only)

## Intent

Dracula Pro's Ghostty themes live in the private repository
`https://github.com/dracula-pro/ghostty`. They are licensed per user and **may not be
redistributed**. A licensee who builds Rex Linux on their own machine should get them as
first-class built-in themes, just like Rosé Pine or Gruvbox Dark: a chrome palette paired
with a terminal theme, picked in Settings → Interface → Theme. Nobody else's build, and
nothing in this repository, should contain them or any colour derived from them.

So: a separate fetch step pulls the repo with the user's own GitHub credentials into a
local directory, and `build.rs` turns whatever it finds there into generated Rust that
compiles in as built-ins. Without the directory, the build is exactly today's app.

## Goals

- `mise run fetch-dracula-pro` clones or updates `dracula-pro/ghostty` using the user's
  existing git/GitHub credentials.
- `mise run build` (and so `mise run install`) runs that fetch first, so builds on this
  machine always include the themes.
- Any `cargo build` that can see the fetched directory generates built-ins from it.
- Each dark theme becomes a chrome palette **plus** its paired terminal theme.
- Alucard (the light variant) becomes a **terminal-only** built-in theme.
- Builds without the directory compile, test and behave exactly as today.

## Non-goals

- A light chrome palette. DESIGN.md says "there is no light chrome palette" (hover,
  pressed and border fills are translucent white). Alucard stays terminal-only until that
  is designed separately.
- Fetching at `cargo build` time. `build.rs` never touches the network.
- Supporting Dracula Pro in CI, release artifacts, the Arch repo or the Nix package.
- Pinning a revision of the Dracula Pro repo; the fetch follows its default branch.

## Anti-goals (a failure even if it works)

- Any Dracula Pro file, colour value or generated code committed to git, put in a release
  tarball, `.deb`, Arch package or Nix store path built by CI, or published anywhere.
  DESIGN.md's token table must not list Dracula Pro colours (describe the mapping only).
- A build that fails, hangs on a credential prompt or stalls a long time because the user
  is offline, unauthenticated or has no licence.
- A user's saved theme choice that crashes or errors on a build without the themes.

## Source facts (checked 2026-11-11)

The repo's default branch is `main`. At the top level it has eight theme files with no
extension, plus `readme.md` and `.gitignore`: `pro`, `blade`, `buffy`, `lincoln`,
`morbius`, `nosferatu`, `van-helsing` (seven dark) and `alucard` (light). Each is a plain
Ghostty theme: `palette = 0..15`, `background`, `foreground`, `cursor-color`, `cursor-text`,
`selection-background` and `selection-foreground`, which `TerminalTheme::parse` in
`src/terminal_theme.rs` already reads. (No colour values are reproduced here on purpose.
Tests use synthetic values.)

## How it works

### 1. Fetch (`mise run fetch-dracula-pro`)

- Location: `${XDG_DATA_HOME:-$HOME/.local/share}/rex-linux/dracula-pro-ghostty`, which can
  be overridden with `REX_DRACULA_PRO_DIR`. *(My call: outside the repo so every worktree
  under `../rex-rune-<topic>` shares one checkout and nothing can be `git add`ed by
  accident.)*
- Missing → `git clone https://github.com/dracula-pro/ghostty.git <dir>`. Present →
  `git -C <dir> pull --ff-only`. Credentials come from the user's git credential helper
  (for example `gh auth setup-git`). Nothing in the repo stores a token.
- Run git with `GIT_TERMINAL_PROMPT=0` and a short connection timeout, so it never
  prompts or hangs.
- **A failed fetch skips with a warning and never fails the build.** On any error
  (offline, no access, not authenticated, git missing) it prints a warning saying why and
  what to do, then exits 0, and the build continues as normal. With no earlier checkout,
  that build simply has no Dracula Pro themes. With an earlier, complete checkout, the
  build uses it as it is. *(My call: being offline shouldn't remove themes you already
  had.)* A failed first clone must not leave a partial directory behind (clone into a
  temporary directory and rename it into place), so `build.rs` never sees half a checkout.
- Implementation as a small script in `scripts/` (shell or Python), wired up as a mise
  task.

### 2. `mise run build`

- `[tasks.build]` gets `depends = ["fetch-dracula-pro"]`.
- Add the fetched directory to the task's `sources`, so new or changed themes trigger a
  rebuild even when no Rust source changed. mise globs are relative to the project, so
  the builder needs to check that an absolute or env-expanded path works there; if it
  doesn't, have the fetch task touch a git-ignored stamp inside the repo when the
  checkout's HEAD changes, and list that stamp instead.

### 3. `build.rs` generation

- Resolve the directory the same way the fetch does (`REX_DRACULA_PRO_DIR`, otherwise the
  XDG default). Emit `cargo:rerun-if-env-changed` for `REX_DRACULA_PRO_DIR`,
  `REX_DRACULA_PRO` and `XDG_DATA_HOME`, and `cargo:rerun-if-changed` for the directory
  and each theme file it reads.
- Opt-out: `REX_DRACULA_PRO=0` skips generation even if the directory exists (used by
  release packaging, see §6).
- Read only the known theme names listed above. Unknown files are ignored, so a future
  upstream theme doesn't land without someone reviewing its name and mapping. *(My call;
  the alternative is "every extensionless file".)* A known file that is missing or can't
  be parsed is skipped with a `cargo:warning`, never a build error. `TerminalTheme::parse`
  accepts partial files, so generation also checks that every field it uses is present
  and valid (background, foreground, cursor colour, selection background, palette 0–15)
  before indexing. A theme that's incomplete is skipped with a warning.
- Write `$OUT_DIR/dracula_pro.rs`, always (empty tables when nothing is found), and pull
  it in with `include!`. Set `cargo:rustc-cfg=rex_dracula_pro` when at least one theme was
  generated, and `rex_dracula_pro_chrome` when at least one chrome palette was (an
  Alucard-only checkout has the first without the second). Declare both with
  `cargo::rustc-check-cfg` so clippy stays clean either way.
- Parsing: `build.rs` can't easily use the library crate. Either write a minimal parser
  (only the keys above) or move `TerminalTheme::parse`'s line parsing into a small file
  that both `build.rs` and the library `#[path]`-include. The second keeps one parser and
  is preferred.

### 4. Terminal themes

- Each file becomes a `Builtin` in `src/terminal_theme.rs`, with the full colours
  (`TerminalTheme::full`-style: background, foreground, cursor, cursor text, selection,
  palette 0–15) and `scheme` set to `dark` (or `light` for Alucard).
- `BUILTINS` is a `const` slice today. It will have to combine the static entries with the
  generated ones, for example via a function or a `LazyLock<Vec<…>>`. That choice is left
  to the builder.
- Ids: `dracula-pro`, `dracula-pro-blade`, `dracula-pro-buffy`, … `dracula-pro-alucard`
  (stored as `rex/dracula-pro-…`). Names: "Dracula Pro", "Dracula Pro Blade", …,
  "Dracula Pro Van Helsing", "Dracula Pro Alucard". *(My call.)*
- `Builtin.chrome` becomes optional (or Alucard gets a separate terminal-only list), since
  Alucard pairs with no chrome palette. `paired(chrome)` still must find a theme for every
  chrome palette.
- In the theme picker they appear under "Built in" with the others.
- If the saved `terminal_theme` names a `rex/dracula-pro-…` id that this build doesn't
  have, **don't** use the existing missing-theme path. Today that path sends a missing
  built-in id to file-name validation, then startup shows an error and follows the
  server's colours. Instead, add an explicit fallback: an unavailable `rex/dracula-pro-…`
  id resolves to Follow chrome without an error, and the saved value is kept as it is.

### 5. Chrome palettes

- Every dark theme gets a `Palette` (`src/app/theme.rs`) and a `ChromeTheme` value, listed
  in Settings → Interface → Theme after the four shipped palettes.
- `ChromeTheme` is a closed, `Copy`, serde `snake_case` enum with `#[serde(other)]`
  falling back to Midnight. Generated entries need stable serialized ids
  (`dracula_pro`, `dracula_pro_blade`, …). One shape that keeps `Copy`: a
  `ChromeTheme::Generated(u8)` variant indexing the generated table, with custom
  (de)serialization by id. `ChromeTheme::ALL` becomes a function or a lazily built list.
- **Requirement (decided): a saved Dracula Pro chrome survives any save.** On a build
  without it, the chrome draws as Midnight. The saved id stays in the settings file
  through every rewrite, and comes back after a rebuild with the themes. Today
  `#[serde(other)]` throws the unknown id away when the file is read, and
  `Workspace::save()` rewrites the whole file on routine actions (host switches, column
  changes, the first theme declaration, and so on). So the saved choice and the chrome
  actually used have to be separate: for example, keep the raw saved id in `Settings`
  (written back as it is) and resolve the effective `ChromeTheme` from it. Only an
  explicit chrome choice by the user replaces the saved id. This applies to *any* unknown
  chrome id, not only Dracula Pro ones. A settings round-trip test pins it.
- Mapping from the Ghostty file, worked out in `build.rs`. The repo holds only this
  formula, never the values:

  | Token | Source |
  |---|---|
  | `ground`, `card` | `background` |
  | `popover` | `background` mixed 6% toward `foreground` *(my call; tune visually)* |
  | `ink` | `foreground` |
  | `muted` | `cursor-color` (Dracula Pro's comment colour), mixed toward `foreground` until it reaches 4.5:1 on `card` |
  | `accent` | palette 4 (purple) |
  | `accent2` | palette 5 (pink) |
  | `live` | palette 2 (green) |
  | `warning` | palette 3 (yellow) |
  | `danger` | palette 1 (red) |
  | `on_accent` | `background` |
  | `terminal_selection` | `selection-background` |

  The raw cursor colour falls short of AA on the background in at least one theme, which
  is why `muted` gets the adjustment.
- The existing test `every_palette_is_legible_and_pairs_with_its_terminal_theme` runs over
  every chrome palette, generated ones included, when they are present. `build.rs`
  enforces the same rules (muted/card and ink/popover ≥ 4.5, warning/danger ≥ 3 on card and
  popover, warning and danger ≥ 30° apart in hue). A theme that still fails after the
  `muted` adjustment becomes terminal-only, with a `cargo:warning`, rather than breaking
  the build.

### 6. Keeping it out of distributed builds

- CI (`.github/workflows/*.yml`) and Nix builds never run the fetch task. The builder should
  confirm they can't see the XDG directory either: GitHub runners have no such directory,
  and the Nix sandbox has no `$HOME`. If the Nix derivation could ever see it, set
  `REX_DRACULA_PRO=0` in `nix/package.nix`.
- `scripts/package-release.py` and `scripts/package-arch.py` must refuse to package a
  binary that contains Dracula Pro themes. The binary needs a detectable marker under
  `cfg(rex_dracula_pro)`. It has to be a marker in the binary's bytes, not something
  you'd get by running it: `package-arch.py` repackages the aarch64 build on x86_64
  without emulation. It must be a fixed string that stays after release stripping (for
  example a `#[used]` static), and the scripts check every tarball/binary for it before
  packaging. A test confirms the marker survives a stripped release build. (Showing it in
  the About dialog as well is a nice extra.) The guard isn't optional.
- `.github/workflows/release.yml` also sets `REX_DRACULA_PRO=0` as a second safeguard.

### 7. Fork-only shape (decided)

This lives only in the kitten-lily fork and isn't proposed upstream to shuv1337/rex-rune.
Keep the diff against upstream small and isolated, so regular upstream syncs conflict
as little as possible:

- Put the generation logic in its own file (for example `build/dracula_pro.rs`, called
  from `build.rs` with one line), and the runtime glue in its own modules (for example
  `src/dracula_pro.rs` and `src/app/dracula_pro.rs`).
- Shared files (`settings.rs`, `terminal_theme.rs`, `app/theme.rs`, `panel.rs`) get only
  the hooks they need: append generated entries to the built-in lists, and add the saved
  vs effective chrome split.
- Doc edits to README, DESIGN.md and AGENTS.md stay short and in separate paragraphs, not
  woven into existing ones.

### 8. Docs

- README: a short section on optional Dracula Pro themes. Say who it's for (Dracula Pro
  licensees), give the command, and say that they stay on your machine and are never in
  release builds.
- DESIGN.md: one paragraph saying that locally built Dracula Pro palettes are derived by
  formula (the table above) and that Alucard is terminal-only. Update the "Palettes are
  dark only…" wording so it stays true. Don't list Dracula Pro colours.
- AGENTS.md: one line under Conventions saying Dracula Pro material must never be
  committed or distributed.

## Known consequence

The in-app update notice points at official signed builds. Installing one replaces this
local build with one that has none of this fork's code. Its `#[serde(other)]` reads a
Dracula Pro chrome as Midnight, and its next routine save writes `"midnight"`, so the
choice is **lost**, not just hidden. The preservation in §5 protects only builds that
contain it (this fork's builds, with or without the themes). Changing the update notice
is out of scope. Accepted by the user (2026-11-11); no upstream change.

## Testing

- Unit tests that don't need the private files: the palette formula (including the
  `muted` AA adjustment) on a synthetic Ghostty theme written in the test, and a settings
  round trip where an unknown `dracula_pro_*` chrome reads as Midnight; generation on a
  synthetic checkout with an incomplete file and an Alucard-only checkout.
- The existing palette legibility test covers generated palettes automatically on
  machines that have them.
- A headless UI test (`src/app/tests.rs`) gated on `cfg(rex_dracula_pro_chrome)`: choosing a
  Dracula Pro chrome applies it and brings its paired terminal theme.
- Manual: `mise run build` offline and while unauthenticated still succeeds; the release
  packaging guard rejects a Dracula Pro build; render a snapshot (`-- --snap out.png`) of a
  Dracula Pro chrome to check `popover` and fills by eye.

## Left to the builder

The `BUILTINS` / `ChromeTheme::ALL` shape, the binary marker, shell vs Python for the fetch
script, the shared parser layout, and the exact popover mix (tune visually).
