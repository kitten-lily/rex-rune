# Hide an offline host until it reconnects — design

## Intent

A saved host that is offline (a laptop that is asleep, a server that is down) takes up a
header, a note and a "Retry connection" row in the sidebar's host list for as long as it
stays offline. The user wants to clear it away and have it come back by itself when it
connects again.

## Goals

- An offline host can be hidden from the sidebar's grouped host list in one step.
- A hidden host returns the moment it connects, with nothing for the user to undo.
- A hidden host stays reachable: "Find a session" (the switcher) and the toolbar's identity
  menu keep listing it with their existing "Retry connection" row, and Settings keeps it.

## Non-goals

- No permanent "never show this host" setting.
- No change to the Flat session list, which already leaves offline hosts out.
- No change to how or when the client retries a connection.
- No new rows in the switcher or identity menu; they already offer Retry.

## Anti-goals

- Hiding must never stop, slow or restart connection attempts, and must never send anything
  to a server.
- A connected host must never be hidden, whatever order events arrive in.

## Approaches

1. **A saved per-host mark, cleared on connect (chosen).** `hidden_offline` on the saved host;
   the sidebar skips marked hosts; the host's next successful connection clears it. Survives
   restarts, so a machine that is off for a week stays out of the way.
2. **An in-memory mark.** Simpler, but every restart brings every offline host back, which is
   when the list is noisiest (nothing has connected yet).
3. **A global "hide offline hosts" setting.** That is the Flat list's behaviour already, and it
   takes away the per-host choice the request asks for.

## Design

- **Hiding.** An offline host's header shows a small × in the slot where a connected host has
  its + (that slot is empty today). The header's right-click menu gains "Hide until it
  reconnects" under "Retry connection". Neither exists for a connected host.
- **Effect.** The host's whole group (header, cached session rows, notes, Retry row) leaves
  the grouped sidebar list. Nothing else changes: switcher, identity menu, Settings → Hosts,
  the toolbar's naming of the active host, and the connection worker.
- **Coming back.** When a host's model is seen connected, its mark is cleared and saved. If
  it drops again later it is shown offline as before, not hidden again.
- **Active host.** May be hidden; the toolbar still names it.
- **Every host hidden.** The list says how many are hidden instead of being empty.
- **Edit host.** Changing a hidden host's endpoint clears the mark: it is a different server.
- **Prefetch.** A hidden host is not "listed", so the sidebar-tabs prefetch skips it.

## Left to the builder

Exact placement of the mark-clearing inside `Workspace::sync`, and test fixture details.
