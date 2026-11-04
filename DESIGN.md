# Design system: Midnight

Rex Linux draws calm, dark chrome around terminals that the server owns. The tokens below mirror `src/app/theme.rs`, which is the source of truth; update both together. Midnight is the default; a build can use Rosé Pine or Rosé Pine Moon instead (`REX_CHROME`, see [Chrome palettes](#chrome-palettes)), which change colours only.

## Principles

- **The terminal is the content.** Chrome frames it and stays out of the way. Terminal text, colours, cursor, block order and split nesting come from the server and are never recoloured. Tiled tabs use the server's ratios. A tab's local scrolling view may instead give its top-level horizontal columns client-owned widths; splits inside each column keep the server's ratios. The chosen terminal theme reaches the terminal through the server. The one background exception remains: when the server default and theme background are equally dark or light, the theme background is drawn; otherwise the server's is kept. Explicit colours are preserved.
- **Cells don't animate.** Terminal cells, colours and cursor never interpolate or crossfade. Cards spring to server layout changes and to a focused column's automatic pan as rigid surfaces; direct wheel panning is immediate.
- **One working row.** Session identity, tabs, commands and the window's own controls share a single toolbar. Navigation (hosts → sessions) opens on demand, with an optional sidebar.
- **Quiet when healthy.** A connected host, a live pane and an idle active tab carry no status dots or "Connected" labels; names and host icons are enough. Words appear for trouble ("Offline", "Reconnecting", "input paused"), unseen completion or bell, and terminal-reported progress.
- **Distinct states.** The current session, the keyboard-selected row, a captured pane target and live terminal focus each have a different cue. Keyboard selection is never fill alone.
- **Name the target.** Creation actions name their host; destructive confirmations name the pane, tab and host and default to **Keep running**; closing the app leaves server programs running.

## Colour

| Token | Value | Use |
| --- | --- | --- |
| `GROUND` | `#001528` | Window ground and page background |
| Terminal background | `#001528` in the built-in theme | The area behind terminal text, from the terminal theme. Pane cards, their headers and strips stay on `GROUND`. |
| `POPOVER` | `#0d1524` | Popovers, dialogs, raised surfaces (drawn at 95% over a backdrop blur) |
| `INK` | `#e8eef6` | Primary text and icons |
| `MUTED` | `#93a3b8` | Secondary text, labels, shortcuts and icons |
| `ACCENT` | `#7cc4ff` | Focus, selection outline, current-session emphasis, caret |
| `ACCENT2` | `#a78bfa` | Secondary accent |
| `LIVE` | `#5eead4` | Kit success colour |
| `WARNING` | `#f5d48a` | Reconnecting, notices, ownership strips |
| `DANGER` | `#ff8a8a` | Validation errors and destructive text |
| `ON_ACCENT` | `#061220` | Text on an accent fill (primary buttons) |
| `TERMINAL_SELECTION` | `#2f4a6e` | Local terminal text-selection overlay, when the terminal theme has no `selection-background` (its `selection-foreground`, if any, recolours selected text) |

Interaction fills are translucent white over the surface: hover 7%, pressed 11%; borders 6–20% white (focus uses `ACCENT`). Trouble states always carry a word, never colour alone; healthy states show nothing. The chrome has no light palette; a light terminal theme changes only the terminal area.

### Chrome palettes

Rosé Pine builds map the tokens onto Rosé Pine's roles; everything else (fills, borders, motion, layout) is unchanged. The built-in terminal theme of each is Rosé Pine's own (as its Ghostty theme), with `GROUND` as its background.

| Token | Role | Rosé Pine | Rosé Pine Moon |
| --- | --- | --- | --- |
| `GROUND` | base | `#191724` | `#232136` |
| `POPOVER` | surface | `#1f1d2e` | `#2a273f` |
| `INK` | text | `#e0def4` | `#e0def4` |
| `MUTED` | subtle | `#908caa` | `#908caa` |
| `ACCENT` | iris | `#c4a7e7` | `#c4a7e7` |
| `ACCENT2` | foam | `#9ccfd8` | `#9ccfd8` |
| `LIVE` | pine | `#31748f` | `#3e8fb0` |
| `WARNING` | gold | `#f6c177` | `#f6c177` |
| `DANGER` | love | `#eb6f92` | `#eb6f92` |
| `ON_ACCENT` | base | `#191724` | `#232136` |
| `TERMINAL_SELECTION` | highlight med | `#403d52` | `#44415a` |

## Typography

Chrome uses Inter (bundled with the kit) in 10–13 pt steps, weights about 450 (body), 560 (medium), 620 (strong); group labels are 10.5 pt small capitals with 0.6 tracking. The terminal family, ordered explicit fallbacks and 8–48 pt size are saved locally; `--font`, repeated `--font-fallback` and `--font-size` override them for one launch. The default is JetBrainsMono Nerd Font 15, explicit fallbacks precede fontconfig's automatic chain, and unavailable primaries use bundled IBM Plex Mono. Family changes apply after reopening the client. Cell width follows the primary font's advance; ligatures and kerning are off so the grid stays exact. Names keep their case and ellipsize when space is bounded, with tooltips for the full text. Server windows are called **tabs**.

## Layout and shape

- **Toolbar:** 52 pt chip bar. Controls sit on glassy chips (10 pt radius, 5.5% white fill, gradient hairline border). Tab pills are 96–240 pt wide with no close button: a tab closes from its right-click menu, a middle-click or `Ctrl Shift W`. Commands (⌘) and New tab (+) are 34 pt icon chips on the right. A pill shows its focused pane's program icon, its label, and then a word for program activity: "Needs approval", "Question", "Needs sign-in" or "Needs input" in `WARNING`, "Error" in `DANGER`, unseen "Done" in `ACCENT`; idle and working tabs show no word. A tab still called "Shell", the name new tabs get, shows its focused pane's terminal title instead while the program that set it is in the foreground; a tab the user named keeps its name. The label truncates before the word does; the program's message is the word's tooltip. The empty space moves the window; edges and corners resize it; minimize/maximize/close are at the right end.
- **Single pane:** a tab with one pane, not zoomed, draws it edge to edge under the toolbar with no card, header, border or margin. Its exception strip floats at the pane's top-right corner.
- **Pane cards:** splits, and a zoomed pane, are rounded cards (12 pt radius) with a 30 pt header (pane name and, when needed, its exception strip), 8 pt gutters and 10 pt outer margin on the ground; their border separates them from the same-coloured ground. Tiled tabs use the server placements. In a scrolling tab, top-level horizontal parts become client-sized columns clipped by the viewport: half-window by default, 0.1–1 window after adjustment, persisted by host/session/window/column. One column fills the viewport; splits inside a column keep the server ratios, and floating layers stay in server window coordinates. Dragging a full-height column boundary preserves the adjacent pair's total width; ordinary divider drags preview immediately and send a server ratio clamped to 0.1–0.9. Terminal text has 8 px padding inside its card.
- **Sidebar:** optional; it opens at launch as it was left, at its saved width. Default 236 pt wide (200–360 pt, drag the edge to resize, double-click to reset). Host rows are 28 pt, session rows 32 pt, groups separated by 12 pt.
- **Popovers:** session/host menus anchor to the identity; pane and tab menus to the pointer (right-click), and pane menus also to the toolbar; the command menu is centered near the top. Popovers are compact: the switcher 340 pt wide, Commands 460, pane menus 260; 28 pt rows (30 in Commands), 26 pt group headers, a 34 pt search field, 5 pt padding and a 12 pt radius. Popovers clamp to the viewport; long lists scroll with directional hints that count hidden rows. A popover never opens another popover from its rows: the switcher's highlighted session row trades its detail for pencil and × buttons, and renaming turns the row itself into a field (accent outline) until Enter or Esc.
- **Dialogs** (forms, confirmation) are centered over a dark scrim and sized to their content within the viewport. **Settings** is a larger dialog (760×520, clamped to the viewport) with a 190 pt section list on the left and the section's rows on the right; each row names the setting, explains it in one muted line and puts its control on the right. Forms and confirmations opened from Settings return to it.
- **Radii:** kit small 8, medium 10; chips 10; cards and popovers 12.
- **Icons:** authored strokes on a 20×20 view box, 1.5 stroke, round caps and joins, `currentColor`. Hosts show their kind (a monitor for this machine's Unix socket, a server for a remote endpoint) in the identity chip, sidebar headers and switcher groups; tabs carry their program's icon at 15 pt (Claude Code, Codex, OpenCode, Pi, Vim, Git, Cargo, SSH), or `>_` for shells and anything else. Pi and OpenCode use solid marks after their own logos, since outlines of them are unrecognisable at that size; the Commands button is ⌘. Magnifying glasses mark search fields only.
- **Elevation:** the working canvas is flat. Popovers and dialogs get a frosted fill, a lit gradient rim and a deep soft shadow.
- **Interface zoom:** 70–150% in 10% steps. Zoom above 100% is limited to preserve a 640×480 layout floor where the viewport allows; terminal font size is independent.

## Interaction

- Workspace shortcuts are listed in the README; they are reserved by the client and never reach the terminal.
- Opening a form or search focuses its first field; close confirmation focuses **Keep running**. Tab and Shift-Tab cycle fields and actions in panels; Enter and Space activate; Escape backs out and restores the previously focused pane. Tab, Space, Return, Escape and Menu never move focus away from the terminal unless a panel is open.
- Right-click captures the specific pane under the pointer; its menu names tab and pane and keeps a target outline through rename and close. A target that disappears dismisses the menu with a notice instead of retargeting.
- Size ownership is automatic: the active window owns terminal size and typing claims it. A non-owner or reconnecting terminal shows an exception strip with **Take control** or **Retry**, and never sends a resize.
- A transport failure marks affected panes input-paused, keeps their output, and never queues or replays input. Notices are toasts: errors are sticky, information leaves after about 6 seconds, and at most three notices are retained.
- Focus cues: the focused terminal uses the program-requested block, bar, underline or hollow cursor; other panes use a hollow cursor. The focused pane has an accent outline when no panel is open. Cursor blink requests stay visually steady with the rest of terminal content.
- Links open only on Ctrl+click (press and release on the same link), never on a plain click, so selecting text and clicking into programs stay unchanged. Holding Ctrl over a link underlines it in its own text colour and shows the hand pointer.
- A horizontal wheel or trackpad gesture over a scrolling tab pans its strip immediately; a precise gesture springs to the nearest column edge when it ends. Focus movement follows the Interface policy: reveal only, center when the previous and new columns cannot fit together, or always center. **Center column** centers explicitly. `Ctrl Shift S` cycles 1/3, 1/2 and 2/3 widths; `Ctrl Shift M` maximizes and restores. Split right creates a new top-level column after the focused whole column. Single-pane columns can swap left or right; a pane can move vertically into an adjacent column or horizontally out of its multi-pane column. These actions use server `swap_blocks` and `move_block`, keep focus on the moved block and migrate local widths after the server confirms the topology. Multi-pane whole-column movement stays unavailable until the server can move a split subtree atomically. Vertical wheel over terminal content remains terminal scrollback or program mouse input.

## Do and don't

- Do keep the toolbar single-row and the terminal canvas dominant.
- Do keep chrome tokens in `theme.rs` rather than hard-coding colours in views.
- Do derive overflow cues and hidden-row counts from measured content.
- Don't recolour terminal output to match the chrome, or animate terminal content.
- Don't let selecting, copying or navigating claim size ownership.
- Don't add status dots, "Connected" labels, terminal size readouts or keyboard-hint footers.
- Don't show illustrative sessions, layouts or captures as shipped runtime state.
