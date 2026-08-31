# DesktopPet

DesktopPet is a lightweight 3D desktop pet for macOS with Windows compile-and-test support. The current Phase 7 build gives an animated Quaternius Fox deterministic autonomous Idle, Turn, and Walk behavior backed by fixed-timestep movement, semantic animation cross-fades, directional facing, and a truly transparent window. macOS is the runtime, visual, and interaction reference platform; Windows is continuously checked by CI.

## Prerequisites

- macOS or Windows
- Rust stable with Cargo, rustfmt, and Clippy
- Git

The repository includes `rust-toolchain.toml`, so rustup selects the expected stable toolchain and components automatically.

## Build and run

```bash
cargo build --workspace
cargo run -p desktop-pet
```

The application opens a persistent 320 x 320 transparent window containing the animated default Fox, which autonomously alternates between Idle and directional Walk cycles. Press Left or Right for an explicit walk command, or Space to toggle rightward walking and Idle. Close the window normally or press Escape to exit through the clean shutdown path. Desktop boundary handling is introduced in Phase 8, so the Phase 7 build can intentionally move beyond the visible work area.

## Verification

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

The same commands run on macOS and Windows for every push and pull request.

## Project documents

- [Development plan](DEVELOPMENT_PLAN.md): product goals, MVP scope, roadmap, and release criteria.
- [Architecture](ARCHITECTURE.md): module contracts, ownership, data flow, and platform boundaries.
- [Tasks](TASKS.md): sequential Phase 0-14 implementation and verification gates.
- [Dependency compatibility](docs/dependency-compatibility.md): selected Rust and crate versions with compatibility notes.
