//! Platform-neutral pointer state in explicit desktop and window coordinate spaces.

use winit::keyboard::ModifiersState;

use crate::display::{DesktopPosition, DisplayManager, PhysicalSize};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct MouseState {
    pub desktop_position: Option<DesktopPosition>,
    pub window_logical_position: Option<[f64; 2]>,
    pub left_pressed: bool,
    pub modifiers: ModifiersState,
}

impl MouseState {
    pub fn update_cursor_desktop(
        &mut self,
        desktop: DesktopPosition,
        window_origin: DesktopPosition,
    ) {
        if !desktop.is_finite() {
            self.clear_cursor();
            return;
        }
        self.desktop_position = Some(desktop);
        self.window_logical_position =
            DisplayManager::desktop_to_window_logical(desktop, window_origin);
    }

    pub fn update_cursor_physical(
        &mut self,
        physical: [f64; 2],
        window_origin: DesktopPosition,
        scale_factor: f64,
        viewport: PhysicalSize,
    ) {
        let Some(logical) = DisplayManager::window_physical_to_logical(physical, scale_factor)
        else {
            self.clear_cursor();
            return;
        };
        let Some(normalized_physical) =
            DisplayManager::window_logical_to_physical(logical, scale_factor)
        else {
            self.clear_cursor();
            return;
        };
        if DisplayManager::physical_to_ndc(normalized_physical, viewport).is_none() {
            self.clear_cursor();
            return;
        }
        let desktop =
            DesktopPosition::new(window_origin.x + logical[0], window_origin.y + logical[1]);
        if !desktop.is_finite() {
            self.clear_cursor();
            return;
        }
        self.desktop_position = Some(desktop);
        self.window_logical_position =
            DisplayManager::desktop_to_window_logical(desktop, window_origin);
    }

    pub fn clear_cursor(&mut self) {
        self.desktop_position = None;
        self.window_logical_position = None;
    }

    pub fn update_window_origin(&mut self, window_origin: DesktopPosition) {
        self.window_logical_position = self
            .desktop_position
            .and_then(|desktop| DisplayManager::desktop_to_window_logical(desktop, window_origin));
    }

    pub fn set_left_pressed(&mut self, pressed: bool) {
        self.left_pressed = pressed;
    }

    pub fn set_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_update_records_matching_desktop_and_window_positions() {
        let mut state = MouseState::default();
        state.update_cursor_physical(
            [160.0, 320.0],
            DesktopPosition::new(-500.0, 40.0),
            2.0,
            PhysicalSize::new(640, 640),
        );

        assert_eq!(state.window_logical_position, Some([80.0, 160.0]));
        assert_eq!(
            state.desktop_position,
            Some(DesktopPosition::new(-420.0, 200.0))
        );
    }

    #[test]
    fn outside_zero_sized_and_non_finite_cursor_updates_are_misses() {
        for (point, viewport) in [
            ([-1.0, 20.0], PhysicalSize::new(640, 640)),
            ([641.0, 20.0], PhysicalSize::new(640, 640)),
            ([20.0, f64::NAN], PhysicalSize::new(640, 640)),
            ([20.0, 20.0], PhysicalSize::new(0, 640)),
        ] {
            let mut state = MouseState {
                desktop_position: Some(DesktopPosition::new(1.0, 1.0)),
                window_logical_position: Some([1.0, 1.0]),
                ..MouseState::default()
            };
            state.update_cursor_physical(point, DesktopPosition::default(), 2.0, viewport);
            assert_eq!(state.desktop_position, None);
            assert_eq!(state.window_logical_position, None);
        }
    }

    #[test]
    fn button_and_modifier_state_are_explicit() {
        let mut state = MouseState::default();
        state.set_left_pressed(true);
        state.set_modifiers(ModifiersState::SHIFT | ModifiersState::CONTROL);
        assert!(state.left_pressed);
        assert!(state.modifiers.shift_key());
        assert!(state.modifiers.control_key());
        state.clear_cursor();
        assert!(
            state.left_pressed,
            "leaving the window does not synthesize release"
        );
    }

    #[test]
    fn moving_window_preserves_desktop_cursor_and_updates_local_position() {
        let mut state = MouseState {
            desktop_position: Some(DesktopPosition::new(-80.0, 80.0)),
            window_logical_position: Some([20.0, 30.0]),
            ..MouseState::default()
        };
        state.update_window_origin(DesktopPosition::new(-110.0, 60.0));
        assert_eq!(state.window_logical_position, Some([30.0, 20.0]));
        assert_eq!(
            state.desktop_position,
            Some(DesktopPosition::new(-80.0, 80.0))
        );
    }

    #[test]
    fn global_cursor_update_supports_points_outside_the_window() {
        let mut state = MouseState::default();
        state.update_cursor_desktop(
            DesktopPosition::new(-40.0, 500.0),
            DesktopPosition::new(10.0, 300.0),
        );
        assert_eq!(state.window_logical_position, Some([-50.0, 200.0]));
        assert_eq!(
            state.desktop_position,
            Some(DesktopPosition::new(-40.0, 500.0))
        );
    }
}
