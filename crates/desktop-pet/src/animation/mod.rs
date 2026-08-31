//! Deterministic skeletal animation playback and cross-fading.

use std::time::Duration;

use glam::{Mat4, Quat, Vec3};
use thiserror::Error;

use crate::asset::{
    AnimationChannelData, AnimationClipData, ChannelValues, Interpolation, LocalTransform,
    PetAsset, RigData,
};

const CROSS_FADE_DURATION: f32 = 0.250;

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
        Self::from_clips(asset.rig.clone(), idle, walk)
    }

    fn from_clips(
        rig: RigData,
        idle: AnimationClipData,
        walk: AnimationClipData,
    ) -> Result<Self, AnimationError> {
        let local_pose = rig.nodes.iter().map(|node| node.bind_transform).collect();
        let mut controller = Self {
            global_pose: vec![Mat4::IDENTITY; rig.nodes.len()],
            skin_matrices: Vec::new(),
            rig,
            clips: [idle, walk],
            clip_elapsed: [0.0; 2],
            current: AnimationRequest::Idle,
            playback_speed: 1.0,
            transition: None,
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
            source_pose: self.local_pose.clone(),
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
        self.rebuild_pose()
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
        self.local_pose = local_pose;
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

    fn controller(idle: AnimationClipData, walk: AnimationClipData) -> AnimationController {
        AnimationController::from_clips(single_joint_rig(), idle, walk).expect("valid controller")
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
}
