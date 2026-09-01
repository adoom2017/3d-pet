//! Deterministic skeletal animation playback and cross-fading.

use std::time::Duration;

use glam::{Mat4, Quat, Vec3};
use thiserror::Error;

use crate::asset::{
    AnimationChannelData, AnimationClipData, ChannelValues, Interpolation, LocalTransform,
    LookAtManifest, PetAsset, RigData,
};

const CROSS_FADE_DURATION: f32 = 0.250;
const LOOK_RESPONSE_PER_SECOND: f32 = 12.0;
const MAX_YAW_RADIANS: f32 = 40.0_f32.to_radians();
const MIN_PITCH_RADIANS: f32 = -20.0_f32.to_radians();
const MAX_PITCH_RADIANS: f32 = 25.0_f32.to_radians();

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LookTarget {
    pub yaw_radians: f32,
    pub pitch_radians: f32,
}

impl LookTarget {
    pub fn from_window_points(
        head_logical: [f64; 2],
        mouse_logical: [f64; 2],
        reference_distance: f64,
    ) -> Option<Self> {
        if !head_logical.into_iter().all(f64::is_finite)
            || !mouse_logical.into_iter().all(f64::is_finite)
            || !reference_distance.is_finite()
            || reference_distance <= 0.0
        {
            return None;
        }
        let horizontal = ((mouse_logical[0] - head_logical[0]) / reference_distance) as f32;
        let vertical = ((head_logical[1] - mouse_logical[1]) / reference_distance) as f32;
        Some(Self {
            yaw_radians: horizontal.atan().clamp(-MAX_YAW_RADIANS, MAX_YAW_RADIANS),
            pitch_radians: vertical.atan().clamp(MIN_PITCH_RADIANS, MAX_PITCH_RADIANS),
        })
    }
}

