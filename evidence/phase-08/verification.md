# Phase 8 verification

Status: `Done`

Implementation commit: `dd158a9fb784336ed933cf6554698463e8420e4b`

## Implemented scope

- `DisplayManager` owns validated monitor snapshots and selects an active monitor by window center, maximum intersection area, then primary fallback.
- Window constraints use native work areas, preserve negative desktop origins, clamp all window edges, allow movement into an overlapping adjacent monitor, and request a reverse Turn at a terminal horizontal edge.
- macOS enumerates `NSScreen.visibleFrame` and converts Cocoa bottom-left coordinates to the shared top-left logical coordinate system.
- Windows enumerates `GetMonitorInfoW.rcWork`; physical rectangles are normalized by the monitor scale factor. Windows runtime acceptance is not required.
- Monitor snapshots refresh every simulation second and immediately after `ScaleFactorChanged`; an empty snapshot leaves proposed movement unchanged.
- `MovementController` commits the position accepted by the display/platform boundary, so a failed native move cannot corrupt its authoritative position.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 52 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |

Tests cover monitor validation, 125%/200%/Retina physical-to-logical conversion, center/intersection/primary selection, negative-left and stacked layouts, adjacent crossing, work-area clamping, reverse Turn generation, empty monitor fallback, topology refresh, and constrained movement commits.

An additional local `cargo check --workspace --all-targets --target x86_64-pc-windows-msvc` and strict Windows-target Clippy both passed. GitHub Actions remains the authoritative Windows build/test gate.

## macOS acceptance

| Date | Commit | Platform | Acceptance | Result | Evidence |
| --- | --- | --- | --- | --- | --- |
| 2026-08-31 | `dd158a9fb784336ed933cf6554698463e8420e4b` | macOS 26.6.2 (25G83), arm64, Apple M5, Metal | Retina dual-monitor work areas and autonomous boundary Turn | Pass | `macos-observation.log` |

The runtime reported a primary work area `(0, 33) 1512x882 @2x` and a secondary work area `(1512, -106) 2560x1410 @2x`. Visual inspection confirmed transparent composition, visible Idle/Walk animation, and placement above reserved menu-bar/Dock regions. Structured logs confirmed each terminal-edge collision entered one reverse Turning state and resumed Walking after the state machine's 250 ms turn duration.

Hotplug handling is covered by snapshot replacement tests and periodic runtime refresh. Physical disconnect/reconnect was not performed during this acceptance run.

## CI acceptance

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-08-31 | `dd158a9fb784336ed933cf6554698463e8420e4b` | `macos-latest` | Pass (39s) | <https://github.com/adoom2017/3d-pet/actions/runs/33371035212> |
| 2026-08-31 | `dd158a9fb784336ed933cf6554698463e8420e4b` | `windows-latest` | Pass (1m41s) | <https://github.com/adoom2017/3d-pet/actions/runs/33371035212> |

Windows acceptance is limited to CI format, strict Clippy, tests, and build. Windows runtime behavior has not been verified.

The local automated gates, macOS dual-monitor acceptance, and macOS/Windows CI gates passed. Phase 8 is `Done`; Phase 9 is `Ready`.
