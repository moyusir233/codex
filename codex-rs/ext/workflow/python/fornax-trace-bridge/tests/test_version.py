from importlib.metadata import packages_distributions, version

from codex_fornax_trace_bridge.config import (
    PINNED_SDK_VERSION,
    SDK_DISTRIBUTION,
    resolve_sdk_version,
)
from codex_fornax_trace_bridge.lifecycle import _configuration_fingerprint


def test_release_manifest_matches_import_ownership_and_lock() -> None:
    assert SDK_DISTRIBUTION in packages_distributions()["bytedance"]
    assert version(SDK_DISTRIBUTION) == PINNED_SDK_VERSION
    assert resolve_sdk_version() == PINNED_SDK_VERSION


def test_configuration_fingerprint_is_keyed_and_never_contains_credentials(
    monkeypatch,
) -> None:
    monkeypatch.setenv("FORNAX_AK", "AK-CANARY")
    monkeypatch.setenv("FORNAX_SK", "SK-CANARY")
    monkeypatch.setenv("FORNAX_CUSTOM_REGION", "region")
    fingerprint = _configuration_fingerprint(b"s" * 32, PINNED_SDK_VERSION)
    assert fingerprint is not None
    assert len(fingerprint) == 64
    assert "CANARY" not in fingerprint
