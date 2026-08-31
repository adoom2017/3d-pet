# Phase 3 verification

Status: `Done`

Implementation commit: `2938c0b18fdf06e200ac23996e1624fde4582ef1`

## Environment

| Date | Commit | Platform | GPU | Result | Evidence |
| --- | --- | --- | --- | --- | --- |
| 2026-08-31 | `2938c0b18fdf06e200ac23996e1624fde4582ef1` | macOS 26.6.2 (25G83), arm64, Retina 2x | Apple M5, Metal | Pass | This document and `screenshots/macos-static-fox.png` |

The runtime selected `Bgra8UnormSrgb`, FIFO presentation, and `PostMultiplied` alpha compositing for a 640 x 640 physical surface backing the 320 x 320 logical window.

## Asset provenance

The default pet is Quaternius' Fox from the Ultimate Animated Animal Pack. The repository contains the official CC0 license and a complete source record in `assets/pets/default/SOURCE.md`.

| Artifact | Size | SHA-256 |
| --- | ---: | --- |
| Repository `pet.glb` | 1,846,576 bytes | `c2cdd61d1ac40b1aa1a5b621f2ab1a39cf546d23a3a3a30e4cd4001273518870` |
| Official license copy | 364 bytes | `83d8959f9fc56353ed571fbe2dc52e4bcd64508e2399501cd45ac2ce3df0bf8c` |

The loader verified five mesh primitives, 12 named animations, the required `Idle` and `Walk` mappings, and the `Head` skeleton mapping. Runtime bounds were `(-0.5308331, -0.001930638, -3.180312)` to `(0.5308331, 2.6747665, 2.7027435)`.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 19 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |

Asset tests cover the valid default pet, missing and oversized manifests, pet-root path escape, corrupt GLB input, and invalid animation mappings. The static model GPU smoke uploads the default Fox, renders its camera, materials, meshes, and depth attachment offscreen, then asserts both visible model pixels and transparent clear pixels. Camera and material mapping tests cover degenerate bounds, portrait viewports, and all glTF alpha modes.

## macOS acceptance

The repository binary was launched through a temporary application bundle so LaunchServices placed its Metal surface in the active GUI Space. The wrapper was removed after capture and is not a project artifact.

The 640 x 640 Retina capture shows the static Fox fully inside the viewport. Visual inspection confirms:

- coherent node transforms and depth ordering;
- expected orange, white, gray, and dark material regions;
- a stable three-quarter view with no clipped head, feet, or tail;
- transparent model surroundings that reveal the live terminal content beneath the window;
- clean polygon edges without an opaque rectangular surface background.

The first surface acquisition can report `Occluded` while the hidden startup window is being mapped. Before the first successful presentation only, the application polls and retries; after the first frame it returns to event-driven `ControlFlow::Wait`. Runtime logs confirmed `first wgpu frame presented` immediately after the transient state.

## CI acceptance

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-08-31 | `981fef7f3f27214828f8d2ecf18f92ac960c121f` | `macos-latest` | Pass (1m25s) | <https://github.com/adoom2017/3d-pet/actions/runs/33353754622> |
| 2026-08-31 | `981fef7f3f27214828f8d2ecf18f92ac960c121f` | `windows-latest` | Pass (2m28s) | <https://github.com/adoom2017/3d-pet/actions/runs/33353754622> |

The trusted asset record, macOS runtime and visual acceptance, deterministic asset and renderer tests, local gates, and both CI jobs passed. Phase 3 is `Done`; Phase 4 is `Ready`.
