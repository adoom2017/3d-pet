# Phase 6 verification

Status: `Done`

Implementation commit: `1c0a8551ec2e212cb0a348038114b4b9e5d9953e`

## Implemented scope

- `DesktopPosition` is the logical desktop-coordinate value and owns consistent system-position rounding, including negative coordinates.
- `PhysicsBody` is the sole logical window-position source. `MovementController` proposes fixed-step positions and commits them only after the platform move succeeds.
- macOS and Windows platform backends normalize winit physical window positions to logical pixels and accept logical positions through the shared `PlatformBackend` contract.
- A bounded accumulator runs logic at 60 Hz independently of presentation frequency. It executes at most five catch-up steps per event-loop turn and reports dropped backlog after long pauses.
- Left, Right, and Space commands synchronize movement state, semantic Idle/Walk requests, window title, and screen-relative model facing.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 37 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |

Deterministic tests cover 80 logical px/s integration, negative-coordinate rounding, successful and rejected platform-position mocks, rejected move rollback, equal two-second positions at 15/30/60/120 render FPS, accumulator remainders and catch-up limits, facing transforms, animation transitions, and the renderer smoke tests.

## CI acceptance

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-08-31 | `1c0a8551ec2e212cb0a348038114b4b9e5d9953e` | `macos-latest` | Pass (46s) | <https://github.com/adoom2017/3d-pet/actions/runs/33363672387> |
| 2026-08-31 | `1c0a8551ec2e212cb0a348038114b4b9e5d9953e` | `windows-latest` | Pass (1m18s) | <https://github.com/adoom2017/3d-pet/actions/runs/33363672387> |

Windows acceptance for this phase is limited to CI format, Clippy, tests, and build; Windows runtime verification is not required.

## macOS acceptance

The repository binary was launched through a temporary macOS application bundle in the current Space. Direct CoreGraphics key events exercised Right, Left, and Space without relying on focus behavior of the borderless `LSUIElement` window.

A controlled two-second leftward interval moved the window from X=4643 to X=4484, a 159 logical-pixel displacement consistent with the configured 80 logical px/s and fixed-step rounding. The title changed to `DesktopPet [Walking]` while moving and `DesktopPet [Idle]` after stopping. Repeated long-distance movement produced no platform error or crash; one intentionally blocked event-loop interval emitted the expected backlog-drop warning.

Visual inspection confirms that Right faces the fox to screen-right, Left faces it to screen-left, Walk remains animated while the window moves, and stopping cross-fades back to a coherent Idle pose without changing the last facing direction. No visible movement jitter, skeletal deformation, or transition pop appeared.

Evidence captures:

- `screenshots/macos-walking-right.png`
- `screenshots/macos-walking-left.png`
- `screenshots/macos-idle-after-movement.png`

Window capture renders transparent pixels as black; the live window remained transparently composited over the desktop. Work-area clamping is intentionally deferred to Phase 8.

The macOS visual and interaction gate, local automated gates, and macOS/Windows CI gates passed. Phase 6 is `Done`; Phase 7 is `Ready`.
