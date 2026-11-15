# Headless test determinism investigation

Base: `a8fc9e736620372a2f1e42045881abe44a9a5cd6` (origin/master).
Scope: pc_cf7357a21377 and the tab-icon visibility test's empty-host-channel assertion. No production or platform changes intended.

## Hypotheses and experiments

- Font measurements use Rune's process-wide last registered fonts unless a thread-scoped `with_families` guard is alive. `Runtime::new` supplies Inter; `term::tests::typing_styled_terminal_runs_never_creates_a_second_ui_line` and `scaled_rects` supplied installed JetBrains fonts without a guard. Inspecting `text/measure.rs` shows `TextSystem::new_with_paths` writes that registry. Test a nested caller with fixed Inter before and after each real-font helper: the caller's metrics must remain unchanged.
- The tab-icon test already has a font guard and its captured host has no worker/wake sender. Direct `sidebar.toggle()` plus assignment to `vertical_tabs` leaves `prefetch` inconsistent. A delayed window-size save calls `save()` → `sync_prefetch()`, and can emit `PrefetchViews` after one second of wall time. Backdate `resized` before polling instead of sleeping; inspect the actual command. This is separate from font state, channel delivery and worker timing.

Raw local logs and preserved before/after test executables: `/home/shuv/.cache/agent-ws/rex-determinism-evidence/`.

### Completed failing-before evidence

The unchanged base passed three default-parallel `cargo test --locked --all-features` runs (each: 379 library, 776 UI and 8 integration passes; 6 existing ignored). Neither historical symptom happened naturally in these three runs. This is not a measured reproduction of the historical 9/40 rate.

Adding only the two caller-metric regressions to the old helpers failed both tests: the caller's Inter cell changed from `(13.549805, 21)` to `(9, 21)` after either `scaled_rects` or the typing test. Both assertions compare with fresh caller state, not with another calculation through the changed font registry. Font epoch restoration additionally checks that fonts were not silently replaced by a coincidentally equal-width face.

Backdating only `resized` by two seconds in the original tab-icon visibility test failed its original no-host-traffic assertion in 0.13 seconds. The channel contained exactly `PrefetchViews({"s"})` then `PrefetchViews({})`. The first came from the overdue size save opening the sidebar's subscription; the second from the explicit sidebar-close action ending it. The fixture's captured connection has a default model and a disconnected wake receiver; no host worker can asynchronously deliver either command. No font change is needed to reproduce this failure.

Commands (both exited 101 before the fixture fixes):

```sh
CARGO_TARGET_DIR=/home/shuv/repos/rex-rune/target cargo test --locked --all-features --bin rex-rune do_not_replace_the_callers_font_metrics
CARGO_TARGET_DIR=/home/shuv/repos/rex-rune/target cargo test --locked --all-features --bin rex-rune icon_counts_preserve_visibility_modes_and_sidebar_fallback
```

### Fix and fidelity

Only the two real-font row helpers acquire thread-local Rune family guards, lasting through text-system construction, measurement and rendering. They supply bundled IBM Plex Mono and Symbols Nerd Font with **no installed-font paths**. There is no global lock/reset, environment mutation, broad serialization or skipped core behavior. The existing 79-keystroke styled prompt, glyph line bounds and fractional-scale sprite/pixel assertions still run with real shaped/rasterized glyphs. The fidelity trade-off is deliberate: these core rules no longer vary with an installed JetBrains font or its machine-specific stand-in. The separate existing opt-in real-font capture tests remain unchanged. Related pc_a1571ab983c8 concerns Camera callers using `Runtime::new` (Inter instead of app mono); these two row helpers already supplied explicit text systems, so that is a different fixture problem and was not broadened into this change.

The tab-icon test uses `set_vertical_tabs(true)`, the same preference boundary as the app, instead of editing fields behind it. It asserts exactly the opening `PrefetchViews({"s"})` and closing `PrefetchViews({})`, consuming only those intentional action effects. It retains a strict empty-channel assertion for everything else and makes the pending size save overdue on every run. No command filtering, sleep, assertion retry or timeout increase is used. All three visibility modes and counts 0/1/5 remain covered.

Both regression commands then passed (2 font tests and 1 nine-case tab-icon matrix).

### Completed validation

Linux toolchain: Rust/Cargo 1.96.1. All Cargo commands used the shared `/home/shuv/repos/rex-rune/target`; no new target tree was installed.

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: exit 0. Six existing Rune dependency warnings remain; none were suppressed or changed.
- Forty **fixed-count**, default-parallel test-process stress runs of the two new font regressions plus the tab-icon visibility test: **0/40 failing runs**, 120 test executions and 360 visibility/count cases. This is an observed rate, not a proof against every possible schedule. Stress did not retry a failing assertion. `stress.sh` and individual logs are in the evidence directory.
- Actual headless GPU acceptance: existing `nerd_symbols_draw` and `tab_icons_draw` each passed, separately, with private HOME/config/state/runtime directories, no display and a nonexistent private bus address. They generated **19 PNGs**; inspected the symbol grid and five-icon toolbar capture. The symbol test independently measured ink bounds against cells. Existing real-font capture coverage was neither gated further nor redesigned.
- Native X11/server acceptance was **not** run: `scripts/tab-icons-x11.py` starts a real Rex server, forbidden by this job. No real Rex endpoint, desktop bus, audio, client restart, package installation, container or privileged/systemd operation was used. These test-only changes were exercised through real Rune layout/input and rendering, not native OS input.
- macOS and Windows native checks were **not** run here. Existing PR CI owns those targets; no cross-target success is claimed.

- `CARGO_TARGET_DIR=/home/shuv/repos/rex-rune/target cargo test --locked --all-features`, repeated **three times with default parallelism**: all exit 0. Each run passed **379 library + 778 UI + 8 integration = 1,165 tests**, with the same 6 existing ignored tests. UI durations were 73.24, 68.91 and 81.03 seconds. No thread-count reduction, test retry or assertion relaxation was used. Logs: `after-full-{1,2,3}.log`; exit receipt: `after-full.results`.

No papercut was marked resolved; the lead will associate the fixed SHA and completed verification with pc_cf7357a21377. No merge, tag or release is part of this change.

## Rat-stack reading applied

- `ratstack://skills/investigate-a-failure`: bounded symptoms, separate hypotheses, deterministic reproductions and measured rates.
- `ratstack://lore/tests-that-earn-their-place`: fresh supplied state and independent assertions, not a production implementation restated in the oracle.
- `ratstack://lore/testing-ladder`: fixture/behavior regressions, then actual runtime rendering and full checks.
- `ratstack://lore/the-fence`: retain fmt, Clippy, hooks and the full gate.
- `ratstack://lore/schemas-define-the-boundary`: typed fonts and decoded fixture layouts at existing boundaries; no new parsing or protocol surface.
- `ratstack://lore/error-model`: fail loudly on unexpected commands or rendering defects; do not mask them as empty success.
- `ratstack://lore/property-based-testing` and `ratstack://lore/model-based-testing`: independent invariants matter; these two concrete fixture leaks need examples and a visibility/count matrix, not a new random/model framework.
- `ratstack://lore/durability`: no durable operation is changed; saved-settings read-back tests remain intact. Record completed verification and the fixed commit before resolving the papercut (resolution belongs to the lead).

The helper's search/read API returned HTTP 429 on some reads; the same public Markdown pages were read directly, and the cached investigate-a-failure page was read locally. Only language-neutral rules were applied; no TS/Effect dependencies.
