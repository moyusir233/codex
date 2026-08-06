"""Singleton daemon discovery, publication, and shutdown."""

import hashlib
import hmac
import json
import os
import secrets
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import urlparse
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener

from . import PROTOCOL_VERSION, __version__
from .config import PINNED_SDK_VERSION
from .errors import BridgeError, unavailable
from .http_api import BridgeHttpServer
from .journal import Journal
from .locking import FileLock
from .sdk import FornaxSdkSession
from .service import BridgeService


@dataclass(frozen=True, slots=True)
class Descriptor:
    instance_id: str
    pid: int
    process_start: str
    endpoint: str
    credential_file: str
    protocol_version: int
    bridge_version: str
    sdk_version: str


def ensure(state_dir: Path, *, deadline: float = 15.0) -> dict[str, Any]:
    """Return the healthy singleton, starting one when absent."""
    _secure_state_dir(state_dir)
    with FileLock(state_dir / "lifecycle.lock", blocking=True):
        current = status(state_dir, probe=True)
        if current.get("status") == "ready":
            descriptor = _read_descriptor(state_dir)
            if descriptor is None:
                raise BridgeError("invalidDescriptor", "bridge descriptor disappeared", 500)
            _verify_supplied_configuration(
                _descriptor_token_path(state_dir, descriptor),
                descriptor.sdk_version,
            )
            return {**current, "status": "alreadyRunning"}
        descriptor = _read_descriptor(state_dir)
        if descriptor is not None and _same_process(descriptor.pid, descriptor.process_start):
            raise BridgeError(
                "configurationMismatch",
                "a live bridge instance is published but failed compatibility checks",
                409,
            )
        try:
            instance_probe = FileLock(state_dir / "instance.lock", blocking=False)
        except BridgeError as error:
            raise BridgeError(
                "invalidDescriptor",
                "bridge instance is live but its descriptor is unusable",
                500,
            ) from error
        instance_probe.close()
        _quarantine_stale(state_dir)
        log_path = state_dir / "stderr.log"
        _rotate_log(log_path, 1024 * 1024)
        log_file = log_path.open("ab", buffering=0)
        os.chmod(log_path, 0o600)
        command = [
            sys.executable,
            "-m",
            "codex_fornax_trace_bridge",
            "daemon",
            "serve",
            "--state-dir",
            str(state_dir),
        ]
        subprocess.Popen(
            command,
            stdin=subprocess.DEVNULL,
            stdout=log_file,
            stderr=log_file,
            close_fds=True,
            start_new_session=True,
        )
        log_file.close()
        expires = time.monotonic() + deadline
        last_error = "descriptor not published"
        while time.monotonic() < expires:
            time.sleep(0.05)
            current = status(state_dir, probe=True)
            if current.get("status") == "ready":
                return {**current, "status": "started"}
            last_error = str(current.get("message", current.get("status")))
        raise unavailable(f"bridge did not become ready: {last_error}")


def status(state_dir: Path, *, probe: bool) -> dict[str, Any]:
    """Read and validate published singleton state."""
    descriptor = _read_descriptor(state_dir)
    if descriptor is None:
        return {
            "status": "stopped",
            "bridgeVersion": __version__,
            "protocolVersion": int(PROTOCOL_VERSION),
            "sdkVersion": PINNED_SDK_VERSION,
        }
    if not _same_process(descriptor.pid, descriptor.process_start):
        return {"status": "stale", "message": "published process identity is no longer live"}
    try:
        token = _read_token(_descriptor_token_path(state_dir, descriptor))
        _validate_base_url(descriptor.endpoint)
        if (
            descriptor.protocol_version != int(PROTOCOL_VERSION)
            or descriptor.bridge_version != __version__
            or descriptor.sdk_version != PINNED_SDK_VERSION
        ):
            raise BridgeError("unsupportedVersion", "published bridge version is incompatible", 503)
        if probe:
            health = request_json(descriptor, token, "GET", "/v1/health", None, timeout=1.0)
            if health.get("instanceId") != descriptor.instance_id:
                raise BridgeError("descriptor_mismatch", "health instance does not match", 409)
        return {
            "status": "ready",
            "instanceId": descriptor.instance_id,
            "pid": descriptor.pid,
            "endpoint": descriptor.endpoint,
            "credentialFile": descriptor.credential_file,
            "protocolVersion": descriptor.protocol_version,
            "bridgeVersion": descriptor.bridge_version,
            "sdkVersion": descriptor.sdk_version,
        }
    except (BridgeError, OSError, URLError) as error:
        return {"status": "unhealthy", "message": str(error)}


