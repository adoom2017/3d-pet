//! Pointer hit testing in window-local logical coordinates.

use glam::{Mat4, Vec3, Vec4};

use crate::render::PetProjection;

pub(crate) const DEFAULT_HIT_PADDING_LOGICAL: f64 = 6.0;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InteractionAction {
    None,
    ClickPet,
}

#[derive(Debug, Default)]
pub(crate) struct InteractionController {
    current_hit: bool,
    pressed_on_pet: bool,
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

    pub fn set_left_pressed(&mut self, pressed: bool) -> InteractionAction {
        if pressed {
            self.pressed_on_pet = self.current_hit;
            return InteractionAction::None;
        }
        let clicked = self.pressed_on_pet && self.current_hit;
        self.pressed_on_pet = false;
        if clicked {
            InteractionAction::ClickPet
        } else {
            InteractionAction::None
        }
    }

    pub fn cancel_pointer(&mut self) {
        self.pressed_on_pet = false;
    }

    pub fn click_through_required(&self) -> bool {
        !self.current_hit && !self.pressed_on_pet
    }
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
        assert_eq!(controller.set_left_pressed(true), InteractionAction::None);
        assert!(!controller.click_through_required());
        assert_eq!(
            controller.set_left_pressed(false),
            InteractionAction::ClickPet
        );
    }

    #[test]
    fn leaving_pet_or_cancelling_suppresses_click() {
        let region = RectHitRegion::new([10.0, 10.0], [20.0, 20.0]).unwrap();
        let mut controller = InteractionController::default();
        controller.update_hit(Some([15.0, 15.0]), Some(&region));
        controller.set_left_pressed(true);
        controller.update_hit(Some([30.0, 30.0]), Some(&region));
        assert!(
            !controller.click_through_required(),
            "a pressed pointer keeps event delivery until release"
        );
        assert_eq!(controller.set_left_pressed(false), InteractionAction::None);
        assert!(controller.click_through_required());

        controller.update_hit(Some([15.0, 15.0]), Some(&region));
        controller.set_left_pressed(true);
        controller.cancel_pointer();
        assert_eq!(controller.set_left_pressed(false), InteractionAction::None);
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
}
