# Phase 10 verification

Status: `Verifying`

Implementation commit: `pending`

## Implemented scope

- `PlatformBackend` exposes idempotent click-through control and normalized global cursor polling.
- macOS switches `NSWindow.ignoresMouseEvents` only when the requested state changes. While ignored, `NSEvent.mouseLocation` keeps hit testing alive so the pet can recover without restarting.
- macOS cursor and monitor conversion use the stable primary item in `NSScreen.screens`, rather than the key-window-dependent `mainScreen` coordinate reference.
- Windows installs a scoped `WM_NCHITTEST` subclass: pet hits return `HTCLIENT`, transparent misses return `HTTRANSPARENT`, and the subclass is removed before its atomic state is dropped.
- `InteractionController` keeps event delivery enabled from a valid pet press through release and emits `ClickPet` only for a complete click that starts and ends on the pet.
- `ClickPet` submits observable `PetIntent::Interact`; the state machine uses Idle animation fallback and deterministically returns `Interacting -> Idle` after 500 ms.
- Focus loss cancels an incomplete click. Fatal, close, and event-loop exit paths restore native mouse handling before teardown.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 72 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |
| `cargo check --workspace --all-targets --target x86_64-pc-windows-msvc` | 0 | Pass | Windows MSVC target completed |
| `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings` | 0 | Pass | No Windows target issues found |

Tests cover hit-to-platform policy, rapid hit changes, complete/cancelled clicks, press retention, global cursor conversion, interaction timeout, idempotent native updates including retry after failure, and Windows hit-test return mapping.

## macOS acceptance

| Date | Commit | Platform | Acceptance | Result | Evidence |
| --- | --- | --- | --- | --- | --- |
| 2026-08-31 | `pending` | macOS 26.6.2 (25G83), arm64, Apple M5, Metal, Retina 2x | Transparent click, pet click recovery, rapid enter/leave, moving window, focus changes, teardown | Pass | `macos-observation.log` |

With Finder established as the frontmost baseline, a CoreGraphics click at the moving pet window's transparent top-left corner changed the frontmost process to the actual underlying DingTalk window. Without restarting DesktopPet, a click in the runtime-projected fox region produced `PetIntent::Interact`, entered `Interacting`, and returned to `Idle` after approximately 500 ms.

Repeated real pointer crossings produced alternating hit/miss and click-through false/true transitions as close as one fixed update apart, then recovered normally. Activating other applications did not leave the pet permanently interactive or permanently ignored. The window continued moving while global cursor polling updated local hit coordinates.

Both attached displays reported Retina 2x, so mixed-DPI hardware was unavailable. DPI behavior remains covered by deterministic coordinate and projected-region tests; the two-display runtime retained a stable primary-screen coordinate reference after the AppKit normalization fix.

## CI acceptance

GitHub Actions macOS/Windows matrix: pending.

Windows acceptance is intentionally limited to format, strict Clippy, tests, and build. Windows runtime mouse behavior has not been verified.
