# Phase 9 verification

Status: `Done`

Implementation commit: `fff27349738491bde49a1d6766c468678ce6fa04`

## Implemented scope

- `MouseState` owns optional desktop logical and window-local logical cursor positions plus left-button and modifier state.
- `DisplayManager` exposes the only coordinate chain: desktop logical to window logical, logical to physical, physical to NDC. Invalid scale, zero viewport, outside points, and non-finite values return `None`.
- `CameraRay::from_ndc` safely unprojects near/far points for future exact 3D hit testing and rejects singular matrices.
- `RectHitRegion` projects all eight model AABB corners through the renderer's current model/view/projection matrix, converts the result to logical coordinates, applies a six-pixel logical padding, and clamps it to the viewport.
- Rendering and hit projection share facing, viewport, DPI, and configured pet scale. The CPU never reads the GPU framebuffer.
- `InteractionController` emits observable logs only when hit state changes; trace-level evaluation remains available for diagnostics.
- Window movement preserves the cursor's desktop position and recomputes its window-local position, preventing drift while the autonomous pet moves beneath a stationary pointer.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 65 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |

Tests cover negative desktop origins, logical/physical round trips, 1.25x/2x/Retina conversion, NDC corners, outside and zero-size rejection, inclusive hit boundaries, NaN protection, viewport/DPI stability, model facing and scale, singular camera matrices, MouseState movement semantics, and hit-state change suppression.

The local Windows MSVC all-target check and strict target Clippy both passed. GitHub Actions remains the authoritative Windows test/build gate.

## macOS acceptance

| Date | Commit | Platform | Acceptance | Result | Evidence |
| --- | --- | --- | --- | --- | --- |
| 2026-08-31 | `fff27349738491bde49a1d6766c468678ce6fa04` | macOS 26.6.2 (25G83), arm64, Apple M5, Metal, Retina 2x | Projected fox region and adjacent transparent-area pointer transitions | Pass | `macos-observation.log` |

The 320x320 logical window used a 640x640 physical surface. The runtime-derived logical hit region was `[56.86, 52.35]..[233.15, 235.26]`. A real cursor event inside that region produced hit; a point immediately left of its minimum X produced miss. The window continued autonomous movement while cursor desktop/local coordinates remained coherent.

Cross-DPI behavior is covered deterministically at 1.25x and 2x. Both available physical monitors reported Retina 2x, so a physical 1.25x monitor was not available for macOS runtime acceptance.

## CI acceptance

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-08-31 | `fff27349738491bde49a1d6766c468678ce6fa04` | `macos-latest` | Pass (29s) | <https://github.com/adoom2017/3d-pet/actions/runs/33376698390> |
| 2026-08-31 | `fff27349738491bde49a1d6766c468678ce6fa04` | `windows-latest` | Pass (1m28s) | <https://github.com/adoom2017/3d-pet/actions/runs/33376698390> |

Windows acceptance is limited to CI format, strict Clippy, tests, and build. Windows runtime behavior has not been verified.

The local automated gates, macOS Retina pointer acceptance, and macOS/Windows CI gates passed. Phase 9 is `Done`; Phase 10 is `Ready`.
