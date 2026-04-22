# codex-realtime-webrtc Code Analysis

## Overview

`codex-realtime-webrtc` is a small adapter crate that gives the rest of the workspace a minimal,
platform-gated WebRTC transport for realtime audio sessions.

At a high level, the crate:

- exposes a synchronous Rust API for starting a WebRTC session,
- creates a local SDP offer,
- accepts a remote SDP answer later,
- emits coarse lifecycle and local-audio-meter events through a standard channel,
- hides all `libwebrtc` details behind a small public surface,
- and degrades cleanly on non-macOS platforms by returning `UnsupportedPlatform`.

The implementation is intentionally narrow: this crate does not manage signaling, backend RPCs,
remote audio rendering, or end-user UI state. Those responsibilities live in the TUI layer and the
app-server integration that sits above this crate.

## File Layout

- `Cargo.toml`: crate manifest, target-specific dependency on `libwebrtc`.
- `src/lib.rs`: public API, error/event types, macOS gating, session handle wrapper.
- `src/native.rs`: macOS-only implementation built around a worker thread plus a Tokio runtime.
- `BUILD.bazel`: Bazel wrapper that applies macOS WebRTC linker flags.

## Public Responsibilities

The crate has six concrete responsibilities:

1. **Create a local WebRTC offer**
   - `RealtimeWebrtcSession::start()` boots the implementation and returns an SDP offer string.

2. **Accept the remote answer**
   - `RealtimeWebrtcSessionHandle::apply_answer_sdp()` parses and installs the answer SDP.

3. **Provide lifecycle notifications**
   - The crate emits `Connected`, `Closed`, and `Failed(String)` through
     `std::sync::mpsc::Receiver<RealtimeWebrtcEvent>`.

4. **Provide local microphone level events**
   - The macOS backend polls `get_stats()` and emits `LocalAudioLevel(u16)` samples.

5. **Hide implementation details**
   - Consumers only see `StartedRealtimeWebrtcSession`, `RealtimeWebrtcSessionHandle`, and
     `RealtimeWebrtcEvent`.

6. **Handle unsupported platforms safely**
   - Public entry points compile everywhere, but non-macOS calls return
     `RealtimeWebrtcError::UnsupportedPlatform`.

## Public API

The public API is intentionally tiny.

### Types

- `RealtimeWebrtcError`
  - `Message(String)`: stringly-typed operational failures.
  - `UnsupportedPlatform`: explicit fallback for non-macOS builds.

- `RealtimeWebrtcEvent`
  - `Connected`
  - `LocalAudioLevel(u16)`
  - `Closed`
  - `Failed(String)`

- `StartedRealtimeWebrtcSession`
  - `offer_sdp: String`
  - `handle: RealtimeWebrtcSessionHandle`
  - `events: mpsc::Receiver<RealtimeWebrtcEvent>`

- `RealtimeWebrtcSessionHandle`
  - `apply_answer_sdp(answer_sdp: String) -> Result<()>`
  - `close()`
  - `local_audio_peak() -> Arc<AtomicU16>`

- `RealtimeWebrtcSession`
  - `start() -> Result<StartedRealtimeWebrtcSession>`

### API Shape Notes

- The crate presents a **blocking/synchronous API** to callers even though the macOS backend uses
  async WebRTC calls internally.
- Event delivery uses `std::sync::mpsc`, not Tokio channels, which makes the API usable from
  existing synchronous UI threads.
- `local_audio_peak()` exposes shared state, but the crate itself does not update that atomic after
  construction. The current caller in the TUI updates it when processing `LocalAudioLevel` events.

## Control Flow

### Startup Flow

1. Caller invokes `RealtimeWebrtcSession::start()`.
2. On macOS, `src/lib.rs` forwards to `native::start()`.
3. `native::start()` creates:
   - a command channel for control messages,
   - an events channel for outward notifications,
   - an offer channel used once during startup.
4. It spawns a dedicated worker thread named `codex-realtime-webrtc`.
5. The worker thread builds a multi-thread Tokio runtime.
6. The worker synchronously runs `create_peer_connection_and_offer()`.
7. If successful, the offer SDP is sent back to the caller and the worker stays alive waiting for
   commands.

### Offer Creation Flow

`create_peer_connection_and_offer()` performs the core WebRTC bootstrap:

