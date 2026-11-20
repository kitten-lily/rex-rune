# Hide an Offline Host Until It Reconnects — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An offline host can be hidden from the sidebar's grouped host list with one press, and comes back by itself when it connects.

**Architecture:** One saved boolean per host, `Host::hidden_offline`, set only by an explicit user action on an offline host and cleared in `Workspace::sync` when that host's model is connected. The sidebar's grouped list and the sidebar-tabs prefetch skip marked hosts; nothing else reads the mark. No protocol or connection-worker change.

**Tech Stack:** Rust, serde, Rune / rune-kit; headless UI tests in `src/app/tests.rs`.

**Spec:** `docs/superpowers/specs/2026-11-20-hide-offline-hosts-design.md`

## Global Constraints

- Work in the worktree `../rex-rune-hide-offline-hosts`, branch `feature/hide-offline-hosts`.
- Hiding sends nothing to any server. The only `HostCommand` code in this plan may cause is the local `PrefetchViews` that `sync_prefetch` already sends when a host stops being listed; never `Reconnect`, a session change or anything else.
- A connected host is never hidden: every path that sets the mark checks `!model.connected` first, and `sync` clears it on connect.
- The mark is `hidden_offline`, `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`: an existing `workspace.json` loads unchanged and a host that is not hidden writes no new key.
- The Flat session list, Activity mode, the switcher, the identity menu and Settings → Hosts are not changed.
- Exact copy: menu row `Hide until it reconnects`; × button label and help `Hide {label} until it reconnects`; empty-list note `1 offline host hidden` / `{n} offline hosts hidden`, help `Find a session lists them`.
- Element keys: `host-hide:{index}` (the ×, replacing `host-new-session-slot:{index}`), `hosts-hidden` (the note).
- `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings` and `cargo test --locked --all-features` stay clean.
- UI follows `DESIGN.md`; DESIGN.md, `docs/configuration.md` and `CHANGELOG.md` change in the same branch.

## Review Focus

1. **Old or hand-edited `workspace.json`** without the key, or with `"hidden_offline": true` on a host that is connected at launch: loads, and the mark is cleared on the first connected sync. Tests in Task 1.
2. **The host connects between opening its menu and choosing Hide** (or the × is pressed on a stale frame): nothing is hidden, and Enter does not run whichever row took Hide's place. Tests in Tasks 1 and 2.
3. **A host that reconnects and then drops again**: shown offline with its Retry row, not hidden again. Test in Task 1.
4. **Every host hidden**: the list shows the count note rather than nothing; one host hidden among several shows no note. Test in Task 2.
5. **Endpoint edited while hidden**: the mark is cleared, so the new server's host is visible; a label or shell edit keeps it. Test in Task 1.
6. **Getting a hidden host back by hand**: Retry from Find a session and from the identity menu still sends exactly one Reconnect for it. Test in Task 2.

---

### Task 1: The mark and its rules

**Files:**
- Modify: `src/settings.rs` (`Host`, and the `Host { .. }` literal near line 584)
- Modify: `src/app/workspace.rs` (`sync` near the `self.hosts[index].model = model` assignment, line ~627; `perform`'s `Intent::RetryHost` neighbourhood, line ~1558; `edit_host`'s `if host.endpoint != endpoint` block, line ~514 (the `Form::EditHost` arm only delegates to it); `sync_prefetch`, line ~1181; `Host` literals at ~241, ~2471, ~2486)
- Modify: `src/app/panel.rs` (`Intent`)
- Modify: every other `Host { .. }` literal so it compiles: `src/app/discover.rs:482`, `src/app/about.rs:511`, `src/app/font_settings_tests.rs:815`, `src/app/menu_tests.rs:102`, `src/app/tests.rs:117`
- Test: `src/app/tests.rs`, `src/settings.rs` tests

**Interfaces:**
- Produces: `pub hidden_offline: bool` on `rex_rune::settings::Host`.
- Produces: `Intent::HideHost(usize)` in `src/app/panel.rs`; `perform` routes it to `hide_offline_host`.
- Produces: `pub fn hide_offline_host(&mut self, host: usize)` on `Workspace`: when `!self.hosts[host].model.connected`, sets the mark and calls `self.save()` and `self.sync_prefetch()`; otherwise does nothing.

- [ ] **Step 1: Write the failing tests** in `src/app/tests.rs`, using `workspace_hosts`, `capture_host` and the existing pattern for a worker-side model (`ws.hosts[i].connection.model.lock().unwrap().connected = true; ws.sync(i, &mut rune::Tx::default())`):