struct LookAtLayer {
    head_node: usize,
    yaw_axis: Vec3,
    pitch_axis: Vec3,
    yaw_sign: f32,
    pitch_sign: f32,
    target: LookTarget,
    current: LookTarget,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum AnimationRequest {
    Idle,
    Walk,
}

impl AnimationRequest {
    const fn index(self) -> usize {
        match self {
            Self::Idle => 0,
            Self::Walk => 1,
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum AnimationError {
    #[error("animation clip {0} was not loaded")]
    MissingClip(String),
    #[error("animation targets an invalid node index")]
    InvalidNode,
    #[error("skeleton hierarchy contains a cycle")]
    CyclicHierarchy,
    #[error("animation playback speed must be finite and positive, got {0}")]
    InvalidPlaybackSpeed(f32),
}

struct Transition {
    source_pose: Vec<LocalTransform>,
    elapsed: f32,
}

pub(crate) struct AnimationController {
    rig: RigData,
    clips: [AnimationClipData; 2],
    clip_elapsed: [f32; 2],
    current: AnimationRequest,
    playback_speed: f32,
    transition: Option<Transition>,
    look_at: Option<LookAtLayer>,
    base_pose: Vec<LocalTransform>,
    local_pose: Vec<LocalTransform>,
    global_pose: Vec<Mat4>,
    skin_matrices: Vec<Vec<Mat4>>,
}

impl AnimationController {
    pub fn new(asset: &PetAsset) -> Result<Self, AnimationError> {
        let idle_name = &asset.manifest.animations.idle;
        let walk_name = &asset.manifest.animations.walk;
        let idle = asset
            .animations
            .get(idle_name)
            .cloned()
            .ok_or_else(|| AnimationError::MissingClip(idle_name.clone()))?;
        let walk = asset
            .animations
            .get(walk_name)
            .cloned()
            .ok_or_else(|| AnimationError::MissingClip(walk_name.clone()))?;
        let look_at = resolve_look_at_layer(
            &asset.rig,
            asset.manifest.skeleton.head_joint.as_deref(),
            asset.manifest.skeleton.look_at,
        );
        if let Some(layer) = look_at.as_ref() {
            tracing::info!(
                pet_id = %asset.manifest.id,
                head_node = layer.head_node,
                yaw_axis = ?layer.yaw_axis,
                pitch_axis = ?layer.pitch_axis,
                yaw_sign = layer.yaw_sign,
                pitch_sign = layer.pitch_sign,
                "look-at pose layer enabled"
            );
        } else {
            tracing::warn!(
                pet_id = %asset.manifest.id,
                head_joint = ?asset.manifest.skeleton.head_joint,
                "look-at disabled because the configured head joint or axes are invalid"
            );
        }
        Self::from_clips_with_look_at(asset.rig.clone(), idle, walk, look_at)
    }

    #[cfg(test)]
    fn from_clips(
        rig: RigData,
        idle: AnimationClipData,
        walk: AnimationClipData,
    ) -> Result<Self, AnimationError> {
        Self::from_clips_with_look_at(rig, idle, walk, None)
    }

    fn from_clips_with_look_at(
        rig: RigData,
        idle: AnimationClipData,
        walk: AnimationClipData,
        look_at: Option<LookAtLayer>,
    ) -> Result<Self, AnimationError> {
        let local_pose: Vec<LocalTransform> =
            rig.nodes.iter().map(|node| node.bind_transform).collect();
        let base_pose = local_pose.clone();
        let mut controller = Self {
            global_pose: vec![Mat4::IDENTITY; rig.nodes.len()],
            skin_matrices: Vec::new(),
            rig,
            clips: [idle, walk],
            clip_elapsed: [0.0; 2],
            current: AnimationRequest::Idle,
            playback_speed: 1.0,
            transition: None,
            look_at,
            base_pose,
            local_pose,
        };
        controller.rebuild_pose()?;
        Ok(controller)
    }

    pub fn request(&mut self, request: AnimationRequest) -> bool {
        if request == self.current {
            return false;
        }
        self.transition = Some(Transition {
            source_pose: self.base_pose.clone(),
            elapsed: 0.0,
        });
        self.current = request;
        true
    }

    pub fn set_playback_speed(&mut self, speed: f32) -> Result<(), AnimationError> {
        if !speed.is_finite() || speed <= 0.0 {
            return Err(AnimationError::InvalidPlaybackSpeed(speed));
        }
        self.playback_speed = speed;
        Ok(())
    }

    pub fn advance(&mut self, delta: Duration) -> Result<(), AnimationError> {
        let scaled_delta = delta.as_secs_f32() * self.playback_speed;
        let index = self.current.index();
        let duration = self.clips[index].duration;
        if duration > 0.0 {
            self.clip_elapsed[index] =
                (self.clip_elapsed[index] + scaled_delta).rem_euclid(duration);
        }
        if let Some(transition) = self.transition.as_mut() {
            transition.elapsed += delta.as_secs_f32();
        }
        if let Some(look_at) = self.look_at.as_mut() {
            let factor = 1.0 - (-LOOK_RESPONSE_PER_SECOND * delta.as_secs_f32()).exp();
            look_at.current.yaw_radians +=
                (look_at.target.yaw_radians - look_at.current.yaw_radians) * factor;
            look_at.current.pitch_radians +=
                (look_at.target.pitch_radians - look_at.current.pitch_radians) * factor;
        }
        self.rebuild_pose()
    }

    pub fn set_look_target(&mut self, target: Option<LookTarget>) {
        if let Some(look_at) = self.look_at.as_mut() {
            look_at.target = target.unwrap_or_default();
        }
    }

    pub fn head_model_position(&self) -> Option<Vec3> {
        let head_node = self.look_at.as_ref()?.head_node;
        self.global_pose
            .get(head_node)
            .map(|global| global.transform_point3(Vec3::ZERO))
    }

    pub fn clip_name(&self) -> &str {
        &self.clips[self.current.index()].name
    }

    pub fn skin_matrices(&self) -> &[Vec<Mat4>] {
        &self.skin_matrices
    }

    #[cfg(test)]
    fn elapsed(&self, request: AnimationRequest) -> f32 {
        self.clip_elapsed[request.index()]
    }

    #[cfg(test)]
    fn is_transitioning(&self) -> bool {
        self.transition.is_some()
    }

    #[cfg(test)]
    fn current_look(&self) -> Option<LookTarget> {
        self.look_at.as_ref().map(|look_at| look_at.current)
    }

    fn rebuild_pose(&mut self) -> Result<(), AnimationError> {
        let index = self.current.index();
        let mut local_pose =
            sample_clip_pose(&self.rig, &self.clips[index], self.clip_elapsed[index])?;
        let transition_finished = if let Some(transition) = self.transition.as_ref() {
            let factor = (transition.elapsed / CROSS_FADE_DURATION).clamp(0.0, 1.0);
            for (target, source) in local_pose.iter_mut().zip(&transition.source_pose) {
                *target = blend_transform(*source, *target, factor);
            }
            factor >= 1.0
        } else {
            false
        };
        if transition_finished {
            self.transition = None;
        }
        self.base_pose = local_pose;
        self.local_pose.clone_from(&self.base_pose);
        if let Some(look_at) = self.look_at.as_ref() {
            let head = self
                .local_pose
                .get_mut(look_at.head_node)
                .ok_or(AnimationError::InvalidNode)?;
            let yaw = Quat::from_axis_angle(
                look_at.yaw_axis,
                look_at.current.yaw_radians * look_at.yaw_sign,
            );
            let pitch = Quat::from_axis_angle(
                look_at.pitch_axis,
                look_at.current.pitch_radians * look_at.pitch_sign,
            );
            head.rotation = (head.rotation * yaw * pitch).normalize();
        }
        self.rebuild_joint_matrices()
    }

    fn rebuild_joint_matrices(&mut self) -> Result<(), AnimationError> {
        let mut resolved = vec![false; self.rig.nodes.len()];
        let mut visiting = vec![false; self.rig.nodes.len()];
        for node in 0..self.rig.nodes.len() {
            resolve_global_pose(
                node,
                &self.rig,
                &self.local_pose,
                &mut self.global_pose,
                &mut resolved,
                &mut visiting,
            )?;
        }
        let mut skin_matrices = Vec::with_capacity(self.rig.skins.len());
        for skin in &self.rig.skins {
            let mut matrices = Vec::with_capacity(skin.joints.len());
            for (&joint, inverse_bind) in skin.joints.iter().zip(&skin.inverse_bind_matrices) {
                let global = self
                    .global_pose
                    .get(joint)
                    .ok_or(AnimationError::InvalidNode)?;
                matrices.push(*global * *inverse_bind);
            }
            skin_matrices.push(matrices);
        }
        self.skin_matrices = skin_matrices;
        Ok(())
    }
}

fn resolve_look_at_layer(
    rig: &RigData,
    head_name: Option<&str>,
    config: Option<LookAtManifest>,
) -> Option<LookAtLayer> {
    let head_name = head_name?;
    let head_node = rig
        .nodes
        .iter()
        .position(|node| node.name.as_deref() == Some(head_name))?;
    let config = config.unwrap_or(LookAtManifest {
        yaw_axis: [0.0, 1.0, 0.0],
        pitch_axis: [1.0, 0.0, 0.0],
        yaw_sign: 1.0,
        pitch_sign: 1.0,
    });
    let yaw_axis = normalized_axis(config.yaw_axis)?;
    let pitch_axis = normalized_axis(config.pitch_axis)?;
    if !config.yaw_sign.is_finite()
        || !config.pitch_sign.is_finite()
        || config.yaw_sign == 0.0
        || config.pitch_sign == 0.0
    {
        return None;
    }
    Some(LookAtLayer {
        head_node,
        yaw_axis,
        pitch_axis,
        yaw_sign: config.yaw_sign.signum(),
        pitch_sign: config.pitch_sign.signum(),
        target: LookTarget::default(),
        current: LookTarget::default(),
    })
}

fn normalized_axis(axis: [f32; 3]) -> Option<Vec3> {
    let axis = Vec3::from(axis);
    (axis.is_finite() && axis.length_squared() > f32::EPSILON).then(|| axis.normalize())
}

fn sample_clip_pose(
    rig: &RigData,
    clip: &AnimationClipData,
    time: f32,
) -> Result<Vec<LocalTransform>, AnimationError> {
    let mut pose: Vec<LocalTransform> = rig.nodes.iter().map(|node| node.bind_transform).collect();
    for channel in &clip.channels {
        let target = pose
            .get_mut(channel.target_node)
            .ok_or(AnimationError::InvalidNode)?;
        apply_channel(target, channel, time);
    }
    Ok(pose)
}

fn blend_transform(source: LocalTransform, target: LocalTransform, factor: f32) -> LocalTransform {
    LocalTransform {
        translation: source.translation.lerp(target.translation, factor),
        rotation: source.rotation.slerp(target.rotation, factor).normalize(),
        scale: source.scale.lerp(target.scale, factor),
    }
}

fn apply_channel(pose: &mut LocalTransform, channel: &AnimationChannelData, time: f32) {
    match &channel.values {
        ChannelValues::Translations(values) => {
            pose.translation = sample_vec3(&channel.times, values, channel.interpolation, time);
        }
        ChannelValues::Rotations(values) => {
            pose.rotation = sample_quat(&channel.times, values, channel.interpolation, time);
        }
        ChannelValues::Scales(values) => {
            pose.scale = sample_vec3(&channel.times, values, channel.interpolation, time);
        }
    }
}

fn sample_indices(times: &[f32], time: f32) -> (usize, usize, f32) {
    let upper = times.partition_point(|sample| *sample <= time);
    if upper == 0 {
        return (0, 0, 0.0);
    }
    if upper >= times.len() {
        let last = times.len() - 1;
        return (last, last, 0.0);
    }
    let lower = upper - 1;
    let span = times[upper] - times[lower];
    let factor = if span > f32::EPSILON {
        (time - times[lower]) / span
    } else {
        0.0
    };
    (lower, upper, factor.clamp(0.0, 1.0))
}

fn sample_vec3(times: &[f32], values: &[Vec3], interpolation: Interpolation, time: f32) -> Vec3 {
    let (lower, upper, factor) = sample_indices(times, time);
    if interpolation == Interpolation::Step || lower == upper {
        values[lower]
    } else {
        values[lower].lerp(values[upper], factor)
    }
}

fn sample_quat(times: &[f32], values: &[Quat], interpolation: Interpolation, time: f32) -> Quat {
    let (lower, upper, factor) = sample_indices(times, time);
    if interpolation == Interpolation::Step || lower == upper {
        values[lower]
    } else {
        values[lower].slerp(values[upper], factor).normalize()
    }
}

fn resolve_global_pose(
    node: usize,
    rig: &RigData,
    local_pose: &[LocalTransform],
    global_pose: &mut [Mat4],
    resolved: &mut [bool],
    visiting: &mut [bool],
) -> Result<Mat4, AnimationError> {
    if resolved[node] {
        return Ok(global_pose[node]);
    }
    if visiting[node] {
        return Err(AnimationError::CyclicHierarchy);
    }
    visiting[node] = true;
    let parent_transform = match rig.nodes[node].parent {
        Some(parent) if parent < rig.nodes.len() => {
            resolve_global_pose(parent, rig, local_pose, global_pose, resolved, visiting)?
        }
        Some(_) => return Err(AnimationError::InvalidNode),
        None => Mat4::IDENTITY,
    };
    global_pose[node] = parent_transform * local_pose[node].matrix();
    visiting[node] = false;
    resolved[node] = true;
    Ok(global_pose[node])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{NodeData, SkinData};

    #[test]
    fn bind_pose_joint_matrix_is_identity() {
        let controller = controller(empty_clip("Idle", 1.0), empty_clip("Walk", 1.0));
        assert_mat4_close(controller.skin_matrices()[0][0], Mat4::IDENTITY);
    }

    #[test]
    fn hierarchy_combines_parent_and_child_transforms() {
        let rig = RigData {
            nodes: vec![
                node(None, Vec3::new(2.0, 0.0, 0.0)),
                node(Some(0), Vec3::new(0.0, 3.0, 0.0)),
            ],
            skins: vec![SkinData {
                joints: vec![1],
                inverse_bind_matrices: vec![Mat4::IDENTITY],
            }],
        };
        let controller =
            AnimationController::from_clips(rig, empty_clip("Idle", 1.0), empty_clip("Walk", 1.0))
                .expect("valid hierarchy");
        assert_mat4_close(
            controller.skin_matrices()[0][0],
            Mat4::from_translation(Vec3::new(2.0, 3.0, 0.0)),
        );
    }

    #[test]
    fn linear_translation_samples_and_loops_deterministically() {
        let mut controller = controller(
            translation_clip("Idle", Vec3::ZERO, Vec3::X * 2.0),
            empty_clip("Walk", 1.0),
        );
        controller
            .advance(Duration::from_millis(250))
            .expect("sample");
        assert_translation(&controller, 0.5);
        controller
            .advance(Duration::from_millis(750))
            .expect("loop");
        assert_eq!(controller.elapsed(AnimationRequest::Idle), 0.0);
        assert_translation(&controller, 0.0);
    }

    #[test]
    fn cross_fade_has_exact_start_midpoint_and_end() {
        let mut controller = controller(constant_clip("Idle", 0.0), constant_clip("Walk", 10.0));
        assert!(controller.request(AnimationRequest::Walk));
        assert_translation(&controller, 0.0);
        controller
            .advance(Duration::from_millis(125))
            .expect("midpoint");
        assert_translation(&controller, 5.0);
        controller.advance(Duration::from_millis(125)).expect("end");
        assert_translation(&controller, 10.0);
        assert!(!controller.is_transitioning());
    }

    #[test]
    fn repeated_request_is_idempotent() {
        let mut controller = controller(constant_clip("Idle", 0.0), constant_clip("Walk", 10.0));
        assert!(!controller.request(AnimationRequest::Idle));
        assert!(controller.request(AnimationRequest::Walk));
        controller
            .advance(Duration::from_millis(125))
            .expect("midpoint");
        assert!(!controller.request(AnimationRequest::Walk));
        controller.advance(Duration::from_millis(125)).expect("end");
        assert_translation(&controller, 10.0);
    }

    #[test]
    fn reversing_transition_continues_from_current_pose() {
        let mut controller = controller(constant_clip("Idle", 0.0), constant_clip("Walk", 10.0));
        controller.request(AnimationRequest::Walk);
        controller
            .advance(Duration::from_millis(125))
            .expect("forward");
        assert_translation(&controller, 5.0);
        controller.request(AnimationRequest::Idle);
        assert_translation(&controller, 5.0);
        controller
            .advance(Duration::from_millis(125))
            .expect("reverse");
        assert_translation(&controller, 2.5);
        controller
            .advance(Duration::from_millis(125))
            .expect("idle");
        assert_translation(&controller, 0.0);
    }

    #[test]
    fn playback_speed_scales_clip_time_but_not_fade_duration() {
        let mut controller = controller(
            translation_clip("Idle", Vec3::ZERO, Vec3::X * 2.0),
            constant_clip("Walk", 10.0),
        );
        controller.set_playback_speed(2.0).expect("valid speed");
        controller
            .advance(Duration::from_millis(250))
            .expect("sample");
        assert_translation(&controller, 1.0);
        assert!(matches!(
            controller.set_playback_speed(0.0),
            Err(AnimationError::InvalidPlaybackSpeed(0.0))
        ));
    }

    #[test]
    fn step_channel_holds_previous_value() {
        assert_eq!(
            sample_vec3(
                &[0.0, 1.0],
                &[Vec3::ZERO, Vec3::ONE],
                Interpolation::Step,
                0.75,
            ),
            Vec3::ZERO
        );
    }

    #[test]
    fn invalid_channel_target_is_reported() {
        let mut idle = empty_clip("Idle", 1.0);
        idle.channels.push(AnimationChannelData {
            target_node: 9,
            times: vec![0.0],
            interpolation: Interpolation::Step,
            values: ChannelValues::Translations(vec![Vec3::ZERO]),
        });
        let error =
            AnimationController::from_clips(single_joint_rig(), idle, empty_clip("Walk", 1.0))
                .err()
                .expect("invalid channel must fail");
        assert!(matches!(error, AnimationError::InvalidNode));
    }

    #[test]
    fn invalid_skin_joint_and_cycle_are_reported() {
        let mut rig = single_joint_rig();
        rig.skins[0].joints[0] = 4;
        let error =
            AnimationController::from_clips(rig, empty_clip("Idle", 1.0), empty_clip("Walk", 1.0))
                .err()
                .expect("invalid skin must fail");
        assert!(matches!(error, AnimationError::InvalidNode));

        let mut rig = single_joint_rig();
        rig.nodes[0].parent = Some(0);
        let error =
            AnimationController::from_clips(rig, empty_clip("Idle", 1.0), empty_clip("Walk", 1.0))
                .err()
                .expect("cycle must fail");
        assert!(matches!(error, AnimationError::CyclicHierarchy));
    }

    #[test]
    fn look_target_handles_center_quadrants_clamps_and_invalid_input() {
        let center = LookTarget::from_window_points([160.0, 100.0], [160.0, 100.0], 160.0)
            .expect("center target");
        assert_eq!(center, LookTarget::default());

        let upper_right = LookTarget::from_window_points([160.0, 100.0], [240.0, 20.0], 160.0)
            .expect("upper-right target");
        assert!(upper_right.yaw_radians > 0.0);
        assert!(upper_right.pitch_radians > 0.0);
        let lower_left = LookTarget::from_window_points([160.0, 100.0], [80.0, 180.0], 160.0)
            .expect("lower-left target");
        assert!(lower_left.yaw_radians < 0.0);
        assert!(lower_left.pitch_radians < 0.0);

        let extreme = LookTarget::from_window_points(
            [0.0, 0.0],
            [f64::from(i32::MAX), f64::from(i32::MAX)],
            1.0,
        )
        .expect("extreme target");
        assert_close(extreme.yaw_radians, MAX_YAW_RADIANS, 1.0e-6);
        assert_close(extreme.pitch_radians, MIN_PITCH_RADIANS, 1.0e-6);
        assert!(LookTarget::from_window_points([0.0, 0.0], [1.0, 1.0], 0.0).is_none());
        assert!(LookTarget::from_window_points([f64::NAN, 0.0], [1.0, 1.0], 1.0).is_none());
    }

    #[test]
    fn look_smoothing_converges_without_overshoot_and_is_step_independent() {
        let target = LookTarget {
            yaw_radians: MAX_YAW_RADIANS,
            pitch_radians: MAX_PITCH_RADIANS,
        };
        let mut sixty_hz = look_controller();
        let mut thirty_hz = look_controller();
        sixty_hz.set_look_target(Some(target));
        thirty_hz.set_look_target(Some(target));
        for _ in 0..60 {
            sixty_hz
                .advance(Duration::from_secs_f32(1.0 / 60.0))
                .expect("60 Hz look step");
        }
        for _ in 0..30 {
            thirty_hz
                .advance(Duration::from_secs_f32(1.0 / 30.0))
                .expect("30 Hz look step");
        }
        let sixty = sixty_hz.current_look().expect("look layer");
        let thirty = thirty_hz.current_look().expect("look layer");
        assert!(sixty.yaw_radians > 0.0 && sixty.yaw_radians <= target.yaw_radians);
        assert!(sixty.pitch_radians > 0.0 && sixty.pitch_radians <= target.pitch_radians);
        assert_close(sixty.yaw_radians, thirty.yaw_radians, 1.0e-5);
        assert_close(sixty.pitch_radians, thirty.pitch_radians, 1.0e-5);
    }

    #[test]
    fn missing_target_smoothly_returns_to_neutral() {
        let mut controller = look_controller();
        controller.set_look_target(Some(LookTarget {
            yaw_radians: 0.5,
            pitch_radians: -0.25,
        }));
        controller
            .advance(Duration::from_millis(100))
            .expect("look toward target");
        let before = controller.current_look().expect("look layer");
        controller.set_look_target(None);
        controller
            .advance(Duration::from_millis(100))
            .expect("return toward neutral");
        let after = controller.current_look().expect("look layer");
        assert!(after.yaw_radians.abs() < before.yaw_radians.abs());
        assert!(after.pitch_radians.abs() < before.pitch_radians.abs());
    }

    #[test]
    fn look_overlay_rotates_the_head_after_base_pose_sampling() {
        let mut controller = look_controller();
        controller.set_look_target(Some(LookTarget {
            yaw_radians: 0.4,
            pitch_radians: 0.0,
        }));
        controller
            .advance(Duration::from_secs(1))
            .expect("look overlay");
        let matrix = controller.skin_matrices()[0][0];
        let forward = matrix.transform_vector3(Vec3::Z);
        assert!(forward.x > 0.3, "forward: {forward:?}");
        assert_close(
            controller.base_pose[0]
                .rotation
                .angle_between(Quat::IDENTITY),
            0.0,
            1.0e-6,
        );
    }

    #[test]
    fn missing_head_joint_or_invalid_axes_disable_only_the_overlay() {
        let mut rig = single_joint_rig();
        rig.nodes[0].name = Some("Neck".to_owned());
        assert!(resolve_look_at_layer(&rig, Some("Head"), None).is_none());
        assert!(resolve_look_at_layer(&rig, None, None).is_none());

        rig.nodes[0].name = Some("Head".to_owned());
        assert!(
            resolve_look_at_layer(
                &rig,
                Some("Head"),
                Some(LookAtManifest {
                    yaw_axis: [0.0; 3],
                    pitch_axis: [1.0, 0.0, 0.0],
                    yaw_sign: 1.0,
                    pitch_sign: 1.0,
                })
            )
            .is_none()
        );

        let controller =
            AnimationController::from_clips(rig, empty_clip("Idle", 1.0), empty_clip("Walk", 1.0))
                .expect("base animation remains available");
        assert_mat4_close(controller.skin_matrices()[0][0], Mat4::IDENTITY);
    }

    fn controller(idle: AnimationClipData, walk: AnimationClipData) -> AnimationController {
        AnimationController::from_clips(single_joint_rig(), idle, walk).expect("valid controller")
    }

    fn look_controller() -> AnimationController {
        AnimationController::from_clips_with_look_at(
            single_joint_rig(),
            empty_clip("Idle", 1.0),
            empty_clip("Walk", 1.0),
            Some(LookAtLayer {
                head_node: 0,
                yaw_axis: Vec3::Y,
                pitch_axis: Vec3::X,
                yaw_sign: 1.0,
                pitch_sign: 1.0,
                target: LookTarget::default(),
                current: LookTarget::default(),
            }),
        )
        .expect("valid look controller")
    }

    fn single_joint_rig() -> RigData {
        RigData {
            nodes: vec![node(None, Vec3::ZERO)],
            skins: vec![SkinData {
                joints: vec![0],
                inverse_bind_matrices: vec![Mat4::IDENTITY],
            }],
        }
    }

    fn node(parent: Option<usize>, translation: Vec3) -> NodeData {
        NodeData {
            name: None,
            parent,
            bind_transform: LocalTransform {
                translation,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        }
    }

    fn empty_clip(name: &str, duration: f32) -> AnimationClipData {
        AnimationClipData {
            name: name.to_owned(),
            duration,
            channels: Vec::new(),
        }
    }

    fn constant_clip(name: &str, x: f32) -> AnimationClipData {
        let mut clip = empty_clip(name, 1.0);
        clip.channels.push(AnimationChannelData {
            target_node: 0,
            times: vec![0.0],
            interpolation: Interpolation::Step,
            values: ChannelValues::Translations(vec![Vec3::X * x]),
        });
        clip
    }

    fn translation_clip(name: &str, start: Vec3, end: Vec3) -> AnimationClipData {
        let mut clip = empty_clip(name, 1.0);
        clip.channels.push(AnimationChannelData {
            target_node: 0,
            times: vec![0.0, 1.0],
            interpolation: Interpolation::Linear,
            values: ChannelValues::Translations(vec![start, end]),
        });
        clip
    }

    fn assert_translation(controller: &AnimationController, expected_x: f32) {
        let actual = controller.skin_matrices()[0][0].transform_point3(Vec3::ZERO);
        assert!(
            (actual.x - expected_x).abs() < 1e-5,
            "translation: {actual:?}"
        );
    }

    fn assert_mat4_close(actual: Mat4, expected: Mat4) {
        assert!(
            actual
                .to_cols_array()
                .iter()
                .zip(expected.to_cols_array())
                .all(|(actual, expected)| (*actual - expected).abs() < 1e-5),
            "matrix mismatch: {actual:?}"
        );
    }

    fn assert_close(actual: f32, expected: f32, epsilon: f32) {
        assert!(
            (actual - expected).abs() <= epsilon,
            "expected {expected}, got {actual}"
        );
    }
}
