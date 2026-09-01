# Phase 13 verification

Status: `Done`

Implementation commit: `e824ff3`

## Implemented scope

- `LookTarget` derives yaw and pitch from the projected head position and the global mouse position in window-logical coordinates.
- Yaw is clamped to `[-40, 40]` degrees and pitch to `[-20, 25]` degrees.
- A fixed-step exponential response smooths target changes without overshoot and returns to neutral when the target is unavailable.
- The procedural head pose is applied after clip sampling and Cross Fade without contaminating the stored base pose.
- The pet manifest configures the head joint, yaw/pitch axes, and axis signs for the selected GLB asset.
- Missing joints or invalid axes disable only Look At and emit one warning when the asset controller is loaded.
- Facing changes mirror yaw so the same desktop target remains correct for both model orientations.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 94 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |
| `cargo check --workspace --all-targets --target x86_64-pc-windows-msvc` | 0 | Pass | Windows MSVC target completed |
| `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings` | 0 | Pass | No Windows target issues found |

Deterministic tests cover center and quadrant targets, asymmetric clamping, invalid coordinates, 30/60 Hz convergence, overshoot prevention, target loss, pose-overlay ordering, missing joints, invalid axes, the default GLB head-node mapping, and projected model points.

## macOS acceptance

| Date | Commit | Platform | Acceptance | Result | Evidence |
| --- | --- | --- | --- | --- | --- |
| 2026-09-01 | `e824ff3` | macOS arm64, Apple M5, Metal, two Retina 2x displays | Mouse movement above, below, left, right, and around the pet; smooth and rapid paths; Idle/Walk animation; both facings | Pass | User confirmation in development session; runtime log observed in the same session |

The acceptance instance logged `look-at pose layer enabled` with head node `0`, yaw axis `(0, 1, 0)`, pitch axis `(1, 0, 0)`, and positive axis signs. Runtime logs also showed repeated Idle/Walk transitions, left/right facing changes, and live pointer hit transitions while the user performed the visual check. Transparent Metal window-layer screenshots did not reliably capture changing frames, so no recording is claimed. Both attached displays were Retina 2x; mixed-DPI hardware was unavailable and coordinate behavior remains covered by deterministic tests.

## CI acceptance

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-09-01 | `e824ff3` | `macos-latest` | Pass (45s) | <https://github.com/adoom2017/3d-pet/actions/runs/33457579811> |
| 2026-09-01 | `e824ff3` | `windows-latest` | Pass (1m8s) | <https://github.com/adoom2017/3d-pet/actions/runs/33457579811> |

Windows acceptance is intentionally limited to strict Clippy, tests, and build. Windows runtime Look At behavior has not been verified.

All Phase 13 exit gates passed. Phase 13 is `Done`; Phase 14 is `Ready`.
