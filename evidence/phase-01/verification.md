# Phase 1 verification

Status: `Verifying`

## Environment

| Date | Commit | Platform | Result | Evidence |
| --- | --- | --- | --- | --- |
| 2026-08-31 | Phase 1 implementation commit pending | macOS 26.6.2 (25G83), arm64, Retina 2x | Pass | This document and `screenshots/macos-window-alpha.png` |

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 7 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |

The tests assert the 320 x 320 logical size, transparent flag, disabled decorations, non-resizable sizing constraints, always-on-top level, and configuration mapping.

## macOS acceptance

The application was launched with `cargo run -p desktop-pet` on 2026-08-31.

Runtime creation record:

```text
desktop pet window created window_id=WindowId(50044011136) logical_width=320.0 logical_height=320.0 physical_width=640 physical_height=640 scale_factor=2.0 transparent=true decorations=false resizable=false always_on_top=true
```

CoreGraphics reported the `DesktopPet` window at logical bounds 320 x 320 and layer 3, confirming the floating window level. A window-only capture produced a 640 x 640 Retina PNG with RGBA channels. Pixel inspection reported:

```text
size=640x640 min_alpha=0.0 max_alpha=0.0 nonzero_alpha_pixels=0
```

The capture contains no opaque pixels or shadow. The application received a synthetic Escape key event and emitted both the exit-request and clean-shutdown INFO records. macOS printed an Input Method Kit Mach-port diagnostic while the synthetic key was delivered; it came from the OS automation path, not DesktopPet tracing, and did not affect shutdown.

## CI and Windows acceptance

- The Phase 1 commit must pass the GitHub Actions macOS/Windows matrix.
- Windows real-machine acceptance remains required for transparent composition, borderless/non-resizable behavior, and always-on-top level.
- Phase 1 remains `Verifying`; Phase 2 remains `Blocked` until both items pass and this record contains their evidence.
