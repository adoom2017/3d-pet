# Phase 5 verification

Status: `Done`

Implementation commit: `aa171afc4d4cacd87fcfb74aff0b981594f8524e`

## Implemented scope

- `AnimationRequest` exposes only the semantic `Idle` and `Walk` requests to application code; raw GLB clip names remain inside the trusted asset and animation boundary.
- `AnimationController` keeps independent looping time for each clip, validates playback speed, ignores repeated requests, and cross-fades local translation, rotation, and scale before rebuilding global joint matrices.
- Every transition lasts 250 ms in real update time. Reversing a transition captures the currently blended pose so the next transition starts without a discontinuity.
- Pressing Space requests the opposite semantic state. The window title records the requested state for macOS automation without adding visible UI to the borderless pet window.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 29 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |

Deterministic animation coverage includes exact 0 ms, 125 ms, and 250 ms blend poses, repeated-request idempotence, mid-transition reversal continuity, clip looping, playback speed, invalid animation data, hierarchy cycles, and the renderer skinning smoke test.

## CI acceptance

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-08-31 | `aa171afc4d4cacd87fcfb74aff0b981594f8524e` | `macos-latest` | Pass (41s) | <https://github.com/adoom2017/3d-pet/actions/runs/33361469901> |
| 2026-08-31 | `aa171afc4d4cacd87fcfb74aff0b981594f8524e` | `windows-latest` | Pass (1m03s) | <https://github.com/adoom2017/3d-pet/actions/runs/33361469901> |

Windows acceptance for this phase is limited to CI format, Clippy, tests, and build; Windows runtime verification is not required.

## macOS acceptance

The repository binary was launched in a temporary macOS application bundle and exercised through direct CoreGraphics keyboard events. The first directed Space event changed the CoreGraphics window name to `DesktopPet [Walk]`. Five additional Space events at 400 ms intervals completed successfully and ended at `DesktopPet [Idle]`, confirming repeated bidirectional semantic requests.

Visual inspection found a clearly distinct Walk pose with lifted and extended legs. The final Idle pose remained coherent after repeated transitions. No visible pop, joint explosion, collapsed mesh, clipping, or transition restart jitter was observed.

Evidence captures:

- `screenshots/macos-idle-before-transition.png`
- `screenshots/macos-walk-after-transition.png`
- `screenshots/macos-idle-after-repeated-transitions.png`

The macOS visual gate, local automated gates, and macOS/Windows CI gates passed. Phase 5 is `Done`; Phase 6 is `Ready`.
