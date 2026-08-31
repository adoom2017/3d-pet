# DesktopPet

DesktopPet is a lightweight 3D desktop pet for macOS with Windows compile-and-test support. Phase 2's wgpu renderer now presents a colored triangle over a truly transparent window, and Phase 3 is ready to add the first static GLB pet. macOS is the runtime, visual, and interaction reference platform; Windows is continuously checked by CI.

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

The application opens a persistent 320 x 320 transparent window. Close the window normally or press Escape to exit through the clean shutdown path.

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
