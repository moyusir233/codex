"""Advisory file locks for daemon lifecycle serialization."""

from __future__ import annotations

import fcntl
import os
from pathlib import Path
from typing import IO

from .errors import BridgeError


class FileLock:
    """Holds one mode-0600 advisory lock file."""

    def __init__(self, path: Path, *, blocking: bool) -> None:
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        self._file: IO[str] = path.open("a+", encoding="utf-8")
        os.chmod(path, 0o600)
        flags = fcntl.LOCK_EX
        if not blocking:
            flags |= fcntl.LOCK_NB
        try:
            fcntl.flock(self._file.fileno(), flags)
        except BlockingIOError as error:
            self._file.close()
            raise BridgeError(
                "already_running", "bridge instance lock is already held", 409
            ) from error

    def close(self) -> None:
        if not self._file.closed:
            fcntl.flock(self._file.fileno(), fcntl.LOCK_UN)
            self._file.close()

    def __enter__(self) -> FileLock:  # noqa: PYI034 - Python 3.10 lacks typing.Self.
        return self

    def __exit__(self, *_args: object) -> None:
        self.close()
