"""Release manifest and non-secret bridge limits."""

from __future__ import annotations

from importlib.metadata import packages_distributions, version

from .errors import BridgeError

SDK_IMPORT_ROOT = "bytedance"
SDK_DISTRIBUTION = "bytedance.fornax"
PINNED_SDK_VERSION = "1.0.46"


def resolve_sdk_version() -> str:
    owners = packages_distributions().get(SDK_IMPORT_ROOT, [])
    if SDK_DISTRIBUTION not in owners:
        raise BridgeError(
            "unsupportedVersion",
            "installed Fornax import is not owned by the pinned distribution",
            503,
        )
    installed = version(SDK_DISTRIBUTION)
    if installed != PINNED_SDK_VERSION:
        raise BridgeError(
            "unsupportedVersion",
            "installed Fornax SDK version is incompatible with this bridge",
            503,
        )
    return installed
