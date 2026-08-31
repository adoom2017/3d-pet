//! Pointer hit testing and drag gestures in explicit desktop coordinates.

use std::{collections::VecDeque, time::Duration};

use glam::{Mat4, Vec3, Vec4};

use crate::{display::DesktopPosition, render::PetProjection};

pub(crate) const DEFAULT_HIT_PADDING_LOGICAL: f64 = 6.0;
const DRAG_THRESHOLD_LOGICAL: f64 = 5.0;
const RELEASE_SAMPLE_WINDOW: Duration = Duration::from_millis(120);
const MAX_RELEASE_SAMPLES: usize = 8;
const MAX_RELEASE_SPEED_LOGICAL_PX_PER_S: f64 = 3_000.0;

pub(crate) trait HitRegion {
    fn contains(&self, window_logical_position: [f64; 2]) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RectHitRegion {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl RectHitRegion {
    pub fn new(min: [f64; 2], max: [f64; 2]) -> Option<Self> {
        if !min.into_iter().chain(max).all(f64::is_finite) || min[0] > max[0] || min[1] > max[1] {
            return None;
        }
        Some(Self { min, max })
    }

    pub fn from_pet_projection(projection: PetProjection, padding_logical: f64) -> Option<Self> {
        if projection.viewport.width == 0
            || projection.viewport.height == 0
            || !projection.scale_factor.is_finite()
            || projection.scale_factor <= 0.0
            || !padding_logical.is_finite()
            || padding_logical < 0.0
            || !matrix_is_finite(projection.clip_from_model)
            || !vec3_is_finite(projection.bounds_min)
            || !vec3_is_finite(projection.bounds_max)
            || projection.bounds_min.cmpgt(projection.bounds_max).any()
        {
            return None;
        }

        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for x in [projection.bounds_min.x, projection.bounds_max.x] {
            for y in [projection.bounds_min.y, projection.bounds_max.y] {
                for z in [projection.bounds_min.z, projection.bounds_max.z] {
                    let clip = projection.clip_from_model * Vec4::new(x, y, z, 1.0);
                    if !vec4_is_finite(clip) || clip.w <= f32::EPSILON {
                        return None;
                    }
                    let ndc = clip.truncate() / clip.w;
                    let physical = [
                        f64::from((ndc.x + 1.0) * 0.5) * f64::from(projection.viewport.width),
                        f64::from((1.0 - ndc.y) * 0.5) * f64::from(projection.viewport.height),
                    ];
                    let logical = [
                        physical[0] / projection.scale_factor,
                        physical[1] / projection.scale_factor,
                    ];
                    min[0] = min[0].min(logical[0]);
                    min[1] = min[1].min(logical[1]);
                    max[0] = max[0].max(logical[0]);
                    max[1] = max[1].max(logical[1]);
                }
            }
        }

        let logical_viewport = [
            f64::from(projection.viewport.width) / projection.scale_factor,
            f64::from(projection.viewport.height) / projection.scale_factor,
        ];
        Self::new(
            [
                (min[0] - padding_logical).clamp(0.0, logical_viewport[0]),
                (min[1] - padding_logical).clamp(0.0, logical_viewport[1]),
            ],
            [
                (max[0] + padding_logical).clamp(0.0, logical_viewport[0]),
                (max[1] + padding_logical).clamp(0.0, logical_viewport[1]),
            ],
        )
    }
}

impl HitRegion for RectHitRegion {
    fn contains(&self, position: [f64; 2]) -> bool {
        position.into_iter().all(f64::is_finite)
            && position[0] >= self.min[0]
            && position[0] <= self.max[0]
            && position[1] >= self.min[1]
            && position[1] <= self.max[1]
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CameraRay {
    pub origin: Vec3,
    pub direction: Vec3,
}

#[allow(dead_code)]
impl CameraRay {
    pub fn from_ndc(ndc: [f64; 2], clip_from_world: Mat4) -> Option<Self> {
        if !ndc.into_iter().all(f64::is_finite)
            || !matrix_is_finite(clip_from_world)
            || clip_from_world.determinant().abs() <= f32::EPSILON
        {
            return None;
        }
        let world_from_clip = clip_from_world.inverse();
        let near =
            homogeneous_point(world_from_clip * Vec4::new(ndc[0] as f32, ndc[1] as f32, 0.0, 1.0))?;
        let far =
            homogeneous_point(world_from_clip * Vec4::new(ndc[0] as f32, ndc[1] as f32, 1.0, 1.0))?;
        let direction = (far - near).normalize_or_zero();
        if !vec3_is_finite(direction) || direction.length_squared() <= f32::EPSILON {
            return None;
        }
        Some(Self {
            origin: near,
            direction,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HitUpdate {
    pub hit: bool,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum InteractionAction {
    None,
    ClickPet,
    BeginDrag {
        offset: [f64; 2],
        desktop_position: DesktopPosition,
    },
    MoveDrag {
        desktop_position: DesktopPosition,
    },
    EndDrag {
        release_velocity: [f64; 2],
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum PointerState {
    #[default]
    Hovering,
    Pressed {
        start: DesktopPosition,
        offset: [f64; 2],
    },
    Dragged {
        offset: [f64; 2],
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DragSample {
    desktop_position: DesktopPosition,
    timestamp: Duration,
}

#[derive(Debug, Default)]
pub(crate) struct InteractionController {
    current_hit: bool,
    pointer_state: PointerState,
    drag_samples: VecDeque<DragSample>,
}

impl InteractionController {
    pub fn update_hit(
        &mut self,
        window_logical_position: Option<[f64; 2]>,
        region: Option<&dyn HitRegion>,
    ) -> HitUpdate {
        let hit = window_logical_position
            .zip(region)
            .is_some_and(|(position, region)| region.contains(position));
        let changed = hit != self.current_hit;
        self.current_hit = hit;
        HitUpdate { hit, changed }
    }

    #[cfg(test)]
    pub fn current_hit(&self) -> bool {
        self.current_hit
    }

    pub fn pointer_down(
        &mut self,
        desktop_position: Option<DesktopPosition>,
        window_origin: DesktopPosition,
        timestamp: Duration,
    ) -> InteractionAction {
        let Some(desktop_position) = desktop_position.filter(|position| position.is_finite())
        else {
            return InteractionAction::None;
        };
        if !matches!(self.pointer_state, PointerState::Hovering)
            || !self.current_hit
            || !window_origin.is_finite()
        {
            return InteractionAction::None;
        }
        let offset = [
            desktop_position.x - window_origin.x,
            desktop_position.y - window_origin.y,
        ];
        self.pointer_state = PointerState::Pressed {
            start: desktop_position,
            offset,
        };
        self.drag_samples.clear();
        self.record_drag_sample(desktop_position, timestamp);
        InteractionAction::None
    }

    pub fn pointer_moved(
        &mut self,
        desktop_position: Option<DesktopPosition>,
        timestamp: Duration,
    ) -> InteractionAction {
        let Some(desktop_position) = desktop_position.filter(|position| position.is_finite())
        else {
            return InteractionAction::None;
        };
        match self.pointer_state {
            PointerState::Hovering => InteractionAction::None,
            PointerState::Pressed { start, offset } => {
                let distance_squared =
                    (desktop_position.x - start.x).powi(2) + (desktop_position.y - start.y).powi(2);
                if distance_squared < DRAG_THRESHOLD_LOGICAL.powi(2) {
                    return InteractionAction::None;
                }
                self.pointer_state = PointerState::Dragged { offset };
                self.record_drag_sample(desktop_position, timestamp);
                InteractionAction::BeginDrag {
                    offset,
                    desktop_position: drag_window_position(desktop_position, offset),
                }
            }
            PointerState::Dragged { offset } => {
                self.record_drag_sample(desktop_position, timestamp);
                InteractionAction::MoveDrag {
                    desktop_position: drag_window_position(desktop_position, offset),
                }
            }
        }
    }

    pub fn pointer_up(
        &mut self,
        desktop_position: Option<DesktopPosition>,
        timestamp: Duration,
    ) -> InteractionAction {
        let previous = std::mem::take(&mut self.pointer_state);
        match previous {
            PointerState::Hovering => InteractionAction::None,
            PointerState::Pressed { .. } => {
                self.drag_samples.clear();
                if self.current_hit && desktop_position.is_some_and(DesktopPosition::is_finite) {
                    InteractionAction::ClickPet
                } else {
                    InteractionAction::None
                }
            }
            PointerState::Dragged { .. } => {
                if let Some(position) = desktop_position.filter(|position| position.is_finite()) {
                    self.record_drag_sample(position, timestamp);
                }
                let release_velocity = self.release_velocity();
                self.drag_samples.clear();
                InteractionAction::EndDrag { release_velocity }
            }
        }
    }

    pub fn cancel_pointer(&mut self) -> InteractionAction {
        let was_dragged = matches!(self.pointer_state, PointerState::Dragged { .. });
        self.pointer_state = PointerState::Hovering;
        self.drag_samples.clear();
        if was_dragged {
            InteractionAction::EndDrag {
                release_velocity: [0.0, 0.0],
            }
        } else {
            InteractionAction::None
        }
    }

    pub fn click_through_required(&self) -> bool {
        !self.current_hit && matches!(self.pointer_state, PointerState::Hovering)
    }

    #[cfg(test)]
    fn is_dragging(&self) -> bool {
        matches!(self.pointer_state, PointerState::Dragged { .. })
    }

    fn record_drag_sample(&mut self, desktop_position: DesktopPosition, timestamp: Duration) {
        if self
            .drag_samples
            .back()
            .is_some_and(|sample| timestamp < sample.timestamp)
        {
            self.drag_samples.clear();
        }
        self.drag_samples.push_back(DragSample {
            desktop_position,
            timestamp,
        });
        while self.drag_samples.len() > MAX_RELEASE_SAMPLES {
            self.drag_samples.pop_front();
        }
        while self.drag_samples.front().is_some_and(|sample| {
            timestamp.saturating_sub(sample.timestamp) > RELEASE_SAMPLE_WINDOW
        }) {
            self.drag_samples.pop_front();
        }
    }

    fn release_velocity(&self) -> [f64; 2] {
        let Some(first) = self.drag_samples.front() else {
            return [0.0, 0.0];
        };
        let Some(last) = self.drag_samples.back() else {
            return [0.0, 0.0];
        };
        let seconds = last.timestamp.saturating_sub(first.timestamp).as_secs_f64();
        if seconds <= f64::EPSILON {
            return [0.0, 0.0];
        }
        clamp_velocity([
            (last.desktop_position.x - first.desktop_position.x) / seconds,
            (last.desktop_position.y - first.desktop_position.y) / seconds,
        ])
    }
}

fn drag_window_position(desktop_position: DesktopPosition, offset: [f64; 2]) -> DesktopPosition {
    DesktopPosition::new(
        desktop_position.x - offset[0],
        desktop_position.y - offset[1],
    )
}

fn clamp_velocity(mut velocity: [f64; 2]) -> [f64; 2] {
    let magnitude = velocity[0].hypot(velocity[1]);
    if magnitude > MAX_RELEASE_SPEED_LOGICAL_PX_PER_S {
        let scale = MAX_RELEASE_SPEED_LOGICAL_PX_PER_S / magnitude;
        velocity[0] *= scale;
        velocity[1] *= scale;
    }
    velocity
}

fn homogeneous_point(point: Vec4) -> Option<Vec3> {
    if !vec4_is_finite(point) || point.w.abs() <= f32::EPSILON {
        return None;
    }
    let point = point.truncate() / point.w;
    vec3_is_finite(point).then_some(point)
}

fn matrix_is_finite(matrix: Mat4) -> bool {
    matrix.to_cols_array().into_iter().all(f32::is_finite)
}

fn vec3_is_finite(vector: Vec3) -> bool {
    vector.to_array().into_iter().all(f32::is_finite)
}

fn vec4_is_finite(vector: Vec4) -> bool {
    vector.to_array().into_iter().all(f32::is_finite)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::PhysicalSize;

    fn projection(matrix: Mat4, viewport: PhysicalSize, scale_factor: f64) -> PetProjection {
        PetProjection {
            bounds_min: Vec3::new(-0.5, -0.5, 0.0),
            bounds_max: Vec3::new(0.5, 0.5, 0.0),
            clip_from_model: matrix,
            viewport,
            scale_factor,
        }
    }

    #[test]
    fn projected_bounds_follow_viewport_and_dpi_without_changing_logical_region() {
        let one_x = RectHitRegion::from_pet_projection(
            projection(Mat4::IDENTITY, PhysicalSize::new(320, 320), 1.0),
            0.0,
        )
        .expect("valid 1x projection");
        let retina = RectHitRegion::from_pet_projection(
            projection(Mat4::IDENTITY, PhysicalSize::new(640, 640), 2.0),
            0.0,
        )
        .expect("valid Retina projection");

        assert_eq!(
            one_x,
            RectHitRegion::new([80.0, 80.0], [240.0, 240.0]).unwrap()
        );
        assert_eq!(retina, one_x);
    }

    #[test]
    fn facing_transform_mirrors_an_asymmetric_projected_region() {
        let asymmetric = PetProjection {
            bounds_min: Vec3::new(-0.75, -0.5, 0.0),
            bounds_max: Vec3::new(0.25, 0.5, 0.0),
            clip_from_model: Mat4::IDENTITY,
            viewport: PhysicalSize::new(320, 320),
            scale_factor: 1.0,
        };
        let left = RectHitRegion::from_pet_projection(asymmetric, 0.0).unwrap();
        let right = RectHitRegion::from_pet_projection(
            PetProjection {
                clip_from_model: Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)),
                ..asymmetric
            },
            0.0,
        )
        .unwrap();

        assert_eq!(
            left,
            RectHitRegion::new([40.0, 80.0], [200.0, 240.0]).unwrap()
        );
        assert_eq!(
            right,
            RectHitRegion::new([120.0, 80.0], [280.0, 240.0]).unwrap()
        );
    }

    #[test]
    fn rectangular_hit_region_has_inclusive_edges_and_rejects_nan() {
        let region = RectHitRegion::new([10.0, 20.0], [30.0, 40.0]).unwrap();
        for point in [[10.0, 20.0], [30.0, 40.0], [20.0, 30.0]] {
            assert!(region.contains(point));
        }
        for point in [[9.99, 20.0], [30.01, 40.0], [f64::NAN, 30.0]] {
            assert!(!region.contains(point));
        }
    }

    #[test]
    fn invalid_projection_inputs_return_no_region() {
        for candidate in [
            projection(Mat4::IDENTITY, PhysicalSize::new(0, 320), 1.0),
            projection(Mat4::IDENTITY, PhysicalSize::new(320, 320), 0.0),
            projection(
                Mat4::from_cols_array(&[f32::NAN; 16]),
                PhysicalSize::new(320, 320),
                1.0,
            ),
        ] {
            assert!(RectHitRegion::from_pet_projection(candidate, 0.0).is_none());
        }
    }

    #[test]
    fn camera_ray_unprojects_ndc_and_rejects_singular_matrices() {
        let ray = CameraRay::from_ndc([0.25, -0.5], Mat4::IDENTITY)
            .expect("identity clip transform is invertible");
        assert_eq!(ray.origin, Vec3::new(0.25, -0.5, 0.0));
        assert_eq!(ray.direction, Vec3::Z);
        assert!(CameraRay::from_ndc([0.0, 0.0], Mat4::ZERO).is_none());
        assert!(CameraRay::from_ndc([f64::NAN, 0.0], Mat4::IDENTITY).is_none());
    }

    #[test]
    fn controller_reports_only_hit_state_changes() {
        let region = RectHitRegion::new([10.0, 10.0], [20.0, 20.0]).unwrap();
        let mut controller = InteractionController::default();
        assert_eq!(
            controller.update_hit(Some([15.0, 15.0]), Some(&region)),
            HitUpdate {
                hit: true,
                changed: true
            }
        );
        assert_eq!(
            controller.update_hit(Some([16.0, 16.0]), Some(&region)),
            HitUpdate {
                hit: true,
                changed: false
            }
        );
        assert_eq!(
            controller.update_hit(None, Some(&region)),
            HitUpdate {
                hit: false,
                changed: true
            }
        );
        assert!(!controller.current_hit());
    }

    #[test]
    fn complete_click_on_pet_emits_action() {
        let region = RectHitRegion::new([10.0, 10.0], [20.0, 20.0]).unwrap();
        let mut controller = InteractionController::default();
        controller.update_hit(Some([15.0, 15.0]), Some(&region));
        assert_eq!(
            controller.pointer_down(
                Some(DesktopPosition::new(115.0, 215.0)),
                DesktopPosition::new(100.0, 200.0),
                Duration::ZERO,
            ),
            InteractionAction::None
        );
        assert!(!controller.click_through_required());
        assert_eq!(
            controller.pointer_up(
                Some(DesktopPosition::new(115.0, 215.0)),
                Duration::from_millis(50),
            ),
            InteractionAction::ClickPet
        );
    }

    #[test]
    fn leaving_pet_or_cancelling_suppresses_click() {
        let region = RectHitRegion::new([10.0, 10.0], [20.0, 20.0]).unwrap();
        let mut controller = InteractionController::default();
        controller.update_hit(Some([15.0, 15.0]), Some(&region));
        controller.pointer_down(
            Some(DesktopPosition::new(115.0, 215.0)),
            DesktopPosition::new(100.0, 200.0),
            Duration::ZERO,
        );
        controller.update_hit(Some([30.0, 30.0]), Some(&region));
        assert!(
            !controller.click_through_required(),
            "a pressed pointer keeps event delivery until release"
        );
        assert_eq!(
            controller.pointer_up(
                Some(DesktopPosition::new(130.0, 230.0)),
                Duration::from_millis(20),
            ),
            InteractionAction::None
        );
        assert!(controller.click_through_required());

        controller.update_hit(Some([15.0, 15.0]), Some(&region));
        controller.pointer_down(
            Some(DesktopPosition::new(115.0, 215.0)),
            DesktopPosition::new(100.0, 200.0),
            Duration::ZERO,
        );
        assert_eq!(controller.cancel_pointer(), InteractionAction::None);
        assert_eq!(
            controller.pointer_up(
                Some(DesktopPosition::new(115.0, 215.0)),
                Duration::from_millis(20),
            ),
            InteractionAction::None
        );
    }

    #[test]
    fn rapid_hit_changes_map_directly_to_platform_policy() {
        let region = RectHitRegion::new([10.0, 10.0], [20.0, 20.0]).unwrap();
        let mut controller = InteractionController::default();
        let policies = [None, Some([15.0, 15.0]), None, Some([20.0, 20.0])].map(|position| {
            controller.update_hit(position, Some(&region));
            controller.click_through_required()
        });
        assert_eq!(policies, [true, false, true, false]);
    }

    #[test]
    fn drag_threshold_preserves_press_offset_and_uses_absolute_position() {
        let region = RectHitRegion::new([0.0, 0.0], [320.0, 320.0]).unwrap();
        let mut controller = InteractionController::default();
        controller.update_hit(Some([15.0, 15.0]), Some(&region));
        controller.pointer_down(
            Some(DesktopPosition::new(115.0, 215.0)),
            DesktopPosition::new(100.0, 200.0),
            Duration::ZERO,
        );

        assert_eq!(
            controller.pointer_moved(
                Some(DesktopPosition::new(117.0, 218.0)),
                Duration::from_millis(10),
            ),
            InteractionAction::None
        );
        assert_eq!(
            controller.pointer_moved(
                Some(DesktopPosition::new(118.0, 219.0)),
                Duration::from_millis(20),
            ),
            InteractionAction::BeginDrag {
                offset: [15.0, 15.0],
                desktop_position: DesktopPosition::new(103.0, 204.0),
            }
        );
        assert!(controller.is_dragging());
        assert_eq!(
            controller.pointer_moved(
                Some(DesktopPosition::new(300.0, -40.0)),
                Duration::from_millis(40),
            ),
            InteractionAction::MoveDrag {
                desktop_position: DesktopPosition::new(285.0, -55.0),
            }
        );
    }

    #[test]
    fn release_velocity_uses_recent_bounded_samples() {
        let region = RectHitRegion::new([0.0, 0.0], [320.0, 320.0]).unwrap();
        let mut controller = InteractionController::default();
        controller.update_hit(Some([15.0, 15.0]), Some(&region));
        controller.pointer_down(
            Some(DesktopPosition::new(100.0, 100.0)),
            DesktopPosition::new(85.0, 85.0),
            Duration::ZERO,
        );
        controller.pointer_moved(
            Some(DesktopPosition::new(110.0, 110.0)),
            Duration::from_millis(20),
        );
        controller.pointer_moved(
            Some(DesktopPosition::new(130.0, 140.0)),
            Duration::from_millis(60),
        );
        assert_eq!(
            controller.pointer_up(
                Some(DesktopPosition::new(140.0, 150.0)),
                Duration::from_millis(100),
            ),
            InteractionAction::EndDrag {
                release_velocity: [400.0, 500.0],
            }
        );
        assert!(!controller.is_dragging());
    }

    #[test]
    fn drag_cancel_releases_capture_without_velocity() {
        let region = RectHitRegion::new([0.0, 0.0], [320.0, 320.0]).unwrap();
        let mut controller = InteractionController::default();
        controller.update_hit(Some([20.0, 20.0]), Some(&region));
        controller.pointer_down(
            Some(DesktopPosition::new(-480.0, 220.0)),
            DesktopPosition::new(-500.0, 200.0),
            Duration::ZERO,
        );
        controller.pointer_moved(
            Some(DesktopPosition::new(-450.0, 250.0)),
            Duration::from_millis(16),
        );
        assert_eq!(
            controller.cancel_pointer(),
            InteractionAction::EndDrag {
                release_velocity: [0.0, 0.0],
            }
        );
        assert!(!controller.is_dragging());
    }

    #[test]
    fn release_speed_is_clamped_and_non_monotonic_time_recovers() {
        let region = RectHitRegion::new([0.0, 0.0], [320.0, 320.0]).unwrap();
        let mut controller = InteractionController::default();
        controller.update_hit(Some([10.0, 10.0]), Some(&region));
        controller.pointer_down(
            Some(DesktopPosition::new(10.0, 10.0)),
            DesktopPosition::default(),
            Duration::from_millis(20),
        );
        controller.pointer_moved(
            Some(DesktopPosition::new(20.0, 10.0)),
            Duration::from_millis(10),
        );
        let action = controller.pointer_up(
            Some(DesktopPosition::new(1_020.0, 10.0)),
            Duration::from_millis(11),
        );
        let InteractionAction::EndDrag { release_velocity } = action else {
            panic!("drag release must emit velocity");
        };
        assert!((release_velocity[0] - MAX_RELEASE_SPEED_LOGICAL_PX_PER_S).abs() < 1e-9);
        assert_eq!(release_velocity[1], 0.0);
    }

    #[test]
    fn release_samples_are_bounded_and_expire() {
        let mut controller = InteractionController::default();
        for index in 0..20_u64 {
            controller.record_drag_sample(
                DesktopPosition::new(index as f64, 0.0),
                Duration::from_millis(index * 10),
            );
        }
        assert_eq!(controller.drag_samples.len(), MAX_RELEASE_SAMPLES);
        assert_eq!(
            controller.drag_samples.front().unwrap().desktop_position,
            DesktopPosition::new(12.0, 0.0)
        );

        controller.record_drag_sample(DesktopPosition::new(30.0, 0.0), Duration::from_millis(400));
        assert_eq!(controller.drag_samples.len(), 1);
        assert_eq!(controller.release_velocity(), [0.0, 0.0]);
    }

    #[test]
    fn missing_release_position_never_synthesizes_a_click() {
        let region = RectHitRegion::new([0.0, 0.0], [320.0, 320.0]).unwrap();
        let mut controller = InteractionController::default();
        controller.update_hit(Some([20.0, 20.0]), Some(&region));
        controller.pointer_down(
            Some(DesktopPosition::new(120.0, 220.0)),
            DesktopPosition::new(100.0, 200.0),
            Duration::ZERO,
        );
        assert_eq!(
            controller.pointer_up(None, Duration::from_millis(20)),
            InteractionAction::None
        );
    }
}