def stop(
    state_dir: Path,
    *,
    grace_seconds: float = 30.0,
    force: bool = False,
) -> dict[str, Any]:
    """Gracefully stop the exact published process, with PID-reuse defense."""
    with FileLock(state_dir / "lifecycle.lock", blocking=True):
        descriptor = _read_descriptor(state_dir)
        if descriptor is None:
            return {"status": "stopped"}
        if not _same_process(descriptor.pid, descriptor.process_start):
            _quarantine_stale(state_dir)
            return {"status": "stale_removed"}
        token = _read_token(_descriptor_token_path(state_dir, descriptor))
        try:
            request_json(
                descriptor,
                token,
                "POST",
                "/v1/shutdown",
                {"mode": "rejectIfActive"},
                timeout=2.0,
            )
        except (BridgeError, OSError, URLError):
            pass
        expires = time.monotonic() + grace_seconds
        while time.monotonic() < expires:
            if not _same_process(descriptor.pid, descriptor.process_start):
                _quarantine_stale(state_dir)
                return {"status": "stopped"}
            time.sleep(0.05)
        if force and _same_process(descriptor.pid, descriptor.process_start):
            os.kill(descriptor.pid, signal.SIGTERM)
            return {"status": "terminationRequested"}
        raise BridgeError("shutdownTimeout", "bridge did not stop before its deadline", 504)


def serve(state_dir: Path, sdk: FornaxSdkSession | None = None) -> None:
    """Own one descriptor, token, journal, HTTP server, and SDK client."""
    _secure_state_dir(state_dir)
    instance_lock = FileLock(state_dir / "instance.lock", blocking=False)
    instance_id = f"fbi_{uuid.uuid4().hex}"
    token_path = state_dir / "credential.json"
    token = secrets.token_urlsafe(48)
    try:
        sdk = sdk or FornaxSdkSession.from_environment()
    except BaseException:
        instance_lock.close()
        raise
    fingerprint_salt = secrets.token_bytes(32)
    config_fingerprint = _configuration_fingerprint(
        fingerprint_salt,
        sdk.sdk_version,
    )
    _atomic_write(
        token_path,
        json.dumps(
            {
                "token": token,
                "fingerprintSalt": fingerprint_salt.hex(),
                "configFingerprint": config_fingerprint,
            },
            sort_keys=True,
            separators=(",", ":"),
        ).encode(),
        0o600,
    )
    journal = Journal(state_dir / "journal.sqlite3", instance_id, sdk.sdk_version)
    service = BridgeService(journal, sdk, instance_id)
    server = BridgeHttpServer(("127.0.0.1", 0), service, token)
    descriptor = Descriptor(
        instance_id=instance_id,
        pid=os.getpid(),
        process_start=_process_start(os.getpid()),
        endpoint=f"http://127.0.0.1:{server.server_address[1]}",
        credential_file=str(token_path),
        protocol_version=int(PROTOCOL_VERSION),
        bridge_version=__version__,
        sdk_version=sdk.sdk_version,
    )
    _atomic_write(
        state_dir / "daemon.json",
        json.dumps(_descriptor_json(descriptor), sort_keys=True, separators=(",", ":")).encode(),
        0o600,
    )

    forced_shutdown = threading.Event()

    def request_shutdown(signum: int, _frame: object) -> None:
        def shutdown_when_idle() -> None:
            if signum == signal.SIGTERM:
                forced_shutdown.set()
                service.prepare_forced_shutdown()
                server.shutdown()
                return
            try:
                service.prepare_shutdown()
            except BridgeError:
                return
            server.shutdown()

        threading.Thread(target=shutdown_when_idle, daemon=True).start()

    previous_sigterm = signal.signal(signal.SIGTERM, request_shutdown)
    previous_sigint = signal.signal(signal.SIGINT, request_shutdown)
    try:
        server.serve_forever(poll_interval=0.1)
        service.close(force=forced_shutdown.is_set())
    finally:
        signal.signal(signal.SIGTERM, previous_sigterm)
        signal.signal(signal.SIGINT, previous_sigint)
        server.server_close()
        current = _read_descriptor(state_dir)
        if current and current.instance_id == instance_id:
            (state_dir / "daemon.json").unlink(missing_ok=True)
        token_path.unlink(missing_ok=True)
        instance_lock.close()


