//! Desktop coordinate values and conversion boundary.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DesktopPosition {
    pub x: f64,
    pub y: f64,
}

impl DesktopPosition {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn rounded(self) -> Self {
        Self::new(self.x.round(), self.y.round())
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_position_rounding_is_consistent_for_negative_coordinates() {
        assert_eq!(
            DesktopPosition::new(-10.6, 20.5).rounded(),
            DesktopPosition::new(-11.0, 21.0)
        );
    }
}
