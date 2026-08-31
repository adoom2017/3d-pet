# Phase 4 verification

Status: `Verifying`

Implementation commit: `bdcc12768d95de1601a9213254454a856b5ed9b6`

## Implemented scope

- The trusted GLB loader reads node TRS hierarchy, skins, inverse bind matrices, `JOINTS_0`, `WEIGHTS_0`, and named animation channels.
- Step and linear translation, rotation, and scale channels are supported. Cubic spline and morph animation fail with explicit errors.
- `AnimationController` samples from bind pose with fixed `1/60s` updates, resolves parent-first global transforms, loops Idle, and generates `joint_global * inverse_bind` matrices.
- The renderer uploads a 128-joint palette per skin and performs weighted GPU vertex skinning. Unskinned primitives use an identity palette.
- Animation time advances only after a successful presentation, so surface retries cannot advance the simulation twice.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 26 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |

Deterministic animation tests cover identity bind pose, a single joint, parent/child hierarchy composition, linear sampling, step sampling, exact loop wrap, invalid channel targets, invalid skin joints, and cyclic hierarchy rejection. The offscreen default-pet smoke compiles and executes the skinning shader with the Fox bind-pose joint palette.

## CI acceptance

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-08-31 | `bdcc12768d95de1601a9213254454a856b5ed9b6` | `macos-latest` | Pass (44s) | <https://github.com/adoom2017/3d-pet/actions/runs/33358345423> |
| 2026-08-31 | `bdcc12768d95de1601a9213254454a856b5ed9b6` | `windows-latest` | Pass (1m32s) | <https://github.com/adoom2017/3d-pet/actions/runs/33358345423> |

## Pending macOS acceptance

The current automated GUI session created a valid LaunchServices process but did not deliver winit's initial `resumed` callback, including with a fresh direct Mach-O application bundle. No animation window was therefore available to capture. The repository keeps the Phase 3 hidden-window startup strategy because it was previously verified on the same Mac; the failed wrapper experiments were removed.

Phase 4 remains `Verifying` until a normal interactive macOS launch is observed for multiple Idle cycles and screenshot or video evidence confirms no exploding joints, collapsed mesh, or visible loop seam. Phase 5 remains `Blocked`.
