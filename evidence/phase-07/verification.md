# Phase 7 verification

Status: `Verifying`

Implementation commit: pending

## Implemented scope

- `WanderingPetBrain` receives only `PetObservation`, injected monotonic time, and `RandomSource`; it has no platform, input, asset, animation, or renderer dependency.
- `BrainConfig` centralizes validated Idle and Walk duration ranges. A seeded `SplitMix64` source provides deterministic direction and duration decisions.
- `BehaviorStateMachine` owns Idle, Turning, and Walking transitions. A direction change spends exactly 250 ms in Turning before Walking; a same-direction decision walks directly.
- `TransitionContext` enforces the priority order for Brain, explicit interaction, physics, and drag. Invalid, unsupported, and suppressed intents return explicit rejection outcomes.
- The application advances a simulation clock at the fixed 60 Hz logic rate and dispatches state-machine animation, facing, and movement commands at the composition root.

## Automated gates

| Command | Exit code | Result | Key output |
| --- | --- | --- | --- |
| `cargo fmt --check` | 0 | Pass | No formatting differences |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | Pass | No issues found |
| `cargo test --workspace` | 0 | Pass | 44 tests passed across 4 suites |
| `cargo build --workspace` | 0 | Pass | Dev profile completed |

Tests cover exact Turn timing, invalid and unsupported intents, high-priority suppression, invalid duration ranges, probability endpoints, simulation-clock injection, an exact eight-intent fixed-seed sequence, replay equality, movement mocks, render-rate independence, animation transitions, and renderer smoke tests.

## CI acceptance

Pending the implementation push. Windows acceptance for this phase is limited to running the same deterministic tests plus CI format, Clippy, and build; Windows runtime verification is not required.

## macOS acceptance

The repository binary was launched through temporary macOS application bundles. A 20 ms CoreGraphics poll observed 12.3 seconds of autonomous behavior without sending input. The sequence completed four Idle/Walk cycles and three direction changes through Turning; `macos-observation.log` records every state-title change and window X position.

Visual inspection confirms a coherent Idle pose and a distinct animated Walk pose while the window moves. Facing and X movement agree, cross-fades remain smooth, and no state became stuck. Window-ID captures are stored in `screenshots/macos-autonomous-idle.png` and `screenshots/macos-autonomous-walking.png`; transparent pixels appear black in isolated window captures but remained transparent in live desktop composition.

Work-area clamping is intentionally deferred to Phase 8, so the long observation was allowed to move beyond the current display without being treated as a Phase 7 failure.

The macOS autonomous behavior gate passed. Phase 7 remains `Verifying` until the final local gates and macOS/Windows CI jobs pass.
