# Phase 4 verification

Status: `Done`

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

## macOS acceptance

The repository binary was launched as a direct Mach-O executable in a fresh temporary application bundle. Its 320 x 320 logical window used a 640 x 640 Retina Metal surface and remained alive for approximately 4 minutes 20 seconds while repeatedly playing Idle.

Three window-region captures are stored in `screenshots/macos-idle-frame-a.png`, `macos-idle-frame-b.png`, and `macos-idle-frame-c.png`. Visual inspection confirms that the head and ears move subtly while the feet, torso, and tail remain coherent; no joint explosion, collapsed mesh, clipping, material corruption, or visible loop discontinuity appeared. The transparent surroundings continued to reveal the live desktop below the model.

Frames A and B were captured approximately one second apart. Decoded pixel comparison reported:

```text
size=640x640 changed_pixels=5363 max_channel_delta=220
```

This proves that the presented Idle pose changed between captures rather than displaying a static bind pose. Frame C was captured after several additional minutes and remained geometrically stable.

The macOS visual gate, local automated gates, and macOS/Windows CI gates passed. Phase 4 is `Done`; Phase 5 is `Ready`.
