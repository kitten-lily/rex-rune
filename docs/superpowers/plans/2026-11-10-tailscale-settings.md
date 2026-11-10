# Tailscale Controls in Settings — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** For a local (`unix://`) host, Settings turns the server's built-in Tailscale on and off, names its device, shows its state, and (only when the user opts in) keeps it on and named across server restarts. For any host whose tailnet HTTPS is available, Settings shows a pairing QR code.

**Architecture:** A UI-free module `src/connection/tailscale.rs` holds the naming rules and a small per-connection reconciler that decides the one call (if any) to make from a status. The host worker keeps the status in `HostModel` (from `session.bootstrap`'s `server_status.tailscale`, call replies, and `tailscale_status_changed`), performs explicit calls the UI sends, and runs the reconciler only for hosts whose saved opt-in reached it at spawn (`Startup::keep_tailscale`) or by an explicit change (`HostCommand::KeepTailscale`). Status events pass a Tailscale-specific sequence floor so a buffered older event never overwrites a newer snapshot or reply. The UI owns the local-only gate, the saved per-host opt-in, and the Settings rows and pairing dialog in a new `src/app/tailscale.rs`.

**Tech Stack:** Rust, tokio, serde_json, rexrs (`session::Tailscale`), Rune / rune-kit, new dependency `qrcode`.

**Spec:** GitHub issue kitten-lily/rex-rune#8 and its comment (pairing code). API facts below are from `vendor/rexrs/src/api/generated/server.rs` (`TailscaleEnable`, `TailscaleDisable`, `TailscaleRename`, `Status`) and `session.rs` (`Tailscale`, `Http`, `BootstrapResponse::server_status`).

## Global Constraints

- **Opt-in, off by default, per saved host.** Nothing calls `server.tailscale.*` on attach unless that host's `keep_tailscale` is true. Every other call is an explicit user action in Settings. Changing a host's endpoint in Edit host clears `keep_tailscale` and `tailscale_name`, as it clears `remote_control` (consent belongs to the saved host).
- **Local only:** the enable/disable switch, Keep switch and device name appear only for hosts whose endpoint starts with `unix://`. `ssh://` hosts are excluded in this plan (decision: the forward does reach the remote server's Unix socket, so the server may accept the calls, but the default name would be this machine's and enabling a remote machine's tailnet device from here is a bigger consent question; follow-up issue). The UI enforces the gate; the worker never decides it, so worker tests can use the TCP fake server.
- **Don't flip access:** the client has no setting of its own for `allow_tailnet_members`. Immediately before every `server.tailscale.enable` (explicit or automatic) the worker calls `server.status` and passes that reply's `tailscale.allow_tailnet_members`. A change made by someone else between that read and the enable can still be overwritten; the API has no "keep the current value" option, so this residual race is accepted and noted in the README (Codex review).
- **Sequence floor:** `ControlState::tailscale_seq` starts at the bootstrap reply's `seq` and rises to the `seq` of each Tailscale call reply and each applied `tailscale_status_changed`. An event with `seq <= tailscale_seq` is ignored.
- **Invalid saved name:** a `tailscale_name` that fails `valid_name` (hand-edited file) is kept for correction, shown as a problem in Settings, and suppresses every automatic call for that host until fixed. It never falls back to the default name.
- **Pairing needs a live connection:** offered and rendered only while `HostModel::connected`; a cached status from before a disconnect is not shown as a pairing code.
- **Device name:** 1–63 ASCII letters, digits, hyphens; not starting or ending with a hyphen (`TailscaleRename::hostname` doc). Default `rex-<first label of this machine's hostname>`, lowercased, other characters → `-`, trimmed of hyphens, truncated to 63 and re-trimmed; `rex` when the hostname yields nothing.
- **Name match:** the first DNS label of `dns_name` equals the wanted name (ASCII case-insensitive), or equals `<wanted>-<one or more digits>` (MagicDNS suffix).
- **No loops, no retries:** per connection, at most one automatic `enable` (only from the bootstrap status, and only if no newer `tailscale_status_changed` was applied before the decision, so a CLI disable during startup wins) and at most one automatic `rename` per wanted name. Never auto-enable when `state == "disabled"` and `error` is set. A failed call is shown in Settings and not retried.
- **Failed calls never drop the connection:** a `rexrs::Error::Api` from a Tailscale call sets `HostModel::tailscale_problem`; it is not returned as an error from the command loop.
- **Disconnected:** explicit calls made while disconnected get the existing "Action not sent while disconnected" notice and are never replayed. `KeepTailscale` is remembered across reconnects (like `Theme`).
- **Unknown states:** `state` unknown → treat as `starting`; `https.state` unknown → `pending` (rexrs docs).
- Pairing wording, from the CLI: "In Rex on your other device, tap Pair via QR Code. That device must be on the same tailnet." The QR encodes exactly `tailscale.https.url`.
- Keep `cargo fmt --all -- --check` and `cargo clippy --locked --all-targets --all-features -- -D warnings` clean; keep `--locked` working (commit `Cargo.lock`).
- Work in a worktree `../rex-rune-tailscale` on branch `tailscale-settings`.

## Review Focus

1. **HTTPS listener failure** (`state: disabled`, `error` set) on a host with Keep on: reconnecting must not re-enable; Settings shows the error and the switch still lets the user turn it on explicitly. Test in Task 1 (reconciler) and Task 2 (worker sends nothing).
2. **Rename reply still shows the old `dns_name`**, then an event with the old name arrives before the new one: exactly one `rename` goes out. Test in Task 2.
3. **Invalid configured name** (hand-edited `workspace.json`, or `bad_request` from the server): shown in Settings, no automatic call with it or with the default, connection stays up. Tests in Tasks 2, 3 and 4.
4. **Edit host changes the endpoint** of a host with Keep on: the opt-in and name are cleared so the new server is never reconfigured. Test in Task 3.
5. **Out-of-order events:** an older `tailscale_status_changed` buffered before the bootstrap reply or a call reply must not overwrite it; a CLI disable arriving right after bootstrap must stop the startup enable. Tests in Task 2.
6. **HTTPS stops, or the host disconnects, while the pairing dialog is open:** the dialog replaces the QR with an explanation rather than showing a dead URL. Test in Task 5.

---

### Task 1: Naming rules and the reconciler

**Files:**
- Create: `src/connection/tailscale.rs` (declare `pub mod tailscale;` in `src/connection.rs` next to the other submodules)
- Test: a `#[cfg(test)] mod tests` at the bottom of `src/connection/tailscale.rs`

**Interfaces:**
- Produces, all in `crate::connection::tailscale`:
  - `pub type Status = rexrs::api::session::Tailscale;`
  - `pub fn default_name(hostname: &str) -> String`
  - `pub fn valid_name(name: &str) -> bool`
  - `pub fn name_matches(dns_name: &str, wanted: &str) -> bool`
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub enum Call { Enable { hostname: String }, Disable, Rename { hostname: String } }`
  - `pub fn from_server(status: rexrs::api::server::Tailscale) -> Status` — the call replies use rexrs's `server::Tailscale`, a field-for-field twin of `session::Tailscale`; convert with a serde round trip.
  - `impl Call { pub fn verb(&self) -> &'static str }` — `"turn on Tailscale"`, `"turn off Tailscale"`, `"rename the Tailscale device"` (for problem text).
  - `#[derive(Default)] pub struct Keeper { … }` — per-connection memory (private fields: `enabled: bool`, `renamed: Option<String>`, `awaiting: bool`).
  - `impl Keeper { pub fn at_connect(&mut self, status: &Status, wanted: &str) -> Option<Call>; pub fn on_event(&mut self, status: &Status, wanted: &str) -> Option<Call>; }`
  - `pub fn parse_event(payload: &serde_json::Value) -> Option<Status>` — the inner payload of a `tailscale_status_changed` event, in exactly the shape Step 0 records (rexrs doesn't document it). No guessing between shapes.

Rules for `Keeper` (both methods share one private `fn next(&mut self, status, wanted, connect: bool)`):
- `state == "disabled"`: if `connect && status.error.is_none() && !self.enabled` → set `enabled`, return `Enable { hostname: wanted }`. Otherwise `None`.
- `state == "running"`: `None` while `awaiting`; `None` if `dns_name` is absent or `name_matches`; `None` if `renamed == Some(wanted)`; else set `renamed = Some(wanted)`, `awaiting = true`, return `Rename`.
- Any other state: `None`.
- `on_event` clears `awaiting` before deciding, and passes `connect = false`.

- [ ] **Step 0: Record the event's real shape.** Add `scripts/tailscale-event-contract.py`, modelled on `scripts/process-cwd-contract.py` (private `rex server run --listen <scratch socket> --restore=false --state-dir <scratch dir>`, so it never touches the user's server or Tailscale identity). It opens a control connection, calls `session.bootstrap` with `event_scope: "all"`, then `server.tailscale.enable {"allow_tailnet_members": false}`, prints the first `tailscale_status_changed` event frame verbatim, calls `server.tailscale.disable`, and stops the server. The device only reaches `needs_login`, so nothing joins a tailnet. If the server reports `tailscale` absent or `not_supported`, stop and report back; don't guess the shape. Run it with `python3 -I scripts/tailscale-event-contract.py`. Put the printed inner payload, verbatim, in the `events_parse` test below and in the Task 2 fixtures, and implement `parse_event` for that shape only.

- [ ] **Step 1: Write the failing tests**

```rust
fn status(state: &str, dns: Option<&str>, error: Option<&str>) -> Status {
    serde_json::from_value(json!({"state":state,"dns_name":dns,"error":error,
        "allow_tailnet_members":true})).unwrap()
}

#[test]
fn default_names_are_valid_device_names() {
    assert_eq!(default_name("Studio.local"), "rex-studio");
    assert_eq!(default_name("my_box"), "rex-my-box");
    assert_eq!(default_name("--"), "rex");
    assert_eq!(default_name(""), "rex");
    let long = default_name(&"a".repeat(80));
    assert_eq!(long.len(), 63);
    assert!(valid_name(&long));
    assert!(valid_name("rex-studio") && valid_name("a"));
    for bad in ["", "-rex", "rex-", "rex studio", "rex.studio", &"a".repeat(64), "rëx"] {
        assert!(!valid_name(bad), "{bad:?}");
    }
}

#[test]
fn magicdns_suffixes_still_match() {
    assert!(name_matches("rex-studio.tail1234.ts.net", "rex-studio"));
    assert!(name_matches("REX-Studio-1.tail1234.ts.net", "rex-studio"));
    assert!(name_matches("rex-studio-12.tail1234.ts.net.", "rex-studio"));
    assert!(!name_matches("rex.tail1234.ts.net", "rex-studio"));
    assert!(!name_matches("rex-studio-x.tail1234.ts.net", "rex-studio"));
    assert!(!name_matches("rex-studio-.tail1234.ts.net", "rex-studio"));
}

#[test]
fn reply_statuses_convert_without_loss() {
    let wire = json!({"state":"running","dns_name":"rex-a.t.ts.net","allow_tailnet_members":false,
        "https":{"state":"available","url":"https://rex-a.t.ts.net"}});
    let server: rexrs::api::server::Tailscale = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(from_server(server)).unwrap(), wire);
}

#[test]
fn the_keeper_enables_once_at_connect_and_never_after_a_listener_failure() {
    let mut k = Keeper::default();
    assert_eq!(k.on_event(&status("disabled", None, None), "rex-a"), None);
    assert_eq!(k.at_connect(&status("disabled", None, None), "rex-a"),
        Some(Call::Enable { hostname: "rex-a".into() }));
    assert_eq!(k.at_connect(&status("disabled", None, None), "rex-a"), None);
    let mut failed = Keeper::default();
    assert_eq!(failed.at_connect(&status("disabled", None, Some("listen: address in use")), "rex-a"), None);
    for state in ["starting", "needs_login", "needs_machine_approval", "mystery"] {
        assert_eq!(Keeper::default().at_connect(&status(state, Some("rex.t.ts.net"), None), "rex-a"), None);
    }
}

#[test]
fn the_keeper_renames_once_and_waits_for_an_event() {
    let mut k = Keeper::default();
    let old = status("running", Some("rex.t.ts.net"), None);
    assert_eq!(k.at_connect(&old, "rex-a"), Some(Call::Rename { hostname: "rex-a".into() }));
    assert_eq!(k.at_connect(&old, "rex-a"), None, "awaiting the event");
    assert_eq!(k.on_event(&old, "rex-a"), None, "one rename per wanted name");
    assert_eq!(k.on_event(&status("running", Some("rex-a-1.t.ts.net"), None), "rex-a"), None);
    assert_eq!(k.on_event(&old, "rex-b"), Some(Call::Rename { hostname: "rex-b".into() }));
    assert_eq!(Keeper::default().at_connect(&status("running", None, None), "rex-a"), None);
}

#[test]
fn events_parse_the_recorded_shape() {
    // RECORDED is the inner payload Step 0 printed, pasted verbatim.
    let recorded: serde_json::Value = serde_json::from_str(RECORDED).unwrap();
    assert!(parse_event(&recorded).is_some());
    assert!(parse_event(&json!({"nope":1})).is_none());
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked --all-features --lib connection::tailscale`
Expected: compile error, module/functions not defined.

- [ ] **Step 3: Implement the module** per the Interfaces and rules above. `name_matches` takes `dns_name.split('.').next()`, compares case-insensitively, and for the suffix form strips `"<wanted>-"` and requires the rest to be non-empty ASCII digits.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --locked --all-features --lib connection::tailscale`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src/connection.rs src/connection/tailscale.rs scripts/tailscale-event-contract.py
git commit -m "Add Tailscale naming rules and keep-on reconciler"
```

---

### Task 2: The host worker tracks status, performs calls, and keeps Tailscale on

**Files:**
- Modify: `src/connection.rs` (`HostModel`, `HostCommand`, `ControlState`, `Startup`, `spawn_labeled`, `apply_bootstrap`, `handle_event`, the connect loop and both disconnected command loops, `handle_command`'s `unreachable!` arm)
- Modify: callers of `spawn_labeled` (`src/app/workspace.rs:381`, `src/connection/perform_tests.rs:22`) for its new parameter
- Create: `src/connection/tailscale_tests.rs`, included from `src/connection/tests.rs` with `#[path = "tailscale_tests.rs"] mod tailscale_tests;` like `cwd_tests`

**Interfaces:**
- Consumes: Task 1's `Status`, `Call`, `Keeper`, `parse_event`.
- Produces:
  - `HostModel::tailscale: Option<tailscale::Status>` — `None` means the server can't run built-in Tailscale (or not yet connected). Set from `boot.server_status.tailscale` in `apply_bootstrap` (replacing the old value, including with `None`), from each successful call's reply, and from each accepted `tailscale_status_changed` event. Left as is on disconnect (the UI checks `connected`; Task 5).
  - `HostModel::tailscale_problem: Option<String>` — `"Couldn't {verb}: {api message}"` after a failed call; cleared by the next successful call and on each new connection.
  - `ControlState::tailscale_seq: u64` — the sequence floor from Global Constraints. Set to `reply.seq` in `apply_bootstrap`; raised to each Tailscale call reply's `seq` and each accepted event's `seq`.
  - `spawn_labeled(…, label: String, keep_tailscale: Option<String>)` — new last parameter, carried in `Startup` into the worker's `keep_tailscale` before the first connect, so the saved opt-in is known at the first bootstrap. `spawn` passes `None`.
  - `HostCommand::KeepTailscale(Option<String>)` — an **explicit** change of the Keep switch or name from Settings (never sent for the saved value at spawn). Updates `keep_tailscale` in all three command loops (like `Theme`). When it arrives while connected and is `Some`, the worker runs `Keeper::at_connect` on a **fresh** `Keeper` against the current status and performs the result — this is the user's own request, so it may enable even after a CLI disable.
  - `HostCommand::Tailscale(tailscale::Call)` — an explicit call. While connected: perform it; if `HostModel::tailscale` is `None`, set `tailscale_problem` to `"This server can't run built-in Tailscale."` and send nothing. While disconnected: falls into the existing "Action not sent while disconnected" arm; the post-failure drain drops it.

Worker behaviour:
- A private `async fn tailscale_call(control: &mut Client, shared: &Shared, state: &mut ControlState, call: &Call) -> Result<()>` uses the typed rexrs methods with `call_with_seq` (`server::TailscaleEnable::new(allow).hostname(name)`, `server::TailscaleDisable::new()`, `server::TailscaleRename::new(name)`) under an 8 s timeout, so the reply's `seq` can raise `tailscale_seq`. For `Enable` it first calls `server::Status::new()` and takes `allow` from that reply's `tailscale` (if the reply has no `tailscale`, set the "can't run" problem and stop). It stores `from_server(reply.value.tailscale)`, and turns `rexrs::Error::Api` and timeouts into `tailscale_problem`; only transport errors are returned (so the loop reconnects as for any other request).
- A `Keeper` lives in the connection scope (new per connection). Startup decision, after `apply_bootstrap` and the queued-event drain: if `keep_tailscale` is `Some(name)`, `model.tailscale` is `Some`, **and `tailscale_seq` still equals the bootstrap `seq`** (no newer status event was applied), call `keeper.at_connect` and perform the result. If a newer event was applied, skip `at_connect` entirely; later events go through `on_event`, which never enables.
- In `handle_event`, handle `"tailscale_status_changed"` **before** `state.includes(&event)`, with its own check: ignore it when `event.seq <= state.tailscale_seq`. Otherwise parse the payload with `parse_event`, store it, raise `tailscale_seq`, then if keeping, `keeper.on_event` and perform. Pass the keeper and `keep_tailscale` into `handle_event` as new parameters.

- [ ] **Step 1: Write the failing worker tests** in `src/connection/tailscale_tests.rs`, using `accept`, `welcome`, `boot`, `recv`, `send`, `reply`, `connected` from `tests.rs`. Each test: bind `127.0.0.1:0`, `HostConnection::spawn_labeled` on `http://{addr}` with `keep_tailscale = Some("rex-a".into())` (unless stated), answer `session.bootstrap` with `boot()` whose `server_status.tailscale` is set as stated, at `seq` 10. Event frames use the existing wire shape: `json!({"type":"event","seq":N,"payload":{"type":"tailscale_status_changed","payload":P}})`, where `P` is built in the shape Task 1 Step 0 recorded. "Expect enable" means: expect `server.status`, reply with the stated status, then expect `server.tailscale.enable`.

```rust
#[tokio::test]
async fn keep_on_enables_a_disabled_device_once_with_the_servers_access() {
    // bootstrap: {"state":"disabled","allow_tailnet_members":false}
    // expect server.status; reply with tailscale {"state":"disabled","allow_tailnet_members":true}
    //   (changed since bootstrap: the fresh value must win)
    // expect "server.tailscale.enable",
    //   payload == json!({"hostname":"rex-a","allow_tailnet_members":true})
    // reply at seq 12 {"tailscale":{"state":"starting","allow_tailnet_members":true}}
    // then: model.tailscale.state == "starting"; send a disabled event at seq 13
    // and assert no further server.tailscale.* request arrives within 300 ms.
}

#[tokio::test]
async fn an_older_buffered_event_never_overwrites_a_newer_status() {
    // before the bootstrap reply, send an event at seq 9 with state "running", dns "old.t.ts.net"
    // bootstrap (seq 10): disabled, keep_tailscale None
    // assert model.tailscale.state == "disabled" after connect
    // send HostCommand::Tailscale(Call::Disable); before replying, send an event at seq 11 "starting";
    //   reply to disable at seq 12 with "disabled"; then an event at seq 12 "running"
    // assert model.tailscale.state == "disabled": the seq-11 event is buffered behind the reply,
    //   which raises the floor to 12, so both events are dropped
}

#[tokio::test]
async fn a_cli_disable_right_after_bootstrap_stops_the_startup_enable() {
    // bootstrap (seq 10): running as "rex-a.t.ts.net"; before the reply is read by the client,
    //   queue an event at seq 11 with state "disabled" (no error)
    // assert no server.tailscale.* within 300 ms; model.tailscale.state == "disabled"
    // then send HostCommand::KeepTailscale(Some("rex-a")) (explicit): expect server.status, then enable
}

#[tokio::test]
async fn a_listener_failure_is_shown_not_retried() {
    // bootstrap: {"state":"disabled","error":"listen tcp :443: in use","allow_tailnet_members":false}
    // assert no server.tailscale.* request within 300 ms;
    // model.tailscale.error == Some("listen tcp :443: in use").
}

#[tokio::test]
async fn one_rename_survives_stale_names() {
    // bootstrap: running as "rex.t.ts.net"
    // expect "server.tailscale.rename" {"hostname":"rex-a"}; reply with dns_name still "rex.t.ts.net"
    // send event running "rex.t.ts.net"; then event running "rex-a-1.t.ts.net"
    // assert no second rename within 300 ms; model dns_name == "rex-a-1.t.ts.net".
}

#[tokio::test]
async fn a_rejected_name_is_a_problem_and_the_connection_stays() {
    // bootstrap: disabled; expect enable; reply with an API error, code "bad_request",
    //   message "invalid hostname"
    // assert model.connected stays true and
    //   model.tailscale_problem == Some("Couldn't turn on Tailscale: invalid hostname".into())
    //   (match the message text the existing notice code produces for rexrs API errors; use `contains("invalid hostname")` if the code is prefixed)
    // then send HostCommand::Tailscale(Call::Disable): expect "server.tailscale.disable" {} ;
    //   reply ok -> problem cleared.
}

#[tokio::test]
async fn without_keep_nothing_is_called() {
    // same as the first test but keep_tailscale None: no server.tailscale.* within 300 ms.
}
```

API error replies use the shape at `src/connection/tests.rs:777`: `json!({"type":"response","id":req["id"],"error":{"code":"bad_request","message":"invalid hostname","status":400}})`. To queue an event "before the reply is read", send the event frame before the bootstrap response frame, as `bootstrap_is_first_and_newer_pre_response_events_survive` does (`tests.rs:177`). The 300 ms "nothing arrives" check: `assert!(tokio::time::timeout(Duration::from_millis(300), recv(&mut ws)).await.is_err())` — answer any non-Tailscale request (`session.view`, keymap) the worker sends meanwhile with the helpers the other tests use, so only `server.tailscale.*` counts.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked --all-features --lib connection::tests::tailscale_tests`
Expected: compile error (`KeepTailscale` / `tailscale` field missing).

- [ ] **Step 3: Implement the model fields, commands, sequence floor, startup parameter, event handling and reconnect wiring** as described above. Clear `tailscale_problem` in the per-connection `shared.update` that clears `activity` etc.

- [ ] **Step 4: Run the new tests and the whole connection suite**

Run: `cargo test --locked --all-features --lib connection::`
Expected: all pass, including the pre-existing bootstrap and cwd tests.

- [ ] **Step 5: Commit**

```bash
git add src/connection.rs src/connection/tests.rs src/connection/tailscale_tests.rs src/connection/perform_tests.rs src/app/workspace.rs
git commit -m "Track built-in Tailscale status and keep it on when asked"
```

---

### Task 3: Saved per-host opt-in and name

**Files:**
- Modify: `src/settings.rs` (`Host`, `Settings::load`, tests)
- Modify: every `Host { … }` literal (`Settings::default`, `src/app/tests.rs::workspace_hosts`, add-host code in `src/app/workspace.rs`) — add the new fields with defaults
- Modify: `src/app/workspace.rs` `edit_host` (clear the new fields when the endpoint changes, where it clears `remote_control`)
- Test: `src/settings.rs` tests; `src/app/remote_tests.rs::remote_control_switch_is_per_host_and_survives_save` gains assertions (or a sibling test in `src/app/tailscale_tests.rs` from Task 4)

**Interfaces:**
- Produces:
  - `Host::keep_tailscale: bool` — `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`
  - `Host::tailscale_name: Option<String>` — `#[serde(default, skip_serializing_if = "Option::is_none")]`
  - `impl Host { pub fn tailscale_name(&self) -> Result<String, String> }` — `Ok` with the saved name when it passes `valid_name`, `Ok(default_name(&gethostname::gethostname().to_string_lossy()))` when none is saved, and `Err(format!("The saved device name “{name}” isn't valid. Edit it to use Tailscale from here."))` for an invalid saved name. Never substitutes the default for an invalid name.
  - `impl Host { pub fn kept_tailscale(&self) -> Option<String> }` — `Some(name)` only when `controls_tailscale() && keep_tailscale` and `tailscale_name()` is `Ok`; this is what every automatic path (spawn, Keep switch) sends.
  - `impl Host { pub fn controls_tailscale(&self) -> bool }` — `self.endpoint.starts_with("unix://")`.
  - `Settings::load` leaves an invalid saved name in place (the user corrects it in Settings).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn tailscale_opt_in_is_off_by_default_and_round_trips() {
    let old: Settings = serde_json::from_str(
        r#"{"hosts":[{"label":"a","endpoint":"unix:///run/rex.sock"}]}"#).unwrap();
    assert!(!old.hosts[0].keep_tailscale);
    let saved = serde_json::to_value(&old).unwrap();
    assert!(saved["hosts"][0].get("keep_tailscale").is_none());
    assert!(saved["hosts"][0].get("tailscale_name").is_none());
    assert!(old.hosts[0].tailscale_name().unwrap().starts_with("rex"));
    assert!(old.hosts[0].controls_tailscale());
    assert_eq!(old.hosts[0].kept_tailscale(), None, "off by default");
    let mut h = old.hosts[0].clone();
    h.keep_tailscale = true;
    h.tailscale_name = Some("rex-a".into());
    assert_eq!(h.kept_tailscale().as_deref(), Some("rex-a"));
    h.endpoint = "ssh://me@box".into();
    assert!(!h.controls_tailscale());
    assert_eq!(h.kept_tailscale(), None);
}

#[test]
fn an_invalid_saved_device_name_is_kept_and_stops_automatic_calls() {
    let dir = std::env::temp_dir().join(format!("rex-ts-{}", std::process::id()));
    let path = dir.join("workspace.json");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, r#"{"hosts":[{"label":"a","endpoint":"unix:///run/rex.sock",
        "keep_tailscale":true,"tailscale_name":"bad name"}]}"#).unwrap();
    let s = Settings::load(&path).unwrap();
    assert!(s.hosts[0].keep_tailscale);
    assert_eq!(s.hosts[0].tailscale_name.as_deref(), Some("bad name"));
    assert!(s.hosts[0].tailscale_name().unwrap_err().contains("bad name"));
    assert_eq!(s.hosts[0].kept_tailscale(), None);
    std::fs::remove_dir_all(dir).unwrap();
}
```

And in the remote-control test (or a new UI test), after the existing endpoint-change assertions:

```rust
rt.state.settings.hosts[1].keep_tailscale = true;
rt.state.settings.hosts[1].tailscale_name = Some("rex-x".into());
let endpoint = rt.state.settings.hosts[1].endpoint.clone();
rt.state.edit_host(1, "same".into(), endpoint, None);
assert!(rt.state.settings.hosts[1].keep_tailscale);
rt.state.edit_host(1, "moved".into(), "unix:///nonexistent/moved.sock".into(), None);
assert!(!rt.state.settings.hosts[1].keep_tailscale);
assert_eq!(rt.state.settings.hosts[1].tailscale_name, None);
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked --all-features tailscale`
Expected: compile errors for the missing fields.

- [ ] **Step 3: Implement** the fields, the three methods, and the `edit_host` reset.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --locked --all-features`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/settings.rs src/app/workspace.rs src/app/tests.rs src/app/remote_tests.rs
git commit -m "Save the Tailscale opt-in and device name per host"
```

---

### Task 4: Tailscale rows in Settings → Hosts

**Files:**
- Create: `src/app/tailscale.rs` (declared in `src/app/mod.rs`) — rows, workspace methods, status words
- Create: `src/app/tailscale_tests.rs` (included like `remote_tests.rs`)
- Modify: `src/app/settings_view.rs` `hosts()` — after each host's Remote control row, `.child(super::tailscale::rows(s, i))`
- Modify: `src/app/panel.rs` — `Form::TailscaleName(usize)`; `src/app/workspace.rs` — open/submit it like `Form::HostShell`
- Modify: `src/app/workspace.rs` `spawn_host` — pass `host.kept_tailscale()` as `spawn_labeled`'s new `keep_tailscale` argument (Task 2 added it with `None` here). No `KeepTailscale` command at spawn: that command means an explicit change.

**Interfaces:**
- Consumes: Task 2's `HostModel::{tailscale, tailscale_problem}`, `HostCommand::{KeepTailscale, Tailscale}`, Task 1's `Call`, `valid_name`; Task 3's `Host` fields and methods.
- Produces:
  - `pub(super) fn rows(s: &Workspace, host: usize) -> El` — empty `col()` unless the host `controls_tailscale()` and its model has `tailscale: Some(_)`; the pairing row (Task 5) is added by this function for any host.
  - `pub(super) fn status_words(status: &Status) -> (String, bool)` — text and whether it is a warning.
  - `impl Workspace { pub fn toggle_tailscale(&mut self, host: usize); pub fn toggle_keep_tailscale(&mut self, host: usize); pub fn set_tailscale_name(&mut self, host: usize, name: String) }`

Rows (use `setting()`, `divider()`, `pill()` from `settings_view.rs`; keys in brackets):
1. **Tailscale** — detail: `status_words`; control: a `rune_kit::Switch` on when `state != "disabled"` [`tailscale:{i}`]. While `needs_login` with `auth_url`, a `Sign in…` pill [`tailscale-sign-in:{i}`] calling `open_link(auth_url)`. If `tailscale_problem` is set, it is shown under the row in `warning()` colour [`tailscale-problem:{i}`].
2. **Device name** — detail: the effective name, or for an invalid saved name the `tailscale_name()` error in `warning()` colour [`tailscale-name-problem:{i}`]; `Edit…` pill [`tailscale-name:{i}`] opening `Form::TailscaleName(i)` (one field, placeholder "1–63 letters, digits and hyphens", prefilled with the saved name, valid or not, else the default). Submit rejects `!valid_name` with the panel error "Use 1–63 letters, digits and hyphens, not starting or ending with a hyphen." and keeps the form open.
3. **Keep on after restarts** — detail: "Turn Tailscale back on under this name whenever Rex Linux connects to this server."; switch [`tailscale-keep:{i}`].

`status_words`: `disabled` + error → (`"Off · {error}"`, true); `disabled` → `"Off"`; `needs_login` → `"Needs sign-in"`; `needs_machine_approval` → (`"Waiting for a tailnet admin to approve this device"`, true); `running` → `"On as {dns_name}"` or `"On"`, plus `" · HTTPS starting…"` when `https` is present and not `available`; anything else → `"Starting…"`.

Actions (each saves settings where it changes them):
- `toggle_tailscale`: on → if `tailscale_name()` is `Err`, do nothing (the name row already shows why); else send `Tailscale(Call::Enable { hostname })`. Off → if Keep is on, set it off, save, send `KeepTailscale(None)`; then send `Tailscale(Call::Disable)`.
- `toggle_keep_tailscale`: flip, save, send `KeepTailscale(host.kept_tailscale())` (the worker enables or renames now if needed; Task 2). With an invalid saved name this sends `None`, so nothing is called until the name is fixed.
- `set_tailscale_name`: save `Some(name)`; if Keep is on send `KeepTailscale(Some(name))`; else if the model's state is `running` send `Tailscale(Call::Rename { hostname: name })`.

- [ ] **Step 1: Write the failing tests** (`capture_host`, `runtime`, `click`, `has` as in `remote_tests.rs`; set `ws.hosts[i].model.tailscale` from JSON):

```rust
#[test]
fn status_words_cover_each_state() {
    // one assert_eq! per bullet in status_words above, including an unknown state -> "Starting…"
}

#[test]
fn only_local_hosts_that_can_run_tailscale_get_the_controls() {
    // host 0 unix:// with tailscale Some(disabled): has "tailscale:0", "tailscale-name:0", "tailscale-keep:0"
    // host 1 unix:// with tailscale None: none of the three keys
    // host 1 endpoint changed to "ssh://me@box" with tailscale Some: none of the three keys
}

#[test]
fn the_switch_enables_under_the_saved_name_and_off_also_stops_keeping() {
    // tailscale_name = Some("rex-a"); click "tailscale:0"
    //   -> rx yields HostCommand::Tailscale(Call::Enable{hostname:"rex-a"})
    // set model state running, keep_tailscale = true; click "tailscale:0"
    //   -> KeepTailscale(None) then Tailscale(Call::Disable), in that order;
    //   Settings::load(path).hosts[0].keep_tailscale == false
}

#[test]
fn keep_and_rename_tell_the_worker() {
    // click "tailscale-keep:0" -> KeepTailscale(Some(default or saved name)); saved true
    // set_tailscale_name(0, "rex-b") with keep on -> KeepTailscale(Some("rex-b")), no Rename
    // keep off, state running: set_tailscale_name(0, "rex-c") -> Tailscale(Rename{"rex-c"})
    // the form rejects "bad name": panel stays open with the error text above, settings unchanged
}

#[test]
fn an_invalid_saved_name_is_shown_and_blocks_calls_until_fixed() {
    // settings host 0: keep_tailscale = true, tailscale_name = Some("bad name"), model disabled
    // has "tailscale-name-problem:0"; click "tailscale:0" -> no command captured
    // click "tailscale-keep:0" twice (off, on) -> KeepTailscale(None) both times
    // set_tailscale_name(0, "rex-ok") with keep on -> KeepTailscale(Some("rex-ok")); problem row gone
}
```

`HostCommand` has no `PartialEq`; match on the variants in the assertions (`matches!(c, HostCommand::Tailscale(Call::Enable { hostname }) if hostname == "rex-a")`).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked --all-features --bin rex-linux tailscale`
Expected: compile errors (module and methods missing).

- [ ] **Step 3: Implement** `src/app/tailscale.rs`, the form, the `hosts()` hook and the `spawn_host` argument (its logic is `Host::kept_tailscale`, tested in Task 3).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --locked --all-features`
Expected: all pass.

- [ ] **Step 5: Check it rendered** — `cargo run --locked --release --bin rex-linux -- --snap target/ts-settings.png` with Settings → Hosts open on a local server (open Settings first per DESIGN.md / the `--snap` docs). Confirm the three rows follow DESIGN.md spacing and the warning colour only appears for problems.

- [ ] **Step 6: Commit**

```bash
git add src/app/
git commit -m "Add Tailscale controls to Settings for local hosts"
```

---

### Task 5: Pairing code

**Files:**
- Modify: `Cargo.toml` — `qrcode = { version = "0.14", default-features = false }`; `Cargo.lock` (via `cargo build`)
- Modify: `src/app/tailscale.rs` — pairing row, dialog, `qr_modules`
- Modify: `src/app/panel.rs` — `PanelKind::Pairing { endpoint: String }` (captures the endpoint, like `StartSsh`); `src/app/dialogs.rs` — dispatch it to `super::tailscale::pairing(s, endpoint)`
- Test: `src/app/tailscale_tests.rs`

**Interfaces:**
- Consumes: `HostModel::tailscale` (Task 2), `rows` (Task 4).
- Produces:
  - `pub(super) fn pairing_url(status: &Status) -> Option<&str>` — `https.url` when `https.state == "available"`.
  - `pub(super) fn qr_modules(url: &str) -> Option<(usize, Vec<bool>)>` — width and row-major dark modules from `qrcode::QrCode::new(url.as_bytes())` (`width()`, `to_colors()`), `None` if encoding fails.
  - `impl Workspace { pub fn show_pairing(&mut self, host: usize) }` — opens `PanelKind::Pairing { endpoint }` titled "Pair a device".
  - `pub(super) fn pairing(s: &Workspace, endpoint: &str) -> El` — looks the host up by endpoint each frame.

Row: for **any** host (remote included) that is `connected` and whose `pairing_url` is `Some`, `rows` adds **Pairing code** — detail "Pair Rex on a phone or tablet on this tailnet."; `Show…` pill [`tailscale-pair:{i}`] calling `show_pairing(i)`.

Dialog: title; the QR as one `canvas(Shape)` of filled squares (one per dark module, see `sprites.rs` for `Shape`/`Path`), drawn on a light square with a 4-module quiet zone so it scans in dark chrome [`pairing-qr`]; the URL as selectable text [`pairing-url`]; the copy from Global Constraints; a `Close` pill. When the host is gone, not `connected`, or `pairing_url` is `None`, replace the QR and URL with "This server isn't reachable over the tailnet right now." [`pairing-unavailable`].

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_qr_code_holds_only_the_url() {
    let (w, dark) = qr_modules("https://rex-studio.tail1234.ts.net").unwrap();
    assert!(w >= 21 && (w - 21) % 4 == 0);
    assert_eq!(dark.len(), w * w);
    // finder pattern corners are dark, the separator next to it is light
    assert!(dark[0] && dark[6] && dark[6 * w] && !dark[7]);
}

#[test]
fn pairing_is_offered_only_while_https_is_available_and_on_remote_hosts_too() {
    // host 1 endpoint "ssh://me@box", tailscale running, https {"state":"pending"}: no "tailscale-pair:1"
    // https {"state":"available","url":"https://rex-box.t.ts.net"}: has "tailscale-pair:1" and no "tailscale:1"
    // click it: has "pairing-qr" and "pairing-url"
    // then set the model's https to None: rebuilt view has "pairing-unavailable" and not "pairing-qr"
    // restore https, then set model.connected = false: "pairing-unavailable" again, and the
    //   Settings row "tailscale-pair:1" is gone
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked --all-features --bin rex-linux pairing`
Expected: compile errors.

- [ ] **Step 3: Add the dependency and implement.** Run `cargo build --locked` first without `--locked` once (`cargo build`) to update `Cargo.lock`, then confirm `cargo build --locked` succeeds. `nix/package.nix` needs no hash change (crates.io dependencies come from `Cargo.lock`).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --locked --all-features`
Expected: all pass.

- [ ] **Step 5: Scan a real code** — against a local server with Tailscale on and HTTPS available, open the dialog, `--snap` it, and confirm the QR decodes to the URL (`zbarimg` on the PNG if installed, or a phone).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/app/
git commit -m "Show a pairing QR code while tailnet HTTPS is available"
```

---

### Task 6: Document the exception

**Files:**
- Modify: `PRODUCT.md:13`, `README.md:38` (and the README's Settings section, if it lists host settings), `AGENTS.md:51`

- [ ] **Step 1: Edit the three attachment-only statements** to name the one exception: when the user turns on **Keep on after restarts** for a local host, connecting may turn that server's built-in Tailscale on (unless it turned off after an HTTPS failure) and rename its device to the saved name. It reads the server's current tailnet access setting just before turning Tailscale on and passes it back unchanged; a change made by someone else in that moment could be overwritten (the API can't keep the current value on its own). In README, add a short "Tailscale" paragraph describing the Settings rows, that the controls are local-only, that race, and the pairing code — only what Tasks 4–5 implemented.

- [ ] **Step 2: Full verification**

Run each; expected all succeed with no warnings or diffs:
```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo build --locked --release --bin rex-linux
cargo run --locked --no-default-features --bin rex-probe -- --help
```

- [ ] **Step 3: Commit**

```bash
git add PRODUCT.md README.md AGENTS.md
git commit -m "Document the opt-in Tailscale exception to attachment-only"
```
