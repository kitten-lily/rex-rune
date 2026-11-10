#!/usr/bin/env python3
"""Record the shape of a built-in Tailscale `tailscale_status_changed` event.

Starts a private Rex server (scratch socket and state dir under ~/.cache/agent-ws,
isolated HOME/XDG dirs), follows every server event with `rex events --all --json`
(the same event stream `session.bootstrap` with event_scope "all" delivers), calls
`server.tailscale.enable` without tailnet members, prints the first
`tailscale_status_changed` frame verbatim, then disables Tailscale and stops the
server. The device only reaches needs_login, so nothing joins a tailnet. Never
touches your own server or Tailscale identity. Requires rex.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time

REX = shutil.which("rex")
assert REX, "rex required"
base = Path.home() / ".cache/agent-ws"
base.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix="rex-tailscale-contract-", dir=base))
for name in ("home", "config", "state", "runtime", "cache"):
    (work / name).mkdir(mode=0o700)
for name in list(os.environ):
    if name.startswith(("REX_", "HERDR_", "TS_")) or name in ("DBUS_SESSION_BUS_ADDRESS", "DISPLAY", "WAYLAND_DISPLAY"):
        del os.environ[name]
SERVER = "unix://" + str(work / "rex.sock")
os.environ.update(HOME=str(work / "home"), XDG_CONFIG_HOME=str(work / "config"),
                  XDG_STATE_HOME=str(work / "state"), XDG_RUNTIME_DIR=str(work / "runtime"),
                  XDG_CACHE_HOME=str(work / "cache"), REX_SERVER=SERVER)
REX_ARGS = [REX, "--autostart=false", "--server", SERVER]


def api(method, **payload):
    out = subprocess.run(REX_ARGS + ["api", "call", method, json.dumps(payload)],
                         capture_output=True, text=True, timeout=30)
    return out.returncode, out.stdout + out.stderr


def stop(process):
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


server = subprocess.Popen([REX, "server", "run", "--listen", SERVER, "--restore=false",
                           "--config", "/dev/null", "--state-dir", str(work / "server-state")],
                          stdout=(work / "server.log").open("w"), stderr=subprocess.STDOUT)
events = None
try:
    print("Private artifacts:", work, flush=True)
    deadline = time.monotonic() + 10
    while not (work / "rex.sock").exists():
        assert time.monotonic() < deadline, "private server did not start"
        time.sleep(.1)
    events = subprocess.Popen(REX_ARGS + ["events", "--all", "--json"], stdout=subprocess.PIPE, text=True)
    frames = []
    threading.Thread(target=lambda: [frames.append(line) for line in events.stdout], daemon=True).start()
    time.sleep(1)
    code, text = api("server.tailscale.enable", allow_tailnet_members=False)
    print("enable:", code, text.strip(), flush=True)
    assert code == 0, "server.tailscale.enable failed (no built-in Tailscale, or not_supported)"
    deadline = time.monotonic() + 20
    found = None
    while time.monotonic() < deadline and not found:
        found = next((f for f in list(frames) if "tailscale_status_changed" in f), None)
        time.sleep(.2)
    assert found, "no tailscale_status_changed event within 20 s; saw: " + repr(frames)
    print("EVENT FRAME:", found.strip(), flush=True)
    print("disable:", *api("server.tailscale.disable"), flush=True)
finally:
    if events:
        stop(events)
    stop(server)
