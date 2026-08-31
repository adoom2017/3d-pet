# Dependency compatibility baseline

Baseline selected on 2026-08-30 for DesktopPet Phase 0.

## Toolchain

| Component | Selected version | Notes |
| --- | --- | --- |
| Rust | stable; verified with 1.96.0 | Local macOS development and CI use the stable channel |
| Cargo | 1.96.0 | Used to generate and verify `Cargo.lock` |
| Minimum Rust version | 1.87 | Set by the highest declared MSRV among Phase 0 dependencies |

## Direct dependencies

| Crate | Selected version | Purpose | Compatibility basis |
| --- | --- | --- | --- |
| winit | 0.30.13 | Window and event loop foundation for Phase 1 | Stable release, Rust 1.70 MSRV, raw-window-handle 0.6 support |
| wgpu | 30.0.1 | GPU foundation for Phase 2 | Stable release, Rust 1.87 MSRV, Metal and Direct3D 12 features enabled |
| anyhow | 1.0.104 | Application-boundary error context | Stable 1.x API |
| thiserror | 2.0.20 | Typed module errors | Stable 2.x derive API |
| tracing | 0.1.44 | Structured diagnostics | Stable 0.1 API |
| tracing-subscriber | 0.3.23 | Formatted tracing subscriber | Stable 0.3 API |
| serde | 1.0.229 | Configuration serialization | Derive feature enabled |
| serde_json | 1.0.151 | JSON configuration parsing | Matches serde 1.x |

## Compatibility decisions

- winit 0.31.0-beta.2 was available but rejected because MVP dependencies must use stable releases.
- winit enables only raw-window-handle 0.6 support; Linux X11 and Wayland features are intentionally omitted because the MVP targets Windows and macOS.
- wgpu enables `dx12`, `metal`, `std`, and `wgsl` without unrelated default backends. This matches the Windows/macOS MVP and the planned WGSL shader pipeline.
- winit and wgpu do not directly depend on each other. Their shared window-handle boundary is raw-window-handle 0.6, which is verified by the resolved dependency graph and dual-platform CI.
- Exact transitive versions are owned by the committed `Cargo.lock`; direct compatible-version requirements remain in the workspace manifest.

## Known platform limits

- Headless CI proves compilation and pure logic, not transparent desktop composition or native input behavior.
- Since 2026-08-31, macOS owns runtime, visual, interaction, and performance acceptance. Windows must pass compilation, Clippy, tests, and build in CI; no Windows real-machine gate is required.
- Platform-native dependencies for windows-rs and objc2 are deferred until their first implementation phase so Phase 0 does not lock unused native APIs.

Update this file whenever a direct dependency line, Rust MSRV, backend feature, or known platform limitation changes.
