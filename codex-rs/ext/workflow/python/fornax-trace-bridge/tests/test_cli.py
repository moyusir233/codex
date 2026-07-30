from __future__ import annotations

import json
from pathlib import Path

from codex_fornax_trace_bridge import PROTOCOL_VERSION, __version__
from codex_fornax_trace_bridge.cli import main


def test_version_emits_one_json_object(capsys) -> None:
    assert main(["version"]) == 0
    lines = capsys.readouterr().out.splitlines()
    assert len(lines) == 1
    assert json.loads(lines[0]) == {
        "protocolVersion": int(PROTOCOL_VERSION),
        "bridgeVersion": __version__,
    }


def test_status_accepts_documented_format_position(tmp_path: Path, capsys) -> None:
    state_dir = (tmp_path / "state").resolve()
    assert main(["daemon", "status", "--state-dir", str(state_dir), "--format", "json"]) == 0
    assert json.loads(capsys.readouterr().out)["status"] == "stopped"


def test_stop_rejects_nonpositive_grace(tmp_path: Path, capsys) -> None:
    state_dir = (tmp_path / "state").resolve()
    assert (
        main(
            [
                "daemon",
                "stop",
                "--state-dir",
                str(state_dir),
                "--grace-seconds",
                "0",
            ]
        )
        == 2
    )
    assert json.loads(capsys.readouterr().out)["error"]["code"] == "invalid_request"
