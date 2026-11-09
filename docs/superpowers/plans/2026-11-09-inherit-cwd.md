# New Panes Inherit the Working Directory — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A split or new tab started from a pane opens its shell in that pane's current directory instead of `~`.

**Architecture:** The UI marks a new-terminal request with the block it came from. The host worker, just before sending it, asks the server `com.superlogical.terminal.process` for that block and takes `child.cwd` (the shell's directory, read by the server from the process itself, so it needs no OSC 7 and is always a path on the server's machine). If it gets one, it sets `layout.block.options.cwd`; otherwise the request goes out unchanged and the server uses its default, as today.

**Tech Stack:** Rust, tokio, serde_json, rexrs control API.

**Spec:** this conversation's findings (no separate doc), measured against a private `rex server run`:
- `terminal.pwd` is only what a program last reported via OSC 7: empty for shells that don't emit it, stale after a `cd` without it, and it accepts other hosts' paths (`file://otherhost/...`).
- `terminal.process` returns `child.cwd` / `foreground.cwd` as absolute paths read from the process. Documented as "Left out if it can't be read".
- `options.cwd` on a shell block is honoured when the directory exists. A missing path, a file, or a relative path silently starts in `~`, and the request still succeeds. So the server already provides the fallback.

## Global Constraints

- Ordinary attachment never creates or destroys anything; this change only adds data to requests the user already triggered.
- Failed input is never replayed; a failed `process` lookup must not fail, delay beyond its timeout, or retry the split/tab request.
- Keep `cargo clippy --locked --all-targets --all-features -- -D warnings` and `cargo fmt --all -- --check` clean.
- Directory lookup timeout: **1 s**. After that the request goes out without `cwd`. The lookup runs inline in the host worker, as every `HostCommand::Request` already does (each waits up to 8 s in `handle_command`), so it adds no new kind of stall. A detached lookup with pending-creation state was considered (Codex review) and rejected as more machinery than a sub-second lookup warrants.
- Source of the directory: the **shell's** cwd. Normally `child`; but on a macOS server `child` is `login` wrapping the shell (rexrs `ProcessResponse::child` doc), so when `child.name == "login"` the shell is the ancestor just below it (`ancestors[len-2]`; `ancestors` is nearest-first and ends with child) or, when `ancestors` is `[login]` or empty, `foreground`. Not `foreground.cwd` in general (an editor or `ssh` started from the shell may sit elsewhere), and not the OSC 7 `cwds` map (stale or remote; see Spec).
- New sessions (`HostCommand::CreateSession`) are unchanged: there is no source pane.

## Review Focus