1. Creates a `PeerConnectionFactory` with platform audio-device support via
   `with_platform_adm()`.
2. Creates a `PeerConnection` using default `RtcConfiguration`.
3. Adds one audio transceiver configured as `SendRecv`.
4. Creates a local audio source and a local audio track named `realtime-mic`.
5. Attaches that track to the transceiver sender.
6. Creates an SDP offer requesting audio but not video.
7. Sets that offer as the local description.
8. Returns `(PeerConnection, offer_sdp_string)`.

Important implication: this crate assumes a simple audio-only realtime session with one local audio
track and no explicit ICE/server customization.

### Answer Application Flow

1. Consumer receives the remote SDP answer from a higher layer.
2. Consumer calls `RealtimeWebrtcSessionHandle::apply_answer_sdp(answer_sdp)`.
3. The handle sends a `Command::ApplyAnswer` into the worker thread and waits for a reply.
4. The worker calls async `apply_answer()`, which:
   - parses the SDP as `SdpType::Answer`,
   - calls `set_remote_description(answer).await`.
5. On success, the worker emits `RealtimeWebrtcEvent::Connected`.
6. The worker also starts the local-audio polling task.

### Audio-Level Flow

Once connected:

1. `start_local_audio_level_task()` spawns an async task on the worker runtime.
2. Every 200 ms it checks whether the peer connection is `Closed` or `Failed`.
3. It calls `get_stats().await`.
4. It scans for the first `RtcStats::MediaSource` with `kind == "audio"`.
5. It reads `audio.audio_level`, clamps it to `[0.0, 1.0]`, scales it to `i16::MAX`, and rounds
   to `u16`.
6. It emits `RealtimeWebrtcEvent::LocalAudioLevel(peak)`.

This is a polling meter, not a push-driven signal from the WebRTC stack.

### Shutdown Flow

- `RealtimeWebrtcSessionHandle::close()` sends `Command::Close`.
- The worker closes the peer connection, emits `Closed`, and exits.
- If all handle senders are dropped, the worker also exits its command loop, closes the connection,
  and emits `Closed`.

## Internal Design

### Concurrency Model

The crate uses a layered concurrency model:

- **Public boundary**: synchronous API using standard channels.
- **Worker boundary**: one dedicated OS thread owns the session lifecycle.
- **Async boundary**: one Tokio runtime inside that thread executes async `libwebrtc` work.

This design keeps `libwebrtc` and Tokio out of the caller-facing API while still allowing async
operations internally.

### Command Pattern

`src/native.rs` uses a tiny command enum:

- `ApplyAnswer { answer_sdp, reply }`
- `Close`

That keeps cross-thread mutation serialized through a single worker owner of the
`PeerConnection`.

### Platform Gating

The crate is compiled for all workspace targets, but real functionality is only enabled on macOS:

- `src/lib.rs` conditionally includes `mod native;`.
- the `libwebrtc` dependency is only declared under
  `target.'cfg(target_os = "macos")'.dependencies`.
- non-macOS public entry points return `UnsupportedPlatform`.

This is a good fit for a workspace crate that must remain linkable in cross-platform builds.

### Error Strategy

Errors are deliberately simple:

- most implementation errors become `RealtimeWebrtcError::Message(String)`,
- platform mismatch becomes `UnsupportedPlatform`.

This keeps the API easy to consume but loses structured error detail such as failure kind,
underlying cause category, or recoverability.

## Dependency Analysis

### Direct Cargo Dependencies

- `thiserror`
  - derives the public error type.

- `tokio` with `rt-multi-thread`
  - used only inside the macOS worker implementation to host async WebRTC calls and the periodic
    stats task.

- `libwebrtc` on macOS only
  - provides `PeerConnectionFactory`, `PeerConnection`, session descriptions, transceivers, and
    stats access.
  - pinned to a Git revision of `juberti-oai/rust-sdks`.

### Build-System Dependency

- `BUILD.bazel` wraps the crate with `MACOS_WEBRTC_RUSTC_LINK_FLAGS`.
  - this signals that Bazel builds require extra linker configuration beyond Cargo metadata.

### Architectural Dependency

The crate depends on the TUI or another higher layer for:

