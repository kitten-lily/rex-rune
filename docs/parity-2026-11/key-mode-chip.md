# Active key-mode chip

Issue [#129](https://github.com/shuv1337/rex-rune/issues/129): Rex key modes were routed
correctly but only a six-second notice named one, so while a mode lasted the client did not
say which one was active. An exclusive mode swallows the keys it does not bind, so the
terminal could look unresponsive with nothing on screen to explain it.

## As built

- **One chip, from the existing stack.** The toolbar's live chips now begin with the key
  mode: a quiet 28 pt chip, `mode resize`, naming the innermost entry of
  `bindings.modes` (`Workspace::key_mode`). Nothing is cached or duplicated; the chip is
  derived from the same state dispatch uses, so nested modes show the inner one and pop back
  to the outer one, a once mode is named until its key runs it, and `Escape`, Exit key mode,
  Exit all key modes, reload and automatic clearing remove it on the next frame.
- **Named from entry, not from the next key.** Every accepted entry — a key, a local
  `client.mode.enter` action or the same action returned by a host — goes through
  `Workspace::enter_key_mode`, which records what the stack belongs to and pushes the mode.
  The chip therefore names a mode entered by `rex do client.mode.enter` before any key is
  pressed, and the next key dispatches from that same stack.
- **Ownership, effective at the change.** A mode belongs to the host, session, keymap
  generation, server keymap revision and connection it was entered under (one
  `bindings::Owner` record, the same predicate dispatch uses). The stack ends where one of
  those changes: `sync` for the active host's session, keymap revision or connection, the
  active-host transitions through `update_visibility` (host switch, host added, removed or
  re-pointed, navigation) and `keymap_changed` for a new local keymap. A pane, a panel or
  focus changing does not end a mode, as before. Because the stack itself ends there,
  switching to another host or session and back does not show a mode nobody entered again;
  the display check is a read-only guard for the same rule, not the only copy of it.
- **Placement.** In the toolbar row and in the macOS title row the chip sits after the tab
  track, before the pending sequence; with no toolbar row it is the first chip on the
  existing floating status surface at the top left, and the title strip draws it where the
  toolbar has no row. It is drawn whether or not **Show key sequence hints** is on, because a
  mode is not a key sequence hint.
- **Bounded, and budgeted with the rest of the row.** At most 150 pt wide and 28 pt high,
  the name truncates with an ellipsis while the element's accessible label
  (`Key mode: <name>`) and tooltip (`Key mode <name> · Escape to leave`) keep it whole. Below
  1000 pt of toolbar width (the window's width divided by interface zoom) the chip is the
  name alone, giving the `mode` word's room to the name; the word stays in the accessible
  name. The live chip row shares one budget: on a narrow bar with a live chip the identity
  chip keeps its existing 120 pt floor instead of growing to its words, the mode and
  pending-sequence chips shrink no further than their floors (56 and 48 pt), and the tab
  track keeps a floor that holds both 26 pt arrows inside it. Downloads also uses effective
  toolbar width: with sidebar tabs it gives up its full filename when a mode shares a narrow
  bar or a pending sequence. Its full name and progress remain in the tooltip and accessible
  label, so Settings and Close stay in the window. The mode chip takes no focus, has no press
  handler, and reserves no space when no mode is
  active.
- **The entry notice is gone.** The chip replaces "Key mode: … · Escape to leave", which only
  ever described the start of a mode. An unknown mode name still gets its notice, since
  nothing was entered.

Per-mode palettes, outline colours or a settings switch are deliberately absent: the issue
listed them as optional, and the chip is chrome like the pending sequence.

## Checks

`src/app/key_mode_tests.rs`, run against the real headless UI runtime:

- no mode: no chip, and no room taken from the drag space;
- a mode appears on the frame that follows the key (not before), stays past the old notice's
  six seconds, and leaves with `Escape` or an inherited `client.mode.exit_all` binding;
- a local `client.mode.enter` action, and the same action returned by a host through `sync`,
  name the mode before any key (neither sets a keyboard input context); the next key then
  routes from that stack, and a once mode entered that way is named until its key runs;
- nested modes name the inner one and fall back to the outer one; a once mode is named until
  its one key runs, including the key that runs a pane action;
- an exclusive mode keeps its name while it swallows a plain key, and a non-exclusive one
  still lets that key reach the program;
- key sequence hints off keeps the chip;
- Toolbar Hidden and Show on hover: no row returns, the chip is the floating status
  surface's first chip, and the terminal grid does not move;
- a long name truncates at 1024×700 and 640×480 with the whole name in the tooltip, the
  chip clears Settings and Close app, and the terminal grid keeps its rect;
- at 640×480, with a long mode, with a long mode and a pending sequence, with a long host
  name too, with a download, and with a sequence but no mode: Commands, New tab, Settings and
  Close app are inside the window in that order, both tab arrows are inside their track and
  clear the mode chip, the chip keeps at least 40 pt for its name, and the whole name stays in
  its accessible label and tooltip;
- a receiving download with a long filename, sidebar or toolbar tabs and a pending sequence
  on/off: all mandatory controls and usable tab arrows fit at 640×480/100%,
  960×720/150%, 1024×768/150%, 1200×900/150% and 1024×700/100%. Each case first checks
  the no-mode control, then enters the long mode through real key dispatch; the scale is
  asserted unclamped, the terminal grid stays fixed and full download names remain accessible;
- the mode word is drawn at 1024×700 and dropped at 640×480, with `Key mode: resize` the
  accessible name at both;
- the unified macOS title row and its title strip name the mode in the row;
- a pane change keeps the mode; another session, a lost and regained connection, another
  server keymap revision, another local keymap, another host and Reload config all end it
  without a keypress, and a session, connection or host round trip does not bring it back
  (the stack is asserted empty, not just the label hidden);
- the chip takes no focus, and existing dispatch/release behaviour is covered unchanged by
  `compatibility_tests.rs`.

Real-font captures (`REX_TEST_SNAP_DIR`, one test at a time as fonts are process-global):
`mode_chip_draws` (1024×700), `mode_chip_draws_at_the_minimum_size` (640×480) and
`mode_chip_draws_with_a_pending_sequence` (both sizes) write no mode, `resize`, a long
truncated name, the title row and a long mode beside a pending sequence.
`mode_chip_draws_with_downloads_at_narrow_effective_widths` adds before/mode/sequence
captures with sidebar tabs at 640×480 and toolbar tabs at 1024×768/150%.

## Limits

- Verified locally on Linux only (check, Clippy, fmt, the headless suite and the captures
  above). The macOS title row is exercised headlessly through `unified_title`, as other
  title-row tests do; native macOS and Windows were not run here — the pull request's CI
  covers their builds and the shared suite.
- At 640 pt the bar's live chips take their room from the tab track, which scrolls to its
  arrows: at that width the track keeps both arrows' hit areas and the chips their floors,
  and the tab strip itself can be narrow until a mode or sequence chip leaves. The strip's
  scrolling content is unchanged; this is chrome width, never a terminal resize.
- The chip reflects only this client's mode stack. A mode another client entered, and a mode
  a host's keymap defines but nobody entered, are not named.
- The download coexistence matrix exercises actual, unclamped 150% zoom at the three sizes
  above, not every viewport/zoom combination or a native high-DPI/window-manager setup.
