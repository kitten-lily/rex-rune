"""Refuse binaries that carry the local Dracula Pro themes (private, not redistributable)."""

from fnmatch import fnmatch
from pathlib import Path
import tarfile

MARKER = b"Includes local Dracula Pro themes"


def refuse_local_themes(data: bytes, what: str) -> None:
    if MARKER in data:
        raise SystemExit(
            f"{what} contains local Dracula Pro themes and must not be packaged or published. "
            "Rebuild with: REX_DRACULA_PRO=0 cargo build --locked --release --bin rex-linux"
        )


def refuse_in_tarball(path: Path) -> None:
    with tarfile.open(path) as archive:
        for member in archive:
            if member.isfile() and fnmatch(member.name, "*/bin/rex-linux"):
                data = archive.extractfile(member).read()
                refuse_local_themes(data, f"{path}:{member.name}")
                return
    raise SystemExit(f"{path} has no */bin/rex-linux member to check for local Dracula Pro themes")
