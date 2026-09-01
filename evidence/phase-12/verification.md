# Phase 12 verification

Status: `Done`

Implementation commits: `8f8951e`, `f4d48fa`, `223cdf4`

## Implemented scope

- `PhysicsBody` integrates release velocity and constant gravity in logical pixels using the fixed update interval.
- Ground collision uses the active monitor work area and solves the collision time inside a fixed step, preventing tunneling and preserving horizontal travel.
- Positions below the work-area ground are clamped immediately; vertical velocity is cleared and `grounded` is set exactly once.
- State transitions are `Dragged -> Falling -> Landing -> Idle`; Brain intents remain suppressed during Falling and Landing.
- Landing uses the existing Idle animation fallback for the fixed 250 ms landing phase.
- Native platform movement is attempted before the confirmed physics position changes. Failed platform moves preserve the previous body and velocity.
- macOS uses an AppKit monitor snapshot fallback when winit temporarily reports no monitors; fallback warnings are emitted once per fallback period.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 88 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |
| `cargo check --workspace --all-targets --target x86_64-pc-windows-msvc` | 0 | Pass | Windows MSVC target completed |
| `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings` | 0 | Pass | No Windows target issues found |

Physics tests cover upward and downward release, release below ground, exact collision position, horizontal velocity preservation, large fixed steps, multiple fixed-step sizes, failed platform movement rollback, and single landing completion. Display tests cover active monitor selection and work areas smaller than the window.

## macOS acceptance

| Date | Commit | Platform | Acceptance | Result | Evidence |
| --- | --- | --- | --- | --- | --- |
| 2026-09-01 | `223cdf4` | macOS arm64, Apple M5, Metal | User-confirmed direct release and upward throw; continuous fall, work-area ground clamp, no repeated landing, return to Idle | Pass | User confirmation in development session |

The interactive process used for acceptance exited before its PTY log could be collected, so this record intentionally relies on the user's explicit confirmation rather than fabricated runtime timestamps. The instance started with an AppKit-derived work area (`x=0, y=30, width=2560, height=1343`) and exercised the macOS runtime path. Mixed-DPI hardware was unavailable; mixed-scale behavior remains covered by deterministic tests.

## CI acceptance

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-08-31 | `223cdf4` | `macos-latest` | Pass | <https://github.com/adoom2017/3d-pet/actions/runs/33400199129> |
| 2026-08-31 | `223cdf4` | `windows-latest` | Pass | <https://github.com/adoom2017/3d-pet/actions/runs/33400199129> |

Windows acceptance is intentionally limited to strict Clippy, tests, and build. Windows runtime physics has not been verified.

All Phase 12 exit gates passed. Phase 12 is `Done`; Phase 13 is `Ready`.