def request_json(
    descriptor: Descriptor,
    token: str,
    method: str,
    path: str,
    body: dict[str, Any] | None,
    *,
    timeout: float,
) -> dict[str, Any]:
    """Call the authenticated descriptor without proxy or redirects."""
    _validate_base_url(descriptor.endpoint)
    encoded = None if body is None else json.dumps(body, separators=(",", ":")).encode()
    request = Request(
        descriptor.endpoint + path,
        data=encoded,
        method=method,
        headers={
            "Authorization": f"Bearer {token}",
            "X-Codex-Fornax-Protocol": PROTOCOL_VERSION,
            "Content-Type": "application/json",
            **(
                {"Idempotency-Key": str(body["operationId"])}
                if body is not None and "operationId" in body
                else {}
            ),
        },
    )
    opener = build_opener(ProxyHandler({}), _NoRedirect())
    try:
        with opener.open(request, timeout=timeout) as response:
            value = json.load(response)
    except HTTPError as error:
        try:
            envelope = json.load(error)
            message = envelope["error"]["message"]
            code = envelope["error"]["code"]
        except Exception:  # noqa: BLE001 - remote error body is untrusted.
            message = "bridge returned an invalid error"
            code = "protocol_error"
        raise BridgeError(str(code), str(message), error.code) from error
    if not isinstance(value, dict):
        raise BridgeError("protocol_error", "bridge returned a non-object response", 502)
    return value


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *_args: object, **_kwargs: object) -> None:
        return None


def _secure_state_dir(state_dir: Path) -> None:
    if not state_dir.is_absolute():
        raise BridgeError("invalid_state_dir", "state directory must be absolute", 400)
    state_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(state_dir, 0o700)


def _atomic_write(path: Path, data: bytes, mode: int) -> None:
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as temporary:
        temporary.write(data)
        temporary.flush()
        os.fsync(temporary.fileno())
        temporary_path = Path(temporary.name)
    os.chmod(temporary_path, mode)
    os.replace(temporary_path, path)


def _rotate_log(path: Path, maximum_bytes: int) -> None:
    try:
        if path.stat().st_size < maximum_bytes:
            return
    except FileNotFoundError:
        return
    rotated = path.with_suffix(path.suffix + ".1")
    rotated.unlink(missing_ok=True)
    path.replace(rotated)
    os.chmod(rotated, 0o600)


def _read_descriptor(state_dir: Path) -> Descriptor | None:
    path = state_dir / "daemon.json"
    try:
        value = json.loads(path.read_text())
        if not isinstance(value, dict):
            return None
        return Descriptor(
            instance_id=value["instanceId"],
            pid=value["pid"],
            process_start=value["processStart"],
            endpoint=value["endpoint"],
            credential_file=value["credentialFile"],
            protocol_version=value["protocolVersion"],
            bridge_version=value["bridgeVersion"],
            sdk_version=value["sdkVersion"],
        )
    except FileNotFoundError:
        return None
    except (KeyError, OSError, TypeError, ValueError, json.JSONDecodeError):
        return None


def _read_token(path: Path) -> str:
    return _read_credential(path)["token"]


