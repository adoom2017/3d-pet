//! Deterministic skeletal pose sampling and joint-matrix generation.

use std::time::Duration;

use glam::{Mat4, Quat, Vec3};
use thiserror::Error;

use crate::asset::{
    AnimationChannelData, AnimationClipData, ChannelValues, Interpolation, LocalTransform,
    PetAsset, RigData,
};

#[derive(Debug, Error)]
pub(crate) enum AnimationError {
    #[error("animation clip {0} was not loaded")]
    MissingClip(String),
    #[error("animation targets an invalid node index")]
    InvalidNode,
    #[error("skeleton hierarchy contains a cycle")]
    CyclicHierarchy,
}

pub(crate) struct AnimationController {
    rig: RigData,
    clip: AnimationClipData,
    elapsed: f32,
    local_pose: Vec<LocalTransform>,
    global_pose: Vec<Mat4>,
    skin_matrices: Vec<Vec<Mat4>>,
}

impl AnimationController {
    pub fn idle(asset: &PetAsset) -> Result<Self, AnimationError> {
        let clip_name = &asset.manifest.animations.idle;
        let clip = asset
            .animations
            .get(clip_name)
            .cloned()
            .ok_or_else(|| AnimationError::MissingClip(clip_name.clone()))?;
        Self::new(asset.rig.clone(), clip)
    }

    fn new(rig: RigData, clip: AnimationClipData) -> Result<Self, AnimationError> {
        let local_pose = rig.nodes.iter().map(|node| node.bind_transform).collect();
        let mut controller = Self {
            global_pose: vec![Mat4::IDENTITY; rig.nodes.len()],
            skin_matrices: Vec::new(),
            rig,
            clip,
            elapsed: 0.0,
            local_pose,
        };
        controller.sample_current_pose()?;
        Ok(controller)
    }

    pub fn advance(&mut self, delta: Duration) -> Result<(), AnimationError> {
        if self.clip.duration > 0.0 {
            self.elapsed = (self.elapsed + delta.as_secs_f32()).rem_euclid(self.clip.duration);
        }
        self.sample_current_pose()
    }

    pub fn clip_name(&self) -> &str {
        &self.clip.name
    }

    #[cfg(test)]
    fn elapsed(&self) -> f32 {
        self.elapsed
    }

    pub fn skin_matrices(&self) -> &[Vec<Mat4>] {
        &self.skin_matrices
    }

    fn sample_current_pose(&mut self) -> Result<(), AnimationError> {
        self.local_pose
            .iter_mut()
            .zip(&self.rig.nodes)
            .for_each(|(pose, node)| *pose = node.bind_transform);
        for channel in &self.clip.channels {
            let pose = self
                .local_pose
                .get_mut(channel.target_node)
                .ok_or(AnimationError::InvalidNode)?;
            apply_channel(pose, channel, self.elapsed);
        }

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
        let controller =
            AnimationController::new(single_joint_rig(), empty_clip(1.0)).expect("valid bind pose");
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
        let controller = AnimationController::new(rig, empty_clip(1.0)).expect("valid hierarchy");
        assert_mat4_close(
            controller.skin_matrices()[0][0],
            Mat4::from_translation(Vec3::new(2.0, 3.0, 0.0)),
        );
    }

    #[test]
    fn linear_translation_samples_and_loops_deterministically() {
        let clip = AnimationClipData {
            name: "Idle".to_owned(),
            duration: 1.0,
            channels: vec![AnimationChannelData {
                target_node: 0,
                times: vec![0.0, 1.0],
                interpolation: Interpolation::Linear,
                values: ChannelValues::Translations(vec![Vec3::ZERO, Vec3::X * 2.0]),
            }],
        };
        let mut controller = AnimationController::new(single_joint_rig(), clip).expect("clip");
        controller
            .advance(Duration::from_millis(250))
            .expect("sample");
        assert_mat4_close(
            controller.skin_matrices()[0][0],
            Mat4::from_translation(Vec3::X * 0.5),
        );
        controller
            .advance(Duration::from_millis(750))
            .expect("loop");
        assert_eq!(controller.elapsed(), 0.0);
        assert_mat4_close(controller.skin_matrices()[0][0], Mat4::IDENTITY);
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
        let mut clip = empty_clip(1.0);
        clip.channels.push(AnimationChannelData {
            target_node: 9,
            times: vec![0.0],
            interpolation: Interpolation::Step,
            values: ChannelValues::Translations(vec![Vec3::ZERO]),
        });
        let error = AnimationController::new(single_joint_rig(), clip)
            .err()
            .expect("invalid channel must fail");
        assert!(matches!(error, AnimationError::InvalidNode));
    }

    #[test]
    fn invalid_skin_joint_is_reported() {
        let mut rig = single_joint_rig();
        rig.skins[0].joints[0] = 4;
        let error = AnimationController::new(rig, empty_clip(1.0))
            .err()
            .expect("invalid skin must fail");
        assert!(matches!(error, AnimationError::InvalidNode));
    }

    #[test]
    fn cyclic_hierarchy_is_reported() {
        let mut rig = single_joint_rig();
        rig.nodes[0].parent = Some(0);
        let error = AnimationController::new(rig, empty_clip(1.0))
            .err()
            .expect("cycle must fail");
        assert!(matches!(error, AnimationError::CyclicHierarchy));
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

    fn empty_clip(duration: f32) -> AnimationClipData {
        AnimationClipData {
            name: "Idle".to_owned(),
            duration,
            channels: Vec::new(),
        }
    }

    fn assert_mat4_close(actual: Mat4, expected: Mat4) {
        let difference = actual.to_cols_array().map(f32::abs);
        let expected = expected.to_cols_array();
        assert!(
            actual
                .to_cols_array()
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (*actual - expected).abs() < 1e-5),
            "matrix mismatch: {actual:?}; absolute values: {difference:?}"
        );
    }
}