```rust
#[test]
fn hiding_marks_only_an_offline_host_and_sends_nothing() {
    // hosts 0 (connected) and 1 (offline), both captured
    ws.hide_offline_host(0);
    assert!(!ws.settings.hosts[0].hidden_offline, "a connected host is never hidden");
    ws.hide_offline_host(1);
    assert!(ws.settings.hosts[1].hidden_offline);
    assert!(Settings::load(&ws.settings_path).unwrap().hosts[1].hidden_offline, "saved");
    // host 1 is expanded with cached sessions and the sidebar lists tabs, so hiding
    // changes what is prefetched: that one local command is all that may go out
    assert!(commands(&rx0).is_empty());
    assert!(commands(&rx1).iter().all(|c| matches!(c, HostCommand::PrefetchViews(_))));
}

#[test]
fn a_hidden_host_returns_when_it_connects_and_stays_after_it_drops() {
    // host 1 hidden; its shared model becomes connected; sync(1)
    assert!(!ws.settings.hosts[1].hidden_offline);
    assert!(!Settings::load(&ws.settings_path).unwrap().hosts[1].hidden_offline, "saved");
    // shared model disconnected again; sync(1)
    assert!(!ws.settings.hosts[1].hidden_offline, "a later drop does not hide it again");
}

#[test]
fn hide_chosen_after_the_host_connected_hides_nothing() {
    // host 1 offline; open its menu with host_menu(1, ..) and move the keyboard selection
    // to "Hide until it reconnects"; make the shared model connected and sync(1), which
    // drops Retry and Hide from the list; then submit through the real menu (Enter)
    assert!(!ws.settings.hosts[1].hidden_offline);
    assert!(creates(&rx1).is_empty(), "and the row that took its place did not run");
    // the notice is "That row is no longer in the list. Nothing was changed."
    // Also: perform(Intent::HideHost(1)) directly on the connected host hides nothing.
}

#[test]
fn editing_a_hidden_hosts_endpoint_shows_it_again() {
    // host 1 hidden; submit the Edit host form with a different endpoint, as the
    // existing edit-host test near tests.rs:2023 does
    assert!(!ws.settings.hosts[1].hidden_offline);
    // hide it again, then change only its label and shell
    assert!(ws.settings.hosts[1].hidden_offline, "the same server stays hidden");
}

#[test]
fn a_hidden_host_is_not_prefetched_for_sidebar_tabs() {
    // as sidebar_tabs_still_prefetch_an_expanded_host_while_it_reconnects, then hide host 0
    assert_eq!(latest(&rx), Some(vec![]));
}
```

and in `src/settings.rs` tests:

```rust
#[test]
fn hidden_offline_defaults_off_and_is_written_only_when_set() {
    let host: Host = serde_json::from_str(r#"{"label":"a","endpoint":"unix:///a"}"#).unwrap();
    assert!(!host.hidden_offline);
    assert!(!serde_json::to_string(&host).unwrap().contains("hidden_offline"));
    let hidden = Host { hidden_offline: true, ..host };
    assert!(serde_json::to_string(&hidden).unwrap().contains(r#""hidden_offline":true"#));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --locked --all-features hidden` and `... hiding_marks`
Expected: compile errors for `hidden_offline`, `hide_offline_host`, `Intent::HideHost`.

- [ ] **Step 3: Add `Host::hidden_offline`** in `src/settings.rs` after `collapsed`, with the doc comment "Left out of the sidebar's host list until it next connects; set only while offline." Add `hidden_offline: false` to every `Host` literal listed above.

- [ ] **Step 4: Add `Intent::HideHost(usize)`** beside `RetryHost` in `src/app/panel.rs`, and `Workspace::hide_offline_host` plus its `perform` arm in `src/app/workspace.rs`. It is a menu intent like `RetryHost`, not a named action: no `actions.rs` entry and no Commands row (it applies only to a host the pointer is on).

- [ ] **Step 5: Clear the mark in `Workspace::sync`**: where the fresh `model` is stored, if `model.connected && self.settings.hosts[index].hidden_offline`, set it false and `self.save()`.

- [ ] **Step 6: Clear the mark when Edit host changes the endpoint**, in `Workspace::edit_host`'s `if host.endpoint != endpoint` block beside `remote_control`, `keep_tailscale` and `tailscale_name`, before the replacement connection is spawned.

- [ ] **Step 7: Skip hidden hosts in `sync_prefetch`**: the `listing && (flat || !collapsed)` term also requires `!hidden_offline` in the non-flat case.

- [ ] **Step 8: Run the tests**