def _read_credential(path: Path) -> dict[str, str | None]:
    metadata = path.lstat()
    if (
        stat.S_ISLNK(metadata.st_mode)
        or not stat.S_ISREG(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) & 0o077
    ):
        raise BridgeError(
            "localAuthentication",
            "credential file ownership or permissions are invalid",
            403,
        )
    try:
        value = json.loads(path.read_text())
        token = value["token"]
        salt = value["fingerprintSalt"]
        fingerprint = value.get("configFingerprint")
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise BridgeError("localAuthentication", "credential file is invalid", 403) from error
    if not isinstance(token, str) or len(token) < 43:
        raise BridgeError("localAuthentication", "credential token is invalid", 403)
    if (
        not isinstance(salt, str)
        or len(salt) != 64
        or not isinstance(fingerprint, (str, type(None)))
    ):
        raise BridgeError("localAuthentication", "credential fingerprint is invalid", 403)
    return {
        "token": token,
        "fingerprintSalt": salt,
        "configFingerprint": fingerprint,
    }


def _descriptor_token_path(state_dir: Path, descriptor: Descriptor) -> Path:
    path = Path(descriptor.credential_file)
    try:
        resolved = path.resolve(strict=True)
        expected_parent = state_dir.resolve(strict=True)
    except OSError as error:
        raise BridgeError("invalid_descriptor", "bridge credential is unavailable", 400) from error
    if resolved.parent != expected_parent or resolved.name != "credential.json":
        raise BridgeError(
            "invalid_descriptor",
            "bridge credential must be owned by the state directory",
            400,
        )
    return resolved


def _validate_base_url(base_url: str) -> None:
    parsed = urlparse(base_url)
    if parsed.scheme != "http" or parsed.username or parsed.password or parsed.path:
        raise BridgeError("invalid_descriptor", "bridge URL must be plain loopback HTTP", 400)
    try:
        address = __import__("ipaddress").ip_address(parsed.hostname or "")
    except ValueError as error:
        raise BridgeError("invalid_descriptor", "bridge host is not an IP address", 400) from error
    if not address.is_loopback or parsed.port is None:
        raise BridgeError("invalid_descriptor", "bridge URL is not loopback", 400)


def _process_start(pid: int) -> str:
    value = Path(f"/proc/{pid}/stat").read_text()
    fields_after_name = value[value.rfind(")") + 2 :].split()
    return fields_after_name[19]


def _same_process(pid: int, expected_start: str) -> bool:
    try:
        return _process_start(pid) == expected_start
    except (FileNotFoundError, IndexError, PermissionError, ProcessLookupError):
        return False


def _quarantine_stale(state_dir: Path) -> None:
    stamp = f"{time.time_ns()}"
    for name in ("daemon.json",):
        path = state_dir / name
        if path.exists():
            path.replace(state_dir / f"{name}.stale-{stamp}")
    credential = state_dir / "credential.json"
    if credential.exists():
        credential.replace(state_dir / f"{credential.name}.stale-{stamp}")


def _descriptor_json(descriptor: Descriptor) -> dict[str, Any]:
    return {
        "instanceId": descriptor.instance_id,
        "pid": descriptor.pid,
        "processStart": descriptor.process_start,
        "endpoint": descriptor.endpoint,
        "credentialFile": descriptor.credential_file,
        "protocolVersion": descriptor.protocol_version,
        "bridgeVersion": descriptor.bridge_version,
        "sdkVersion": descriptor.sdk_version,
    }


def _configuration_fingerprint(salt: bytes, sdk_version: str) -> str | None:
    ak = os.environ.get("FORNAX_AK")
    sk = os.environ.get("FORNAX_SK")
    if not ak and not sk:
        return None
    if not ak or not sk:
        raise BridgeError("sdkAuthentication", "FORNAX_AK and FORNAX_SK are required", 503)
    region = os.environ.get("FORNAX_CUSTOM_REGION", "")
    payload = f"{ak}\0{sk}\0{region}\0{sdk_version}".encode()
    return hmac.new(salt, payload, hashlib.sha256).hexdigest()


def _verify_supplied_configuration(path: Path, sdk_version: str) -> None:
    if not os.environ.get("FORNAX_AK") and not os.environ.get("FORNAX_SK"):
        return
    credential = _read_credential(path)
    salt = bytes.fromhex(str(credential["fingerprintSalt"]))
    expected = _configuration_fingerprint(salt, sdk_version)
    actual = credential["configFingerprint"]
    if not isinstance(actual, str) or not hmac.compare_digest(actual, expected or ""):
        raise BridgeError(
            "configurationMismatch",
            "supplied SDK configuration does not match the running bridge",
            409,
        )
