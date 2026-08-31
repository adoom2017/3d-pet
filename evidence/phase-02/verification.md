# Phase 2 verification

Status: `Done`

Implementation commit: `5a86b21ecfe55400df14e5e2a32db3eac65f659b`

## Environment

| Date | Commit | Platform | GPU | Result | Evidence |
| --- | --- | --- | --- | --- | --- |
| 2026-08-31 | `5a86b21ecfe55400df14e5e2a32db3eac65f659b` | macOS 26.6.2 (25G83), arm64, Retina 2x | Apple M5, Metal | Pass | This document and `screenshots/macos-triangle-alpha.png` |

The runtime selected `Bgra8UnormSrgb`, FIFO presentation, and `PostMultiplied` alpha compositing for a 640 x 640 physical surface backing the 320 x 320 logical window.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 10 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |

The renderer tests cover zero-sized surface handling, transparent alpha-mode selection, adapter/device initialization, and offscreen rendering. The offscreen smoke renders the same WGSL triangle into a 64 x 64 RGBA texture and asserts that a corner pixel has alpha 0 while the triangle center has alpha 255.

## macOS acceptance

The repository binary was wrapped in a temporary `/private/tmp` application bundle solely so LaunchServices would place its Metal surface in the active GUI Space. The wrapper was deleted after capture and is not a project artifact.

The window-only Retina capture is 640 x 640 pixels. Pixel inspection reported:

```text
size=640x640 min_alpha=0.0 max_alpha=1.0 nonzero_alpha=98606 opaque=98606 colored=98606
```

The 98,606 colored triangle pixels are fully opaque. Every pixel outside the triangle has alpha 0, proving that the surface clear remains transparent rather than black. Visual inspection confirms the RGB interpolation, triangle geometry, and clean transparent edges.

## Surface recovery and failures

- A zero-sized resize suspends surface configuration; the next non-zero resize configures the new dimensions.
- `Lost` recreates the surface, `Outdated` reconfigures it, and `Suboptimal` presents before reconfiguration.
- `Timeout` retries a later redraw and `Occluded` returns the event loop to `Wait`.
- `Validation`, uncaptured out-of-memory/internal errors, and unexpected device loss are classified as fatal renderer errors with structured diagnostics.
- The window is hidden during GPU initialization and shown after configuration. The first dirty redraw temporarily uses `ControlFlow::Poll`, then returns to `Wait` immediately after presentation.

## CI acceptance

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-08-31 | `b731fdb0a9ddf1abcdac6d440644ec4be56d3756` | `macos-latest` | Pass | <https://github.com/adoom2017/3d-pet/actions/runs/33351372856> |
| 2026-08-31 | `b731fdb0a9ddf1abcdac6d440644ec4be56d3756` | `windows-latest` | Pass | <https://github.com/adoom2017/3d-pet/actions/runs/33351372856> |

The macOS runtime/visual acceptance, deterministic renderer tests, local gates, and both CI jobs passed. Phase 2 is `Done`; Phase 3 is `Ready`.
