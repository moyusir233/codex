"""`codex-fornax-trace` lifecycle CLI."""

from __future__ import annotations

import argparse
import json
import os
from collections.abc import Sequence
from pathlib import Path

from . import PROTOCOL_VERSION, __version__
from .errors import BridgeError
from .lifecycle import ensure, serve, status, stop


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(prog="codex-fornax-trace")
    subcommands = result.add_subparsers(dest="command", required=True)
    version = subcommands.add_parser("version")
    version.add_argument("--format", choices=("json",), default="json")
    daemon = subcommands.add_parser("daemon")
    daemon_commands = daemon.add_subparsers(dest="daemon_command", required=True)
    for name in ("ensure", "status", "stop", "serve"):
        command = daemon_commands.add_parser(name)
        command.add_argument("--state-dir", type=Path, default=_default_state_dir())
        command.add_argument("--format", choices=("json",), default="json")
        if name == "stop":
            command.add_argument("--grace-seconds", type=float, default=30.0)
            command.add_argument("--force-after-seconds", type=float)
    return result


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "version":
            output = {"bridgeVersion": __version__, "protocolVersion": int(PROTOCOL_VERSION)}
        elif args.daemon_command == "ensure":
            output = ensure(args.state_dir)
        elif args.daemon_command == "status":
            output = status(args.state_dir, probe=True)
        elif args.daemon_command == "stop":
            deadline = args.force_after_seconds or args.grace_seconds
            if deadline <= 0:
                raise BridgeError("invalid_request", "grace seconds must be positive", 400)
            output = stop(
                args.state_dir,
                grace_seconds=deadline,
                force=args.force_after_seconds is not None,
            )
        elif args.daemon_command == "serve":
            serve(args.state_dir)
            return 0
        else:  # pragma: no cover - argparse makes this unreachable.
            raise AssertionError("unreachable command")
    except BridgeError as error:
        print(json.dumps(error.envelope(), sort_keys=True, separators=(",", ":")))
        return _exit_code(error)
    print(json.dumps(output, sort_keys=True, separators=(",", ":")))
    return 0


def _default_state_dir() -> Path:
    codex_home = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex"))
    return (codex_home / "workflow" / "fornax-trace-daemon").resolve()


def _exit_code(error: BridgeError) -> int:
    if error.code in {"invalidRequest", "invalid_request", "invalidStateDir"}:
        return 2
    if error.code in {"sdkAuthentication"}:
        return 3
    if error.code in {"unsupportedVersion", "configurationMismatch"}:
        return 4
    if error.code in {"shutdownTimeout", "startupTimeout"}:
        return 6
    return 5
