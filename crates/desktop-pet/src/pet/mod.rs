//! Pet state, behavior, movement, and physics boundary.

use std::time::Duration;

use crate::display::DesktopPosition;

pub(crate) const DEFAULT_WALK_SPEED_LOGICAL_PX_PER_S: f64 = 80.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HorizontalDirection {
    Left,
    Right,
}

impl HorizontalDirection {
    const fn sign(self) -> f64 {
        match self {
            Self::Left => -1.0,
            Self::Right => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PhysicsBody {
    pub position: DesktopPosition,
    pub velocity_logical_px_per_s: [f64; 2],
    pub gravity_logical_px_per_s2: f64,
    pub grounded: bool,
}

impl PhysicsBody {
    fn proposed_position(self, delta: Duration) -> DesktopPosition {
        let seconds = delta.as_secs_f64();
        DesktopPosition::new(
            self.position.x + self.velocity_logical_px_per_s[0] * seconds,
            self.position.y + self.velocity_logical_px_per_s[1] * seconds,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MovementState {
    Idle,
    Walking(HorizontalDirection),
}

pub(crate) struct MovementController {
    body: PhysicsBody,
    state: MovementState,
}

impl MovementController {
    pub fn new(position: DesktopPosition) -> Self {
        Self {
            body: PhysicsBody {
                position,
                velocity_logical_px_per_s: [0.0, 0.0],
                gravity_logical_px_per_s2: 0.0,
                grounded: true,
            },
            state: MovementState::Idle,
        }
    }

    pub fn state(&self) -> MovementState {
        self.state
    }

    pub fn start_walking(&mut self, direction: HorizontalDirection) {
        self.state = MovementState::Walking(direction);
        self.body.velocity_logical_px_per_s[0] =
            direction.sign() * DEFAULT_WALK_SPEED_LOGICAL_PX_PER_S;
    }

    pub fn stop(&mut self) {
        self.state = MovementState::Idle;
        self.body.velocity_logical_px_per_s[0] = 0.0;
    }

    fn proposed_position(&self, delta: Duration) -> DesktopPosition {
        self.body.proposed_position(delta)
    }

    fn confirm_position(&mut self, position: DesktopPosition) {
        self.body.position = position;
    }

    pub fn try_advance<E>(
        &mut self,
        delta: Duration,
        mut set_platform_position: impl FnMut(DesktopPosition) -> Result<(), E>,
    ) -> Result<bool, E> {
        if !matches!(self.state, MovementState::Walking(_)) {
            return Ok(false);
        }
        let next = self.proposed_position(delta);
        set_platform_position(next)?;
        self.confirm_position(next);
        Ok(true)
    }

    #[cfg(test)]
    fn position(&self) -> DesktopPosition {
        self.body.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{FIXED_UPDATE_INTERVAL, FixedStepAccumulator};

    #[test]
    fn walking_uses_logical_pixels_per_second() {
        let mut movement = MovementController::new(DesktopPosition::new(10.0, -20.0));
        movement.start_walking(HorizontalDirection::Right);
        let next = movement.proposed_position(Duration::from_millis(500));
        assert_eq!(next, DesktopPosition::new(50.0, -20.0));
        movement.confirm_position(next);
        movement.start_walking(HorizontalDirection::Left);
        assert_eq!(
            movement.proposed_position(Duration::from_millis(250)),
            DesktopPosition::new(30.0, -20.0)
        );
    }

    #[test]
    fn failed_platform_move_can_preserve_last_confirmed_position() {
        let mut movement = MovementController::new(DesktopPosition::new(5.0, 7.0));
        movement.start_walking(HorizontalDirection::Right);
        let error = movement
            .try_advance(Duration::from_secs(1), |_position| Err("mock failure"))
            .expect_err("mock platform must reject the move");
        assert_eq!(error, "mock failure");
        assert_eq!(movement.position(), DesktopPosition::new(5.0, 7.0));
    }

    #[test]
    fn successful_platform_move_commits_the_proposed_position() {
        let mut movement = MovementController::new(DesktopPosition::new(5.0, 7.0));
        movement.start_walking(HorizontalDirection::Right);
        let mut received = None;
        assert_eq!(
            movement.try_advance(Duration::from_secs(1), |position| {
                received = Some(position);
                Ok::<_, ()>(())
            }),
            Ok(true)
        );
        assert_eq!(received, Some(DesktopPosition::new(85.0, 7.0)));
        assert_eq!(movement.position(), DesktopPosition::new(85.0, 7.0));
    }

    #[test]
    fn fixed_updates_are_independent_of_render_rate() {
        let positions = [15_u32, 30, 60, 120].map(simulate_two_seconds);
        for position in positions {
            assert!((position - 160.0).abs() < 0.001, "position={position}");
        }
    }

    fn simulate_two_seconds(render_fps: u32) -> f64 {
        let mut movement = MovementController::new(DesktopPosition::default());
        movement.start_walking(HorizontalDirection::Right);
        let mut fixed_steps = FixedStepAccumulator::default();
        let frame_count = render_fps * 2;
        for frame in 1..=frame_count {
            let elapsed = Duration::from_secs_f64(frame as f64 / render_fps as f64);
            let previous = Duration::from_secs_f64((frame - 1) as f64 / render_fps as f64);
            for _ in 0..fixed_steps.push(elapsed - previous).steps {
                let next = movement.proposed_position(FIXED_UPDATE_INTERVAL);
                movement.confirm_position(next);
            }
        }
        movement.position().x
    }
}