- signaling transport to send the offer SDP outward,
- receipt of the answer SDP from the backend/service,
- event consumption and UI updates,
- microphone visualization using the emitted peak values,
- and any remote audio playback path.

## Consumer Integration

The main in-repo consumer is the TUI.

Observed flow from `tui/src/chatwidget/realtime.rs`:

1. The TUI starts a background task that calls `RealtimeWebrtcSession::start()`.
2. On success it forwards `offer_sdp` into the app-server `thread/realtime/start` request using
   `ConversationStartTransport::Webrtc { sdp }`.
3. When the backend later returns an SDP answer, the TUI calls `handle.apply_answer_sdp(sdp)`.
4. The TUI listens for crate events:
   - `Connected` moves the UI to the active realtime state.
   - `Closed` resets UI state.
   - `Failed(message)` marks the realtime conversation as failed.
   - `LocalAudioLevel(peak)` updates the recording meter.
5. The TUI stores the peak into the `Arc<AtomicU16>` returned by `local_audio_peak()`.

This shows the crate is strictly a local transport primitive, not the signaling orchestrator.

## Testing Status

### Current Coverage

- The crate currently contains **no unit tests**, **no integration tests**, and **no doctests**.
- `cargo test -p codex-realtime-webrtc --lib` succeeds, but reports `running 0 tests`.

### What Is Effectively Being Tested Today

Only indirect validation exists:

- the crate compiles successfully in the workspace,
- the TUI exercises it in real application flows,
- and the type surface is small enough that compile-time breakage would be visible quickly.

### Missing Tests That Would Add Value

1. **Pure unit tests**
   - `audio_level_to_peak()` boundary cases: `< 0`, `0`, fractional values, `1`, `> 1`.

2. **Worker protocol tests**
   - startup failure propagation,
   - `Close` event behavior,
   - dropped command channel behavior.

3. **Platform contract tests**
   - non-macOS `start()` and `apply_answer_sdp()` return `UnsupportedPlatform`.

4. **Mocked integration tests**
   - a test seam around signaling or peer-connection creation could validate command/event order
     without depending on a real WebRTC stack.

## Notable Design Strengths

- **Minimal public surface** keeps callers decoupled from `libwebrtc`.
- **Platform isolation** avoids leaking unsupported behavior into other targets.
- **Single-owner worker model** avoids shared mutable access to `PeerConnection`.
- **Synchronous public API** fits existing non-async UI code well.
- **Event-based meter updates** keep audio-level reporting decoupled from UI rendering.

## Notable Design Limitations

- **macOS-only implementation**
  - other platforms compile but cannot use the feature.

- **Stringly-typed errors**
  - operational failures lose machine-readable structure.

- **No explicit connection-state callbacks from WebRTC**
  - `Connected` is emitted after applying the answer, not after a stronger transport-ready signal.

- **No ICE/STUN/TURN customization**
  - `RtcConfiguration::default()` may be too limited for broader network environments.

- **No remote-track handling**
  - the crate sets up local audio sending and stats polling, but does not expose remote media
    events or playback hooks.

- **No restart/idempotency guard**
  - repeated `apply_answer_sdp()` calls appear allowed at the command layer, though semantic
    behavior depends on `libwebrtc`.

- **Shared atomic is consumer-maintained**
  - `local_audio_peak()` suggests crate-owned state, but the atomic is actually updated by the TUI.

## Open Questions

1. Should `Connected` mean “answer SDP accepted” or “ICE/DTLS connection actually established”?
2. Should `RealtimeWebrtcSessionHandle` reject repeated `apply_answer_sdp()` calls explicitly?
3. Is `RtcConfiguration::default()` sufficient, or should the caller provide ICE server config?
4. Should local audio peak be updated internally by the crate instead of by the consumer?
5. Does the product need remote audio/media events from this crate, or is playback handled
   entirely elsewhere?
6. Should the worker surface richer typed errors for startup, SDP parse, runtime, and stats
   failures?
7. Is polling `get_stats()` every 200 ms acceptable for CPU/battery usage on laptops?
8. Should there be a trait seam to make the worker logic testable without a concrete `libwebrtc`
   dependency?

## Key Source References

- Public API: `src/lib.rs`
- macOS implementation: `src/native.rs`
- Bazel linkage: `BUILD.bazel`
- Main consumer flow: `../tui/src/chatwidget/realtime.rs`
