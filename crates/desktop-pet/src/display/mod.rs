//! Desktop coordinate values, monitor selection, and window constraints.

use crate::pet::HorizontalDirection;

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LogicalSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalSize {
    pub width: u32,
    pub height: u32,
}

impl PhysicalSize {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

impl LogicalSize {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    fn is_valid(self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width >= 0.0 && self.height >= 0.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct MonitorId(pub u64);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MonitorInfo {
    pub id: MonitorId,
    pub work_area_origin: DesktopPosition,
    pub work_area_size: LogicalSize,
    pub scale_factor: f64,
    pub is_primary: bool,
}

impl MonitorInfo {
    pub fn new(
        id: MonitorId,
        work_area_origin: DesktopPosition,
        work_area_size: LogicalSize,
        scale_factor: f64,
        is_primary: bool,
    ) -> Option<Self> {
        if !work_area_origin.is_finite()
            || !work_area_size.is_valid()
            || !scale_factor.is_finite()
            || scale_factor <= 0.0
        {
            return None;
        }
        Some(Self {
            id,
            work_area_origin,
            work_area_size,
            scale_factor,
            is_primary,
        })
    }

    #[cfg(any(target_os = "windows", test))]
    pub fn from_physical_work_area(
        id: MonitorId,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        scale_factor: f64,
        is_primary: bool,
    ) -> Option<Self> {
        let width = right.checked_sub(left)?;
        let height = bottom.checked_sub(top)?;
        Self::new(
            id,
            DesktopPosition::new(left as f64 / scale_factor, top as f64 / scale_factor),
            LogicalSize::new(width as f64 / scale_factor, height as f64 / scale_factor),
            scale_factor,
            is_primary,
        )
    }

    fn right(&self) -> f64 {
        self.work_area_origin.x + self.work_area_size.width
    }

    fn bottom(&self) -> f64 {
        self.work_area_origin.y + self.work_area_size.height
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoundaryResult {
    pub position: DesktopPosition,
    pub turn: Option<HorizontalDirection>,
}

impl BoundaryResult {
    const fn unconstrained(position: DesktopPosition) -> Self {
        Self {
            position,
            turn: None,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DisplayManager {
    monitors: Vec<MonitorInfo>,
}

impl DisplayManager {
    pub fn new(monitors: Vec<MonitorInfo>) -> Self {
        Self { monitors }
    }

    pub fn refresh(&mut self, monitors: Vec<MonitorInfo>) -> bool {
        if self.monitors == monitors {
            return false;
        }
        self.monitors = monitors;
        true
    }

    pub fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    pub fn desktop_to_window_logical(
        desktop: DesktopPosition,
        window_origin: DesktopPosition,
    ) -> Option<[f64; 2]> {
        if !desktop.is_finite() || !window_origin.is_finite() {
            return None;
        }
        Some([desktop.x - window_origin.x, desktop.y - window_origin.y])
    }

    pub fn window_logical_to_physical(logical: [f64; 2], scale_factor: f64) -> Option<[f64; 2]> {
        if !logical.into_iter().all(f64::is_finite)
            || !scale_factor.is_finite()
            || scale_factor <= 0.0
        {
            return None;
        }
        Some([logical[0] * scale_factor, logical[1] * scale_factor])
    }

    pub fn window_physical_to_logical(physical: [f64; 2], scale_factor: f64) -> Option<[f64; 2]> {
        if !physical.into_iter().all(f64::is_finite)
            || !scale_factor.is_finite()
            || scale_factor <= 0.0
        {
            return None;
        }
        Some([physical[0] / scale_factor, physical[1] / scale_factor])
    }

    pub fn physical_to_ndc(physical: [f64; 2], viewport: PhysicalSize) -> Option<[f64; 2]> {
        if viewport.width == 0
            || viewport.height == 0
            || !physical.into_iter().all(f64::is_finite)
            || physical[0] < 0.0
            || physical[1] < 0.0
            || physical[0] > f64::from(viewport.width)
            || physical[1] > f64::from(viewport.height)
        {
            return None;
        }
        Some([
            2.0 * physical[0] / f64::from(viewport.width) - 1.0,
            1.0 - 2.0 * physical[1] / f64::from(viewport.height),
        ])
    }

    pub fn active_monitor(
        &self,
        window_position: DesktopPosition,
        window_size: LogicalSize,
    ) -> Option<&MonitorInfo> {
        if !window_position.is_finite() || !window_size.is_valid() {
            return self.primary_monitor();
        }
        let center = DesktopPosition::new(
            window_position.x + window_size.width / 2.0,
            window_position.y + window_size.height / 2.0,
        );
        self.monitors
            .iter()
            .find(|monitor| contains_point(monitor, center))
            .or_else(|| {
                self.monitors
                    .iter()
                    .map(|monitor| {
                        (
                            monitor,
                            intersection_area(window_position, window_size, monitor),
                        )
                    })
                    .filter(|(_, area)| *area > 0.0)
                    .max_by(|(_, left), (_, right)| left.total_cmp(right))
                    .map(|(monitor, _)| monitor)
            })
            .or_else(|| self.primary_monitor())
    }

    pub fn constrain_position(
        &self,
        position: DesktopPosition,
        window_size: LogicalSize,
    ) -> DesktopPosition {
        let Some(active) = self.active_monitor(position, window_size) else {
            return position;
        };
        let maximum_x = (active.right() - window_size.width).max(active.work_area_origin.x);
        let maximum_y = (active.bottom() - window_size.height).max(active.work_area_origin.y);
        DesktopPosition::new(
            position.x.clamp(active.work_area_origin.x, maximum_x),
            position.y.clamp(active.work_area_origin.y, maximum_y),
        )
    }

    pub fn ground_y(
        &self,
        window_position: DesktopPosition,
        window_size: LogicalSize,
    ) -> Option<f64> {
        let active = self.active_monitor(window_position, window_size)?;
        Some((active.bottom() - window_size.height).max(active.work_area_origin.y))
    }

    pub fn constrain_horizontal_move(
        &self,
        current: DesktopPosition,
        proposed: DesktopPosition,
        window_size: LogicalSize,
        direction: HorizontalDirection,
    ) -> BoundaryResult {
        if self.monitors.is_empty()
            || !current.is_finite()
            || !proposed.is_finite()
            || !window_size.is_valid()
        {
            return BoundaryResult::unconstrained(proposed);
        }
        let Some(active) = self.active_monitor(current, window_size) else {
            return BoundaryResult::unconstrained(proposed);
        };

        let crossed_horizontal_edge = match direction {
            HorizontalDirection::Left => proposed.x < active.work_area_origin.x,
            HorizontalDirection::Right => proposed.x + window_size.width > active.right(),
        };
        if crossed_horizontal_edge
            && self.monitors.iter().any(|candidate| {
                candidate.id != active.id
                    && is_in_direction(candidate, active, direction)
                    && intersection_area(proposed, window_size, candidate) > 0.0
            })
        {
            return BoundaryResult::unconstrained(proposed);
        }

        let position = self.constrain_position(proposed, window_size);
        let turn = match direction {
            HorizontalDirection::Left if proposed.x < active.work_area_origin.x => {
                Some(HorizontalDirection::Right)
            }
            HorizontalDirection::Right if proposed.x + window_size.width > active.right() => {
                Some(HorizontalDirection::Left)
            }
            _ => None,
        };
        BoundaryResult { position, turn }
    }

    fn primary_monitor(&self) -> Option<&MonitorInfo> {
        self.monitors
            .iter()
            .find(|monitor| monitor.is_primary)
            .or_else(|| self.monitors.first())
    }
}

fn contains_point(monitor: &MonitorInfo, point: DesktopPosition) -> bool {
    point.x >= monitor.work_area_origin.x
        && point.x < monitor.right()
        && point.y >= monitor.work_area_origin.y
        && point.y < monitor.bottom()
}

fn intersection_area(position: DesktopPosition, size: LogicalSize, monitor: &MonitorInfo) -> f64 {
    let width =
        (position.x + size.width).min(monitor.right()) - position.x.max(monitor.work_area_origin.x);
    let height = (position.y + size.height).min(monitor.bottom())
        - position.y.max(monitor.work_area_origin.y);
    width.max(0.0) * height.max(0.0)
}

fn is_in_direction(
    candidate: &MonitorInfo,
    active: &MonitorInfo,
    direction: HorizontalDirection,
) -> bool {
    match direction {
        HorizontalDirection::Left => candidate.work_area_origin.x < active.work_area_origin.x,
        HorizontalDirection::Right => candidate.right() > active.right(),
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

    #[test]
    fn ground_y_uses_the_active_monitor_work_area() {
        let manager = DisplayManager::new(vec![
            monitor(1, 0.0, 20.0, 1_000.0, 780.0, 1.0, true),
            monitor(2, 1_000.0, -100.0, 1_200.0, 900.0, 2.0, false),
        ]);
        let size = LogicalSize::new(320.0, 320.0);
        assert_eq!(
            manager.ground_y(DesktopPosition::new(100.0, 100.0), size),
            Some(480.0)
        );
        assert_eq!(
            manager.ground_y(DesktopPosition::new(1_200.0, -50.0), size),
            Some(480.0)
        );
    }

    #[test]
    fn ground_y_handles_a_work_area_smaller_than_the_window() {
        let manager =
            DisplayManager::new(vec![monitor(1, -500.0, -200.0, 200.0, 100.0, 1.5, true)]);
        assert_eq!(
            manager.ground_y(
                DesktopPosition::new(-500.0, -200.0),
                LogicalSize::new(320.0, 320.0)
            ),
            Some(-200.0)
        );
    }

    fn monitor(
        id: u64,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        scale: f64,
        primary: bool,
    ) -> MonitorInfo {
        MonitorInfo::new(
            MonitorId(id),
            DesktopPosition::new(x, y),
            LogicalSize::new(width, height),
            scale,
            primary,
        )
        .expect("valid monitor fixture")
    }

    #[test]
    fn monitor_validation_rejects_invalid_snapshots() {
        assert!(
            MonitorInfo::new(
                MonitorId(1),
                DesktopPosition::new(0.0, 0.0),
                LogicalSize::new(1920.0, 1080.0),
                0.0,
                true,
            )
            .is_none()
        );
        assert!(
            MonitorInfo::new(
                MonitorId(1),
                DesktopPosition::new(f64::NAN, 0.0),
                LogicalSize::new(1920.0, 1080.0),
                2.0,
                true,
            )
            .is_none()
        );
    }

    #[test]
    fn physical_work_areas_convert_to_logical_coordinates_at_common_dpi_scales() {
        let cases = [
            (1.25, (-1920, 0, 0, 1080), (-1536.0, 0.0, 1536.0, 864.0)),
            (2.0, (0, -2160, 2560, 0), (0.0, -1080.0, 1280.0, 1080.0)),
            (2.0, (0, 48, 3024, 1890), (0.0, 24.0, 1512.0, 921.0)),
        ];
        for (scale, physical, logical) in cases {
            let converted = MonitorInfo::from_physical_work_area(
                MonitorId(1),
                physical.0,
                physical.1,
                physical.2,
                physical.3,
                scale,
                true,
            )
            .expect("valid physical work area");
            assert_eq!(
                converted.work_area_origin,
                DesktopPosition::new(logical.0, logical.1)
            );
            assert_eq!(
                converted.work_area_size,
                LogicalSize::new(logical.2, logical.3)
            );
        }
    }

    #[test]
    fn coordinate_pipeline_handles_negative_desktop_origin_and_retina_scale() {
        let desktop = DesktopPosition::new(-1234.5, 220.25);
        let window = DesktopPosition::new(-1280.0, 160.0);
        let logical = DisplayManager::desktop_to_window_logical(desktop, window)
            .expect("finite desktop coordinates");
        let physical =
            DisplayManager::window_logical_to_physical(logical, 2.0).expect("valid Retina scale");
        let ndc = DisplayManager::physical_to_ndc(physical, PhysicalSize::new(640, 640))
            .expect("point inside viewport");

        assert_eq!(logical, [45.5, 60.25]);
        assert_eq!(physical, [91.0, 120.5]);
        assert_eq!(ndc, [-0.715625, 0.6234375]);
        assert_eq!(
            DisplayManager::window_physical_to_logical(physical, 2.0),
            Some(logical)
        );
    }

    #[test]
    fn ndc_conversion_includes_edges_and_rejects_invalid_viewports_and_points() {
        let viewport = PhysicalSize::new(400, 200);
        assert_eq!(
            DisplayManager::physical_to_ndc([0.0, 0.0], viewport),
            Some([-1.0, 1.0])
        );
        assert_eq!(
            DisplayManager::physical_to_ndc([400.0, 200.0], viewport),
            Some([1.0, -1.0])
        );
        for (point, size) in [
            ([-0.1, 20.0], viewport),
            ([401.0, 20.0], viewport),
            ([f64::NAN, 20.0], viewport),
            ([1.0, 1.0], PhysicalSize::new(0, 200)),
        ] {
            assert_eq!(DisplayManager::physical_to_ndc(point, size), None);
        }
    }

    #[test]
    fn active_monitor_prefers_center_then_intersection_then_primary() {
        let manager = DisplayManager::new(vec![
            monitor(1, 0.0, 0.0, 1000.0, 800.0, 2.0, true),
            monitor(2, 1000.0, 0.0, 1000.0, 800.0, 1.25, false),
        ]);
        let size = LogicalSize::new(320.0, 320.0);

        assert_eq!(
            manager
                .active_monitor(DesktopPosition::new(850.0, 100.0), size)
                .map(|monitor| monitor.id),
            Some(MonitorId(2))
        );
        assert_eq!(
            manager
                .active_monitor(DesktopPosition::new(900.0, 700.0), size)
                .map(|monitor| monitor.id),
            Some(MonitorId(2))
        );
        assert_eq!(
            manager
                .active_monitor(DesktopPosition::new(3000.0, 3000.0), size)
                .map(|monitor| monitor.id),
            Some(MonitorId(1))
        );
    }

    #[test]
    fn monitor_layout_table_handles_negative_stacked_and_mixed_dpi_screens() {
        struct Case {
            name: &'static str,
            monitors: Vec<MonitorInfo>,
            position: DesktopPosition,
            expected: MonitorId,
        }
        let cases = [
            Case {
                name: "single retina",
                monitors: vec![monitor(1, 0.0, 0.0, 1512.0, 945.0, 2.0, true)],
                position: DesktopPosition::new(100.0, 100.0),
                expected: MonitorId(1),
            },
            Case {
                name: "negative left at 125 percent",
                monitors: vec![
                    monitor(1, 0.0, 0.0, 1920.0, 1040.0, 2.0, true),
                    monitor(2, -1536.0, 0.0, 1536.0, 824.0, 1.25, false),
                ],
                position: DesktopPosition::new(-1000.0, 100.0),
                expected: MonitorId(2),
            },
            Case {
                name: "stacked screen at 200 percent",
                monitors: vec![
                    monitor(1, 0.0, 0.0, 1920.0, 1040.0, 1.0, true),
                    monitor(2, 200.0, -1080.0, 1280.0, 1080.0, 2.0, false),
                ],
                position: DesktopPosition::new(500.0, -800.0),
                expected: MonitorId(2),
            },
        ];
        let size = LogicalSize::new(320.0, 320.0);
        for case in cases {
            let manager = DisplayManager::new(case.monitors);
            assert_eq!(
                manager
                    .active_monitor(case.position, size)
                    .map(|monitor| monitor.id),
                Some(case.expected),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn boundary_clamps_to_work_area_and_requests_one_reverse_turn() {
        let manager = DisplayManager::new(vec![monitor(1, 0.0, 24.0, 1440.0, 846.0, 2.0, true)]);
        let result = manager.constrain_horizontal_move(
            DesktopPosition::new(1119.0, 700.0),
            DesktopPosition::new(1121.0, 900.0),
            LogicalSize::new(320.0, 320.0),
            HorizontalDirection::Right,
        );

        assert_eq!(result.position, DesktopPosition::new(1120.0, 550.0));
        assert_eq!(result.turn, Some(HorizontalDirection::Left));
    }

    #[test]
    fn adjacent_monitor_overlap_allows_cross_screen_movement() {
        let manager = DisplayManager::new(vec![
            monitor(1, 0.0, 0.0, 1000.0, 800.0, 2.0, true),
            monitor(2, 1000.0, 0.0, 800.0, 700.0, 1.25, false),
        ]);
        let proposed = DesktopPosition::new(681.0, 100.0);
        let result = manager.constrain_horizontal_move(
            DesktopPosition::new(680.0, 100.0),
            proposed,
            LogicalSize::new(320.0, 320.0),
            HorizontalDirection::Right,
        );

        assert_eq!(result, BoundaryResult::unconstrained(proposed));
    }

    #[test]
    fn empty_monitor_snapshot_is_a_noop_and_can_be_refreshed() {
        let mut manager = DisplayManager::default();
        let proposed = DesktopPosition::new(-123.0, 456.0);
        assert_eq!(
            manager.constrain_horizontal_move(
                DesktopPosition::default(),
                proposed,
                LogicalSize::new(320.0, 320.0),
                HorizontalDirection::Left,
            ),
            BoundaryResult::unconstrained(proposed)
        );
        assert!(manager.refresh(vec![monitor(7, 0.0, 0.0, 800.0, 600.0, 1.0, true)]));
        assert!(!manager.refresh(vec![monitor(7, 0.0, 0.0, 800.0, 600.0, 1.0, true)]));
        assert_eq!(manager.monitors().len(), 1);
    }
}