Run: `cargo test --locked --all-features`
Expected: PASS, including the six new tests. (The stale-menu test passes only after Task 2 adds the row and its identity; write it in Task 1 against `perform` and extend it in Task 2.)

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "Hide an offline host until it reconnects: the saved mark and its rules"
```

### Task 2: The sidebar

**Files:**
- Modify: `src/app/sidebar.rs` (`host_header`, `session_list`)
- Modify: `src/app/panel.rs` (`host_entries`)
- Modify: `DESIGN.md`, `docs/configuration.md`, `CHANGELOG.md`
- Test: `src/app/tests.rs`

**Interfaces:**
- Consumes: `Host::hidden_offline`, `Intent::HideHost(usize)`, `Workspace::hide_offline_host(&mut self, host: usize)` from Task 1.

- [ ] **Step 1: Write the failing tests** in `src/app/tests.rs`:

```rust
#[test]
fn an_offline_hosts_x_hides_its_group_until_it_connects() {
    // three hosts as host_plus_creates_on_its_own_host_at_once_with_its_shell: 0 and 1
    // connected, 2 offline with a cached session "away"; sidebar open
    assert!(has(&rt, "host-hide:2") && !has(&rt, "host-hide:0"));
    click(&mut rt, "host-hide:2");
    for key in ["host:2", "retry-host:2", "session:2:away", "hosts-hidden"] {
        assert!(!has(&rt, key), "{key}");
    }
    assert!(has(&rt, "host:0") && has(&rt, "host:1"));
    // shared model of host 2 connected; sync(2); frame
    assert!(has(&rt, "host:2") && has(&rt, "host-new-session:2"));
}

#[test]
fn the_host_menu_offers_hide_only_while_offline() {
    // labels of host_entries for an offline host contain, in order,
    // "Retry connection", "Hide until it reconnects"; for a connected host neither.
    // Choosing the row hides the host.
}

#[test]
fn with_every_host_hidden_the_list_says_how_many() {
    // two offline hosts, both hidden
    assert!(has(&rt, "hosts-hidden"));
    // its text is "2 offline hosts hidden"; with one host, "1 offline host hidden"
}
```

```rust
#[test]
fn a_hidden_host_is_still_retried_from_find_a_session_and_the_identity_menu() {
    // host 1 offline and hidden. Open Find a session (the sidebar chip) and run host 1's
    // "Retry connection" row; then the same from the toolbar's identity menu.
    assert_eq!(reconnects(&rx1), 1); // after each: exactly one HostCommand::Reconnect
    assert!(reconnects(&rx0) == 0);
    assert!(rt.state.settings.hosts[1].hidden_offline, "still hidden until it connects");
}
```

Update the two existing tests this changes: `host_plus_creates_on_its_own_host_at_once_with_its_shell` (line ~355: the offline host now has `host-hide:2` instead of `host-new-session-slot:2`) and `host_menu_rows_online_and_offline` (the offline row list gains the new row).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --locked --all-features host`
Expected: FAIL on the missing `host-hide:2` and the missing menu row.

- [ ] **Step 3: The × in `host_header`**: replace the empty `host-new-session-slot` element with a button built like the + beside it (22×22, radius 6, same hover/press, `tab_index(-1)`, `preserve_focus_on_press`) with `glyph(&CLOSE, 13., rgb(muted()))`, key, label and help from Global Constraints, pressing `s.hide_offline_host(index)` then `tx.blur()`. Update the comment above it.

- [ ] **Step 4: Skip hidden hosts in `session_list`** (grouped branch only): `continue` for a host whose mark is set and whose model is not connected. If no group was added and at least one host is hidden, add the `hosts-hidden` note (the existing `note(text, muted())` helper, `NOTE_ROW` high) with the copy and help from Global Constraints.

- [ ] **Step 5: The menu row in `host_entries`**: inside the existing `if !connected` block, after "Retry connection", push `Entry::new("Hide until it reconnects", "", "", Intent::HideHost(host)).in_section(1)`. Give this row and the "Retry connection" row above it stable identities (`Entry::id`: `host.hide`, `host.retry`), as Commands rows have, so `submit` (workspace.rs ~2355) runs a keyboard-chosen row by identity: when a reconnect drops both rows under the selection, Enter runs nothing and says so instead of running the row that moved up. Extend `hide_chosen_after_the_host_connected_hides_nothing` to go through the menu as its comment describes. Update the function's doc comment.

- [ ] **Step 6: Run the tests**

Run: `cargo test --locked --all-features`
Expected: PASS.

- [ ] **Step 7: Docs.** `DESIGN.md` sidebar paragraph: an offline host's header has a × in the + slot that hides its group until it connects; with every host hidden the list shows the count note. `docs/configuration.md`: `hidden_offline` in the saved-hosts list (line ~37) and a sentence in the sidebar section saying where a hidden host can still be reached (Find a session, the identity menu, Settings → Hosts). `CHANGELOG.md` → Unreleased → Added: one line.

- [ ] **Step 8: Look at it rendered.** Build, run with a copy of `workspace.json` (`--config`) that has one saved host whose endpoint does not exist, and confirm by eye: the × lines up under the other hosts' +, hiding removes the group without the list jumping, and the note reads well with every host hidden.

- [ ] **Step 9: Full checks and commit**

Run: `cargo fmt --all -- --check && cargo clippy --locked --all-targets --all-features -- -D warnings && cargo test --locked --all-features`
Expected: all clean.

```bash
git add -A && git commit -m "Hide an offline host until it reconnects: sidebar ×, menu row and docs"
```
