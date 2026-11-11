#!/usr/bin/env python3
"""Tests scripts/fetch-dracula-pro.sh against local bare repos in temp dirs.

Never touches the network or the real checkout location: every run sets
REX_DRACULA_PRO_URL to a local path and REX_DRACULA_PRO_DIR to a temp path.
"""
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "fetch-dracula-pro.sh"
GIT = ["git", "-c", "commit.gpgsign=false", "-c", "user.name=test", "-c", "user.email=test@example.invalid"]


def git(*args, cwd):
    subprocess.run(GIT + list(args), cwd=cwd, check=True, capture_output=True)


class FetchTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.bare = self.tmp / "remote.git"
        work = self.tmp / "work"
        subprocess.run(["git", "init", "-q", "--bare", "-b", "main", str(self.bare)], check=True)
        subprocess.run(["git", "init", "-q", "-b", "main", str(work)], check=True)
        (work / "pro").write_text("made-up one\n")
        git("add", "pro", cwd=work)
        git("commit", "-q", "-m", "one", cwd=work)
        git("push", "-q", str(self.bare), "main", cwd=work)
        self.work = work
        self.dest = self.tmp / "data" / "checkout"

    def run_script(self, *args, url=None, dest=None, cwd=None, **env):
        e = {k: v for k, v in os.environ.items() if not k.startswith("REX_DRACULA_PRO")}
        e["REX_DRACULA_PRO_URL"] = url if url is not None else str(self.bare)
        e["REX_DRACULA_PRO_DIR"] = str(dest or self.dest)
        e.update(env)
        return subprocess.run(
            ["bash", str(SCRIPT), *args], env=e, cwd=cwd or self.tmp, capture_output=True, text=True
        )

    def head(self, dest=None):
        return subprocess.run(
            ["git", "-C", str(dest or self.dest), "rev-parse", "HEAD"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()

    def test_first_fetch_clones(self):
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.dest / "pro").read_text(), "made-up one\n")
        self.assertNotIn("warning", r.stderr)
        self.assertEqual([p.name for p in self.dest.parent.iterdir()], ["checkout"])

    def test_second_fetch_pulls_new_commit(self):
        self.run_script()
        (self.work / "blade").write_text("made-up two\n")
        git("add", "blade", cwd=self.work)
        git("commit", "-q", "-m", "two", cwd=self.work)
        git("push", "-q", str(self.bare), "main", cwd=self.work)
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.dest / "blade").read_text(), "made-up two\n")

    def test_unreachable_remote_warns_and_keeps_checkout(self):
        self.run_script()
        before = self.head()
        moved = self.tmp / "moved.git"
        self.bare.rename(moved)
        r = self.run_script()
        self.assertEqual(r.returncode, 0)
        self.assertIn("warning:", r.stderr)
        self.assertIn("the existing checkout", r.stderr)
        self.assertEqual(self.head(), before)
        self.assertEqual((self.dest / "pro").read_text(), "made-up one\n")

    def test_failed_first_clone_leaves_nothing(self):
        r = self.run_script(url="file:///nonexistent")
        self.assertEqual(r.returncode, 0)
        self.assertIn("warning:", r.stderr)
        self.assertIn("no Dracula Pro themes", r.stderr)
        self.assertFalse(self.dest.exists())
        self.assertEqual(list(self.dest.parent.glob("*.tmp.*")), [])

    def test_rev_prints_the_checkout_head_without_fetching(self):
        self.run_script()
        head = self.head()
        (self.work / "blade").write_text("made-up two\n")
        git("add", "blade", cwd=self.work)
        git("commit", "-q", "-m", "two", cwd=self.work)
        git("push", "-q", str(self.bare), "main", cwd=self.work)
        r = self.run_script("--rev")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout.strip(), head)
        self.assertEqual(self.head(), head)

    def test_rev_prints_nothing_without_a_checkout_or_when_disabled(self):
        r = self.run_script("--rev")
        self.assertEqual((r.returncode, r.stdout, r.stderr), (0, "", ""))
        self.assertFalse(self.dest.parent.exists())
        self.run_script()
        r = self.run_script("--rev", REX_DRACULA_PRO="0")
        self.assertEqual((r.returncode, r.stdout, r.stderr), (0, "", ""))

    def test_a_trailing_slash_still_clones(self):
        dest = f"{self.dest}//"
        r = self.run_script(dest=dest)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertNotIn("warning", r.stderr)
        self.assertEqual((self.dest / "pro").read_text(), "made-up one\n")
        self.assertEqual([p.name for p in self.dest.parent.iterdir()], ["checkout"])

    def test_disabled_does_nothing(self):
        r = self.run_script(REX_DRACULA_PRO="0")
        self.assertEqual(r.returncode, 0)
        self.assertEqual(r.stdout + r.stderr, "")
        self.assertFalse(self.dest.parent.exists())

    def test_refuses_a_directory_inside_the_repo(self):
        repo = self.tmp / "repo"
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        dest = repo / "themes" / "checkout"
        r = self.run_script(dest=dest, cwd=repo)
        self.assertEqual(r.returncode, 0)
        self.assertIn("warning:", r.stderr)
        self.assertFalse(dest.exists())


if __name__ == "__main__":
    unittest.main()
