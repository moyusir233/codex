"""Stable bridge error codes and sanitized envelopes."""

from dataclasses import dataclass


@dataclass(slots=True)
class BridgeError(Exception):
    """An expected protocol/lifecycle/SDK failure."""

    code: str
    message: str
    status: int = 400
    retry: str = "never"
    operation_id: str | None = None

    def envelope(self) -> dict[str, object]:
        return {
            "error": {
                "code": self.code,
                "message": self.message,
                "operationId": self.operation_id,
                "retry": self.retry,
            }
        }


def invalid(message: str) -> BridgeError:
    return BridgeError("invalidRequest", message, 400)


def conflict(message: str) -> BridgeError:
    return BridgeError("idempotencyConflict", message, 409)


def unavailable(message: str) -> BridgeError:
    return BridgeError("sdkUnavailable", message, 503, retry="safe")
