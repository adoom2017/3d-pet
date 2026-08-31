# Phase 11 verification

Status: `Done`

Implementation commit: `619c7560adbef4977f23ac8e2a725ab40053b3a1`

## Implemented scope

- `InteractionController` owns explicit `Hovering`, `Pressed`, and `Dragged` pointer states with a 5 logical-pixel drag threshold.
- Drag positions use the absolute desktop pointer position minus the original press offset, so skipped move events do not accumulate position error.
- Recent timestamped pointer samples are bounded by count and age; release velocity is capped before being transferred to `PhysicsBody` for Phase 12.
- `PetState::Dragged` has drag priority and suppresses Brain transitions and ordinary movement until an explicit release or cancellation.
- Native window movement is applied before the movement controller confirms its owned desktop position.
- Focus loss cancels capture with zero release velocity. Active press/drag keeps native click-through disabled until release.
- macOS accepts the first mouse event and verifies each `ignoresMouseEvents` update through AppKit readback.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 80 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |
| `cargo check --workspace --all-targets --target x86_64-pc-windows-msvc` | 0 | Pass | Windows MSVC target completed |
| `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings` | 0 | Pass | No Windows target issues found |

Deterministic tests cover threshold behavior, preserved press offset, skipped move events, bounded and expired release samples, non-monotonic timestamps, speed clamping, cancellation, missing release coordinates, drag state priority, and movement ownership.

## macOS acceptance

| Date | Commit | Platform | Acceptance | Result | Evidence |
| --- | --- | --- | --- | --- | --- |
| 2026-08-31 | `619c7560adbef4977f23ac8e2a725ab40053b3a1` | macOS 26.6.2 (25G83), arm64, Apple M5, Metal, two Retina 2x displays | Slow/fast drag, preserved grab point, work-area edge, second display, release, click-through recovery | Pass | `macos-observation.log`; user confirmation in the development session |

Runtime logs show real pointer down/up events, entry into and exit from `Dragged`, non-zero release velocities, and restoration of `click-through=true`. A drag used secondary-display desktop coordinates (`x=2505` to `x=2112`), confirming the multi-display path. Both attached displays are Retina 2x, so mixed-DPI hardware remains unavailable; mixed-scale conversion is covered by deterministic display and pointer tests.

## CI acceptance

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-08-31 | `619c7560adbef4977f23ac8e2a725ab40053b3a1` | `macos-latest` | Pass (52s) | <https://github.com/adoom2017/3d-pet/actions/runs/33388676133> |
| 2026-08-31 | `619c7560adbef4977f23ac8e2a725ab40053b3a1` | `windows-latest` | Pass (1m11s) | <https://github.com/adoom2017/3d-pet/actions/runs/33388676133> |

Windows acceptance is intentionally limited to strict Clippy, tests, and build. Windows runtime dragging has not been verified.

All Phase 11 exit gates passed. Phase 11 is `Done`; Phase 12 is `Ready`.
