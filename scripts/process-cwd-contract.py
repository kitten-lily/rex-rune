#!/usr/bin/env python3
"""Check the server behaviour that new terminals' working directories rely on.

This is a server-contract check, not application acceptance: it calls the Rex API the way
rex-linux's host worker does, without the app. In a shell that never sends OSC 7 it checks
that `terminal.process` reports the shell's directory after a `cd`, that a split given that
directory as `cwd` starts there, and that a split given a missing directory still opens, in
the home directory.

Requires rex. Starts its own server with a throwaway HOME; no daily sessions are used.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

REX = shutil.which("rex")
assert REX, "rex required"
work = Path(tempfile.mkdtemp(prefix="rex-cwd-contract-"))
for name in ("home", "config", "state", "runtime", "cache"):
    (work / name).mkdir(mode=0o700)
for name in list(os.environ):
    if name.startswith(("REX_", "HERDR_")):
        del os.environ[name]
os.environ.update(HOME=str(work / "home"), XDG_CONFIG_HOME=str(work / "config"),
                  XDG_STATE_HOME=str(work / "state"), XDG_RUNTIME_DIR=str(work / "runtime"),
                  XDG_CACHE_HOME=str(work / "cache"))
SERVER = "unix://" + str(work / "rex.sock")
REX_ARGS = [REX, "--autostart=false", "--server", SERVER]
TERMINAL = "com.superlogical.terminal"
# /bin/sh with no rc files sends no OSC 7.
SHELL = {"flavor": TERMINAL + ".shell", "options": {"shell": "/bin/sh"}}


def api(method, **payload):
    return json.loads(subprocess.check_output(REX_ARGS + ["api", "call", method, json.dumps(payload)],
                                              text=True, timeout=12))


def wait(predicate, description, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if (value := predicate()):
            return value
        time.sleep(.1)
    raise AssertionError(description)


def shell_cwd(session, block):
    reply = api(TERMINAL + ".process", session_id=session, block_id=block, args={})
    return (reply.get("child") or {}).get("cwd")


def split(session, anchor, cwd):
    layout = {"block": {**SHELL, "options": {**SHELL["options"], "cwd": cwd}}}
    reply = api("session.new_split", session_id=session, anchor_id=anchor, direction="horizontal",
                side="after", layout=layout, focus=False)
    return reply["block_ids"][0]


server = subprocess.Popen([REX, "server", "run", "--listen", SERVER, "--restore=false",
                           "--config", "/dev/null", "--state-dir", str(work / "server")],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    wait(lambda: (work / "rex.sock").exists(), "private server did not start")
    created = api("session.create", label="cwd", initial_windows=[{"layout": {"block": SHELL}}])
    session = created["session_id"]
    first = created["initial_windows"][0]["block_ids"][0]
    home = str((work / "home").resolve())
    wait(lambda: shell_cwd(session, first) == home, "the first shell starts at home")

    subprocess.check_call(REX_ARGS + ["send", "-s", session, "-b", first, "cd /usr/share\n"])
    wait(lambda: shell_cwd(session, first) == "/usr/share",
         "process reports the shell's directory after cd, without OSC 7")
    assert "pwd" not in api(TERMINAL + ".pwd", session_id=session, block_id=first, args={}), \
        "no OSC 7 was sent, so pwd has nothing"

    inherited = split(session, first, "/usr/share")
    wait(lambda: shell_cwd(session, inherited) == "/usr/share", "a split starts in its cwd")
    missing = split(session, first, str(work / "gone"))
    wait(lambda: shell_cwd(session, missing) == home, "a missing cwd starts at home")
    print("ok")
finally:
    server.terminate()
    server.wait(timeout=8)
    shutil.rmtree(work, ignore_errors=True)