1. **The `process` call fails or times out** (server error, block closed meanwhile, other user's process): the split must still be sent, without `cwd`. Test in Task 2.
2. **Reply has no `child` or no `child.cwd`** (command exited in a `--keep-open` pane; unreadable): no `cwd` key at all, not an empty string. Test in Task 1.
3. **Non-absolute or empty `cwd` string:** ignored. Test in Task 1.
6. **macOS `login` wrapper:** the shell under `login` supplies the directory, not `login`. Test in Task 1.
7. **Unanswered lookup:** the split goes out exactly once, without `cwd`, after the 1 s timeout. Test in Task 2.
4. **Scrolling view split anchored on a column ID:** the anchor is `column:…` but the directory must come from the focused *block*. Test in Task 3 (existing scrolling-view split test gets the assertion).
5. **New tab with no focused block** (empty window): request goes out as plain `HostCommand::Request`, no lookup. Test in Task 3.

---

### Task 1: Layout carries `cwd`; parse the shell's directory from a `process` reply

**Files:**
- Modify: `src/layout.rs` (add `with_cwd`, plus test)
- Modify: `src/connection/activity.rs` (add `shell_dir`, plus test in its test module, or in `src/connection/tests.rs` beside `program_names_prefer_the_script_and_drop_login_dashes`)

**Interfaces:**
- Produces: `pub fn with_cwd(layout: &mut serde_json::Value, cwd: &str)` in `crate::layout`: sets `layout["block"]["options"]["cwd"]`. It does nothing when `layout` has no `"block"` object (a split layout).
- Produces: `pub fn shell_dir(process: &serde_json::Value) -> Option<String>` in `crate::connection::activity`: the shell's `cwd` (per Global Constraints, unwrapping `login`) when it is a non-empty string starting with `/`.

- [ ] **Step 1: Write the failing tests**

```rust
// src/layout.rs
#[test]
fn with_cwd_sets_the_blocks_directory_only() {
    let mut layout = shell_layout(Some("zsh"), None);
    with_cwd(&mut layout, "/srv/app");
    assert_eq!(layout["block"]["options"], json!({"shell":"zsh","cwd":"/srv/app"}));
    let mut bare = shell_layout(None, None);
    with_cwd(&mut bare, "/srv/app");
    assert_eq!(bare["block"]["options"], json!({"cwd":"/srv/app"}));
    let mut split = json!({"split":{}});
    with_cwd(&mut split, "/srv/app");
    assert_eq!(split, json!({"split":{}}));
}

// activity tests
#[test]
fn the_shells_directory_comes_from_the_child_only() {
    assert_eq!(shell_dir(&json!({"child":{"cwd":"/srv/app"},"foreground":{"cwd":"/tmp"}})).as_deref(), Some("/srv/app"));
    assert_eq!(shell_dir(&json!({"foreground":{"cwd":"/tmp"}})), None);
    assert_eq!(shell_dir(&json!({"child":null})), None);
    assert_eq!(shell_dir(&json!({"child":{"cwd":""}})), None);
    assert_eq!(shell_dir(&json!({"child":{"cwd":"rel/dir"}})), None);
    // macOS: login wraps the shell.
    let login = json!({"name":"login","cwd":"/"});
    let zsh = json!({"name":"zsh","cwd":"/Users/me/app"});
    let vim = json!({"name":"vim","cwd":"/tmp"});
    assert_eq!(shell_dir(&json!({"child":login,"foreground":zsh,"ancestors":[login]})).as_deref(), Some("/Users/me/app"));
    assert_eq!(shell_dir(&json!({"child":login,"foreground":vim,"ancestors":[zsh,login]})).as_deref(), Some("/Users/me/app"));
    assert_eq!(shell_dir(&json!({"child":login})), None);
}
```

- [ ] **Step 2: Run, expect compile failure** — `cargo test --locked --all-features --lib with_cwd_sets` and `cargo test --locked --all-features --lib the_shells_directory` → FAIL (`with_cwd`/`shell_dir` not found).
- [ ] **Step 3: Implement both functions** as specified in Interfaces.
- [ ] **Step 4: Run the tests again** → PASS.
- [ ] **Step 5: Commit** — `git commit -m "Add a layout cwd option and read the shell's directory from process replies"`

### Task 2: Host worker looks up the directory before opening a terminal

**Files:**
- Modify: `src/connection.rs` (`HostCommand` enum near line 381; worker dispatch near line 1562; reconnect drain near line 1374 needs no change since `_ => {}` already drops it, which matches `Request`)
- Test: `src/connection/tests.rs`

**Interfaces:**
- Consumes: `layout::with_cwd`, `activity::shell_dir` (Task 1).
- Produces: new variant
  ```rust
  /// A request that opens a terminal (`session.new_split` / `session.new_window`) whose
  /// layout is `payload["layout"]`; it starts in the shell directory of `cwd_from`, a block
  /// of the payload's session, when the server can tell within 1 s.
  NewTerminal { method: String, payload: Value, cwd_from: String },
  ```
- Worker handling: `let session = payload["session_id"]`; call `request`-like with a **1 s** timeout on `"com.superlogical.terminal.process"` with `{"session_id":session,"block_id":cwd_from,"args":{}}`. On `Ok(reply)`, `if let Some(dir) = activity::shell_dir(&reply) { layout::with_cwd(&mut payload["layout"], &dir) }`. Any error/timeout is ignored (`.ok()`), **never `?`**. Then exactly the `HostCommand::Request` path: `request(control, &method, payload).await?; refresh_after_request(...)`. Factor a small `async fn shell_dir_of(control, session, block) -> Option<String>` beside `request`, using `tokio::time::timeout(Duration::from_secs(1), control.call_raw(...))`. A transport failure during the lookup is also swallowed there; the creation request that follows then fails on the same dead transport and takes the existing disconnect path, so nothing is replayed.

- [ ] **Step 1: Write the failing tests** in `src/connection/tests.rs`, modelled on `unselected_sessions_are_probed_without_blocking_commands`, but with `boot_with(&[])`, so there are no prefetches. The bootstrap session's view has no terminal blocks, so no probes interleave; if probes do appear, use a view without terminal blocks.

```rust
#[tokio::test]
async fn new_terminals_start_in_the_source_shells_directory() {
    // server: after bootstrap,
    //   let p = next(&mut ws).await;
    //   assert_eq!(p["method"], "com.superlogical.terminal.process");
    //   assert_eq!(p["payload"], json!({"session_id":SESSION,"block_id":"block:a","args":{}}));
    //   reply(&mut ws, &p, 3, json!({"child":{"cwd":"/srv/app"},"foreground":{"cwd":"/tmp"}})).await;
    //   let split = next(&mut ws).await;
    //   assert_eq!(split["method"], "session.new_split");
    //   assert_eq!(split["payload"]["layout"]["block"]["options"], json!({"shell":"zsh","cwd":"/srv/app"}));
    //   reply + answer the session.view refresh.
    // client: connected(&host); host.commands.send(HostCommand::NewTerminal {
    //   method: "session.new_split".into(),
    //   payload: json!({"session_id":SESSION,"anchor_id":"block:a","direction":"horizontal","side":"after",
    //                   "layout":{"block":{"flavor":"com.superlogical.terminal.shell","options":{"shell":"zsh"}}},"focus":true}),
    //   cwd_from: "block:a".into() })
}

#[tokio::test]
async fn a_failed_directory_lookup_still_opens_the_terminal() {
    // same, but the process request gets
    //   {"type":"response","id":p["id"],"error":{"code":"not_found","message":"no","status":404}}
    // and the new_split payload's options equal {"shell":"zsh"} (no "cwd" key);
    // and the host stays connected (model.connected) afterwards.
}

#[tokio::test]
async fn an_unanswered_directory_lookup_opens_the_terminal_once_without_one() {
    // the process request is never answered; await the split with a 4 s timeout around recv()
    // (not next(), whose own 2 s limit races the 1 s fallback plus slack);
    // assert it is session.new_split with options {"shell":"zsh"}; then answer it and the refresh;
    // then reply late to the process request and assert nothing further is sent before Close.
}
```

- [ ] **Step 2: Run** `cargo test --locked --all-features --lib directory` and `... --lib new_terminals_start` → FAIL (no variant).
- [ ] **Step 3: Implement** the variant and worker arm as in Interfaces.
- [ ] **Step 4: Run** the two tests → PASS. Run the whole `connection` test module → PASS.
- [ ] **Step 5: Commit** — `git commit -m "Start new terminals in the source pane's shell directory"`

### Task 3: Workspace sends splits and new tabs as `NewTerminal`

**Files:**
- Modify: `src/app/workspace.rs`: `Intent::NewWindow` (~1875), `Intent::Split` (~1879), `PaneAction::Split` in `pane_action` (~2208); add a helper next to `request` (~1578).
- Test: `src/app/tests.rs`: `requests()` helper (~1282), `tabs_and_splits_start_the_right_hosts_shell` (~398), the scrolling-view split test (~1110).

**Interfaces:**
- Consumes: `HostCommand::NewTerminal` (Task 2).
- Produces: `fn open_terminal(&mut self, host: usize, method: &str, payload: Value, session: &str, cwd_from: Option<&str>)` on `Workspace`. It sets `payload["session_id"] = session`, then sends `NewTerminal { cwd_from }` when `cwd_from` is `Some`, else plain `Request`.
- Sources:
  - `Intent::NewWindow`: `cwd_from = model.focused()` of the selected session (the active tab's focused block). Session = `model.selected`, exactly as `request` uses today.
  - `Intent::Split`: `cwd_from = Some(block)` (the focused block it already reads), **not** `anchor`.
  - `PaneAction::Split`: `cwd_from = Some(&target.block)`, host `target.host`, session `target.session`.

- [ ] **Step 1: Update the test helper and write failing assertions.** In `requests()`, add an arm `HostCommand::NewTerminal { method, mut payload, cwd_from } => { payload["cwd_from"] = cwd_from.into(); let session = payload["session_id"].as_str()?.to_owned(); Some((method, session, payload)) }`, so existing tests keep reading `(method, session, payload)`. Then:
  - `tabs_and_splits_start_the_right_hosts_shell`: give host 0's model a view for `"cur"` with focused block `block:f`. Assert `sent[0].2["cwd_from"] == "block:f"` for the new window, and `== "block:1"` for the pane-menu split.
  - Scrolling-view split test (~1112): assert `sent[0].2["anchor_id"] == "column:eg"` (existing) **and** `sent[0].2["cwd_from"] == "block:e"` (the fixture's focused block, not the column).
  - New test `a_new_tab_in_an_empty_window_asks_for_no_directory`: a selected session whose window has no `focused_block_id`. `Intent::NewWindow` sends a plain `HostCommand::Request` for `session.new_window` (assert via a direct `try_iter` match that it is the `Request` variant).
- [ ] **Step 2: Run** `cargo test --locked --all-features --bin rex-linux` → the three changed/new tests FAIL.
- [ ] **Step 3: Implement** `open_terminal` and switch the three call sites to it.
- [ ] **Step 4: Run** the full suite `cargo test --locked --all-features` → PASS. Then run `cargo fmt --all -- --check` and `cargo clippy --locked --all-targets --all-features -- -D warnings` → clean.
- [ ] **Step 5: Commit** — `git commit -m "Open splits and tabs in the directory of the pane they come from"`

### Task 4: Server-contract check (not application acceptance)

**Files:**
- Create: `scripts/process-cwd-contract.py`. A protocol-level check like `/tmp/…/cwd_probe.py`: a private `rex server run` with a throwaway HOME, no UI.
- Modify: `README.md`, one line where splits/tabs are described: new panes open in the source shell's directory when the server can read it.

The UI and worker paths are covered by Tasks 2–3; this script checks the server contract the worker relies on. In a `/bin/sh` pane (no OSC 7) it runs `cd /usr/share`, then asserts that `terminal.process` gives `child.cwd == "/usr/share"` (the unmeasured link: `child.cwd` follows `cd` without OSC 7). It then asserts that `session.new_split` with `options.cwd` set to that directory starts a shell whose `pwd` prints `/usr/share`.

- [ ] **Step 1:** Write the script (docstring: requirements `rex`; starts its own server; touches nothing else).
- [ ] **Step 2:** Run `python3 -I scripts/process-cwd-contract.py` → prints `ok`.
- [ ] **Step 3:** Update the README line; commit — `git commit -m "Check the server contract new terminals' directories rely on"`.
