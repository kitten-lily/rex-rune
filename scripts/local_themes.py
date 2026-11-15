"""Refuse binaries that carry the local Dracula Pro themes (private, not redistributable)."""

from fnmatch import fnmatch
from pathlib import Path
import tarfile

MARKER = b"Includes local Dracula Pro themes"


def refuse_local_themes(data: bytes, what: str) -> None:
    if MARKER in data:
        raise SystemExit(
            f"{what} contains local Dracula Pro themes and must not be packaged or published. "
            "Rebuild with: REX_DRACULA_PRO=0 cargo build --locked --release --bin rex-rune"
        )


def refuse_in_tarball(path: Path) -> None:
    """Check every client binary in a release tarball: bin/rex-rune, and bin/rex-linux in
    the bridge tarball the 0.7.x updater installs."""
    with tarfile.open(path) as archive:
        members = [
            m for m in archive
            if m.isfile() and (fnmatch(m.name, "*/bin/rex-rune") or fnmatch(m.name, "*/bin/rex-linux"))
        ]
        if not members:
            raise SystemExit(f"{path} has no */bin/rex-rune member to check for local Dracula Pro themes")
        for member in members:
            refuse_local_themes(archive.extractfile(member).read(), f"{path}:{member.name}")
