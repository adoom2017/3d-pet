//! Trusted manifest and glTF/GLB loading boundary.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_MODEL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_NODES: usize = 2_048;
const MAX_PRIMITIVES: usize = 256;
const MAX_VERTICES: usize = 1_000_000;
const MAX_INDICES: usize = 3_000_000;
pub(crate) const MAX_JOINTS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PetAssetHandle(usize);

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("asset handle did not resolve after loading")]
    InvalidHandle,
    #[error("asset path is outside the trusted root: {0}")]
    OutsideTrustedRoot(PathBuf),
    #[error("asset path does not exist or cannot be resolved: {path}: {source}")]
    Canonicalize {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to read asset file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("asset file {path} is {actual} bytes, above the {limit} byte limit")]
    FileTooLarge {
        path: PathBuf,
        actual: u64,
        limit: u64,
    },
    #[error("manifest JSON is invalid: {0}")]
    ManifestJson(#[from] serde_json::Error),
    #[error("unsupported manifest format version {0}")]
    UnsupportedManifestVersion(u32),
    #[error("manifest field {0} is missing or empty")]
    MissingManifestField(&'static str),
    #[error("manifest model must be a .glb file")]
    ModelMustBeGlb,
    #[error("model SHA-256 mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("GLB is invalid: {0}")]
    InvalidGltf(#[from] gltf::Error),
    #[error("GLB must contain one binary buffer blob")]
    MissingBinaryBlob,
    #[error("external GLB URI is not allowed: {0}")]
    ExternalUri(String),
    #[error("mesh primitive is missing POSITION data")]
    MissingPositions,
    #[error("mesh primitive uses unsupported draw mode {0:?}")]
    UnsupportedPrimitiveMode(gltf::mesh::Mode),
    #[error("asset limit exceeded: {0}")]
    LimitExceeded(&'static str),
    #[error("required animation mapping {semantic} -> {clip} does not exist in the GLB")]
    MissingAnimation {
        semantic: &'static str,
        clip: String,
    },
    #[error("embedded image is invalid: {0}")]
    InvalidImage(#[from] image::ImageError),
    #[error("skin data is invalid: {0}")]
    InvalidSkin(&'static str),
    #[error("animation channel is invalid: {0}")]
    InvalidAnimation(&'static str),
    #[error("animation interpolation {0:?} is not supported")]
    UnsupportedInterpolation(gltf::animation::Interpolation),
    #[error("morph target animation is not supported")]
    UnsupportedMorphAnimation,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PetManifest {
    pub format_version: u32,
    pub id: String,
    pub name: String,
    pub model: String,
    pub animations: AnimationManifest,
    pub skeleton: SkeletonManifest,
    pub source: SourceManifest,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnimationManifest {
    pub idle: String,
    pub walk: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SkeletonManifest {
    pub head_joint: Option<String>,
    #[serde(default)]
    pub look_at: Option<LookAtManifest>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LookAtManifest {
    pub yaw_axis: [f32; 3],
    pub pitch_axis: [f32; 3],
    #[serde(default = "positive_one")]
    pub yaw_sign: f32,
    #[serde(default = "positive_one")]
    pub pitch_sign: f32,
}

const fn positive_one() -> f32 {
    1.0
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceManifest {
    pub author: String,
    pub url: String,
    pub license: String,
    pub retrieved_on: String,
    pub sha256: String,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct CpuVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tex_coord: [f32; 2],
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalTransform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl LocalTransform {
    pub fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NodeData {
    pub name: Option<String>,
    pub parent: Option<usize>,
    pub bind_transform: LocalTransform,
}

#[derive(Clone, Debug)]
pub(crate) struct SkinData {
    pub joints: Vec<usize>,
    pub inverse_bind_matrices: Vec<Mat4>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Interpolation {
    Step,
    Linear,
}

#[derive(Clone, Debug)]
pub(crate) enum ChannelValues {
    Translations(Vec<Vec3>),
    Rotations(Vec<Quat>),
    Scales(Vec<Vec3>),
}

#[derive(Clone, Debug)]
pub(crate) struct AnimationChannelData {
    pub target_node: usize,
    pub times: Vec<f32>,
    pub interpolation: Interpolation,
    pub values: ChannelValues,
}

#[derive(Clone, Debug)]
pub(crate) struct AnimationClipData {
    pub name: String,
    pub duration: f32,
    pub channels: Vec<AnimationChannelData>,
}

#[derive(Clone, Debug)]
pub(crate) struct RigData {
    pub nodes: Vec<NodeData>,
    pub skins: Vec<SkinData>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AlphaMode {
    Opaque,
    Mask(f32),
    Blend,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum WrapMode {
    ClampToEdge,
    MirroredRepeat,
    Repeat,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SamplerData {
    pub mag_nearest: bool,
    pub min_nearest: bool,
    pub wrap_u: WrapMode,
    pub wrap_v: WrapMode,
}

#[derive(Clone, Debug)]
pub(crate) struct TextureData {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
    pub sampler: SamplerData,
}

#[derive(Clone, Debug)]
pub(crate) struct MaterialData {
    pub base_color: [f32; 4],
    pub base_color_texture: Option<TextureData>,
    pub alpha_mode: AlphaMode,
    pub double_sided: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct MeshPrimitive {
    pub vertices: Vec<CpuVertex>,
    pub indices: Vec<u32>,
    pub material: MaterialData,
    pub skin_index: Option<usize>,
}

#[derive(Debug)]
pub(crate) struct PetAsset {
    pub manifest: PetManifest,
    pub primitives: Vec<MeshPrimitive>,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    pub animation_names: HashSet<String>,
    pub animations: HashMap<String, AnimationClipData>,
    pub rig: RigData,
}

pub(crate) struct AssetManager {
    trusted_root: PathBuf,
    pets: Vec<PetAsset>,
}

impl AssetManager {
    pub fn new(trusted_root: impl AsRef<Path>) -> Result<Self, AssetError> {
        let trusted_root = canonicalize(trusted_root.as_ref())?;
        Ok(Self {
            trusted_root,
            pets: Vec::new(),
        })
    }

    pub fn load_pet(&mut self, manifest_path: &Path) -> Result<PetAssetHandle, AssetError> {
        let manifest_path = self.resolve_trusted(manifest_path, &self.trusted_root)?;
        let manifest_bytes = read_limited(&manifest_path, MAX_MANIFEST_BYTES)?;
        let manifest: PetManifest = serde_json::from_slice(&manifest_bytes)?;
        validate_manifest(&manifest)?;

        let pet_root = manifest_path
            .parent()
            .ok_or_else(|| AssetError::OutsideTrustedRoot(manifest_path.clone()))?;
        let model_path = self.resolve_trusted(&pet_root.join(&manifest.model), pet_root)?;
        if model_path.extension().and_then(|value| value.to_str()) != Some("glb") {
            return Err(AssetError::ModelMustBeGlb);
        }
        let model_bytes = read_limited(&model_path, MAX_MODEL_BYTES)?;
        let actual_hash = sha256(&model_bytes);
        if !actual_hash.eq_ignore_ascii_case(&manifest.source.sha256) {
            return Err(AssetError::HashMismatch {
                expected: manifest.source.sha256.clone(),
                actual: actual_hash,
            });
        }

        let pet = parse_glb(manifest, &model_bytes)?;
        let handle = PetAssetHandle(self.pets.len());
        self.pets.push(pet);
        Ok(handle)
    }

    pub fn pet(&self, handle: PetAssetHandle) -> Option<&PetAsset> {
        self.pets.get(handle.0)
    }

    fn resolve_trusted(&self, path: &Path, root: &Path) -> Result<PathBuf, AssetError> {
        let resolved = canonicalize(path)?;
        if !resolved.starts_with(root) || !resolved.starts_with(&self.trusted_root) {
            return Err(AssetError::OutsideTrustedRoot(resolved));
        }
        Ok(resolved)
    }
}

pub(crate) fn default_asset_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

pub(crate) fn default_manifest_path() -> PathBuf {
    default_asset_root().join("pets/default/pet.json")
}

fn canonicalize(path: &Path) -> Result<PathBuf, AssetError> {
    path.canonicalize()
        .map_err(|source| AssetError::Canonicalize {
            path: path.to_path_buf(),
            source,
        })
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, AssetError> {
    let metadata = fs::metadata(path).map_err(|source| AssetError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > limit {
        return Err(AssetError::FileTooLarge {
            path: path.to_path_buf(),
            actual: metadata.len(),
            limit,
        });
    }
    fs::read(path).map_err(|source| AssetError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_manifest(manifest: &PetManifest) -> Result<(), AssetError> {
    if manifest.format_version != 1 {
        return Err(AssetError::UnsupportedManifestVersion(
            manifest.format_version,
        ));
    }
    for (name, value) in [
        ("id", manifest.id.as_str()),
        ("name", manifest.name.as_str()),
        ("model", manifest.model.as_str()),
        ("animations.idle", manifest.animations.idle.as_str()),
        ("animations.walk", manifest.animations.walk.as_str()),
        ("source.author", manifest.source.author.as_str()),
        ("source.url", manifest.source.url.as_str()),
        ("source.license", manifest.source.license.as_str()),
        ("source.retrieved_on", manifest.source.retrieved_on.as_str()),
        ("source.sha256", manifest.source.sha256.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(AssetError::MissingManifestField(name));
        }
    }
    if manifest.source.license != "CC0-1.0" {
        return Err(AssetError::MissingManifestField("source.license=CC0-1.0"));
    }
    Ok(())
}

fn parse_glb(manifest: PetManifest, bytes: &[u8]) -> Result<PetAsset, AssetError> {
    let gltf = gltf::Gltf::from_slice(bytes)?;
    if gltf.document.nodes().len() > MAX_NODES {
        return Err(AssetError::LimitExceeded("node count"));
    }
    for buffer in gltf.document.buffers() {
        if let gltf::buffer::Source::Uri(uri) = buffer.source() {
            return Err(AssetError::ExternalUri(uri.to_owned()));
        }
    }
    for image in gltf.document.images() {
        if let gltf::image::Source::Uri { uri, .. } = image.source() {
            return Err(AssetError::ExternalUri(uri.to_owned()));
        }
    }

    let blob = gltf.blob.as_deref().ok_or(AssetError::MissingBinaryBlob)?;
    let rig = read_rig(&gltf.document, blob)?;
    let animations = read_animations(&gltf.document, blob)?;
    let animation_names: HashSet<String> = animations.keys().cloned().collect();
    for (semantic, clip) in [
        ("idle", manifest.animations.idle.as_str()),
        ("walk", manifest.animations.walk.as_str()),
    ] {
        if !animation_names.contains(clip) {
            return Err(AssetError::MissingAnimation {
                semantic,
                clip: clip.to_owned(),
            });
        }
    }

    let mut primitives = Vec::new();
    let mut bounds_min = Vec3::splat(f32::INFINITY);
    let mut bounds_max = Vec3::splat(f32::NEG_INFINITY);
    for scene in gltf.document.scenes() {
        for node in scene.nodes() {
            collect_node(
                node,
                Mat4::IDENTITY,
                blob,
                &mut primitives,
                &mut bounds_min,
                &mut bounds_max,
            )?;
        }
    }
    if primitives.is_empty() {
        return Err(AssetError::LimitExceeded("asset has no mesh primitives"));
    }
    for primitive in &primitives {
        let Some(skin_index) = primitive.skin_index else {
            continue;
        };
        let skin = rig
            .skins
            .get(skin_index)
            .ok_or(AssetError::InvalidSkin("primitive skin index"))?;
        if primitive.vertices.iter().any(|vertex| {
            vertex
                .joints
                .iter()
                .zip(vertex.weights)
                .any(|(&joint, weight)| {
                    weight > f32::EPSILON && usize::from(joint) >= skin.joints.len()
                })
        }) {
            return Err(AssetError::InvalidSkin("vertex joint index"));
        }
    }

    Ok(PetAsset {
        manifest,
        primitives,
        bounds_min,
        bounds_max,
        animation_names,
        animations,
        rig,
    })
}

fn read_rig(document: &gltf::Document, blob: &[u8]) -> Result<RigData, AssetError> {
    let mut nodes: Vec<NodeData> = document
        .nodes()
        .map(|node| {
            let (translation, rotation, scale) = node.transform().decomposed();
            NodeData {
                name: node.name().map(str::to_owned),
                parent: None,
                bind_transform: LocalTransform {
                    translation: Vec3::from(translation),
                    rotation: Quat::from_array(rotation).normalize(),
                    scale: Vec3::from(scale),
                },
            }
        })
        .collect();
    for node in document.nodes() {
        for child in node.children() {
            nodes[child.index()].parent = Some(node.index());
        }
    }

    let mut skins = Vec::new();
    for skin in document.skins() {
        let joints: Vec<usize> = skin.joints().map(|node| node.index()).collect();
        if joints.is_empty() || joints.len() > MAX_JOINTS {
            return Err(AssetError::InvalidSkin("joint count"));
        }
        let inverse_bind_matrices: Vec<Mat4> = skin
            .reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => Some(blob),
                gltf::buffer::Source::Uri(_) => None,
            })
            .read_inverse_bind_matrices()
            .map(|matrices| {
                matrices
                    .map(|matrix| Mat4::from_cols_array_2d(&matrix))
                    .collect()
            })
            .unwrap_or_else(|| vec![Mat4::IDENTITY; joints.len()]);
        if inverse_bind_matrices.len() != joints.len() {
            return Err(AssetError::InvalidSkin("inverse bind matrix count"));
        }
        skins.push(SkinData {
            joints,
            inverse_bind_matrices,
        });
    }
    Ok(RigData { nodes, skins })
}

fn read_animations(
    document: &gltf::Document,
    blob: &[u8],
) -> Result<HashMap<String, AnimationClipData>, AssetError> {
    let mut clips = HashMap::new();
    for animation in document.animations() {
        let Some(name) = animation.name().map(str::to_owned) else {
            continue;
        };
        let mut duration = 0.0_f32;
        let mut channels = Vec::new();
        for channel in animation.channels() {
            let reader = channel.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => Some(blob),
                gltf::buffer::Source::Uri(_) => None,
            });
            let times: Vec<f32> = reader
                .read_inputs()
                .ok_or(AssetError::InvalidAnimation("missing input samples"))?
                .collect();
            if times.is_empty()
                || times.iter().any(|time| !time.is_finite())
                || times.windows(2).any(|pair| pair[0] > pair[1])
            {
                return Err(AssetError::InvalidAnimation("invalid input sample times"));
            }
            duration = duration.max(*times.last().expect("times is non-empty"));
            let interpolation = match channel.sampler().interpolation() {
                gltf::animation::Interpolation::Step => Interpolation::Step,
                gltf::animation::Interpolation::Linear => Interpolation::Linear,
                other => return Err(AssetError::UnsupportedInterpolation(other)),
            };
            let values = match reader
                .read_outputs()
                .ok_or(AssetError::InvalidAnimation("missing output samples"))?
            {
                gltf::animation::util::ReadOutputs::Translations(values) => {
                    ChannelValues::Translations(values.map(Vec3::from).collect())
                }
                gltf::animation::util::ReadOutputs::Rotations(values) => ChannelValues::Rotations(
                    values
                        .into_f32()
                        .map(|value| Quat::from_array(value).normalize())
                        .collect(),
                ),
                gltf::animation::util::ReadOutputs::Scales(values) => {
                    ChannelValues::Scales(values.map(Vec3::from).collect())
                }
                gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => {
                    return Err(AssetError::UnsupportedMorphAnimation);
                }
            };
            let value_count = match &values {
                ChannelValues::Translations(values) | ChannelValues::Scales(values) => values.len(),
                ChannelValues::Rotations(values) => values.len(),
            };
            if value_count != times.len() {
                return Err(AssetError::InvalidAnimation("input/output sample count"));
            }
            channels.push(AnimationChannelData {
                target_node: channel.target().node().index(),
                times,
                interpolation,
                values,
            });
        }
        clips.insert(
            name.clone(),
            AnimationClipData {
                name,
                duration,
                channels,
            },
        );
    }
    Ok(clips)
}

fn collect_node(
    node: gltf::Node<'_>,
    parent_transform: Mat4,
    blob: &[u8],
    output: &mut Vec<MeshPrimitive>,
    bounds_min: &mut Vec3,
    bounds_max: &mut Vec3,
) -> Result<(), AssetError> {
    let local = Mat4::from_cols_array_2d(&node.transform().matrix());
    let world = parent_transform * local;
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if output.len() >= MAX_PRIMITIVES {
                return Err(AssetError::LimitExceeded("primitive count"));
            }
            output.push(read_primitive(
                primitive,
                world,
                node.skin().map(|skin| skin.index()),
                blob,
                bounds_min,
                bounds_max,
            )?);
        }
    }
    for child in node.children() {
        collect_node(child, world, blob, output, bounds_min, bounds_max)?;
    }
    Ok(())
}

fn read_primitive(
    primitive: gltf::Primitive<'_>,
    transform: Mat4,
    skin_index: Option<usize>,
    blob: &[u8],
    bounds_min: &mut Vec3,
    bounds_max: &mut Vec3,
) -> Result<MeshPrimitive, AssetError> {
    if primitive.mode() != gltf::mesh::Mode::Triangles {
        return Err(AssetError::UnsupportedPrimitiveMode(primitive.mode()));
    }
    let reader = primitive.reader(|buffer| match buffer.source() {
        gltf::buffer::Source::Bin => Some(blob),
        gltf::buffer::Source::Uri(_) => None,
    });
    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .ok_or(AssetError::MissingPositions)?
        .collect();
    if positions.len() > MAX_VERTICES {
        return Err(AssetError::LimitExceeded("vertices per primitive"));
    }
    let normals: Vec<[f32; 3]> = reader
        .read_normals()
        .map(Iterator::collect)
        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
    let tex_coords: Vec<[f32; 2]> = reader
        .read_tex_coords(0)
        .map(|values| values.into_f32().collect())
        .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
    let joints: Vec<[u16; 4]> = reader
        .read_joints(0)
        .map(|values| values.into_u16().collect())
        .unwrap_or_else(|| vec![[0, 0, 0, 0]; positions.len()]);
    let weights: Vec<[f32; 4]> = reader
        .read_weights(0)
        .map(|values| values.into_f32().collect())
        .unwrap_or_else(|| vec![[1.0, 0.0, 0.0, 0.0]; positions.len()]);
    if joints.len() != positions.len() || weights.len() != positions.len() {
        return Err(AssetError::InvalidSkin("vertex attribute count"));
    }
    if skin_index.is_some() && (reader.read_joints(0).is_none() || reader.read_weights(0).is_none())
    {
        return Err(AssetError::InvalidSkin("missing vertex joints or weights"));
    }

    let normal_transform = transform.inverse().transpose();
    let vertices: Vec<CpuVertex> = positions
        .into_iter()
        .zip(normals)
        .zip(tex_coords)
        .zip(joints)
        .zip(weights)
        .map(|((((position, normal), tex_coord), joints), weights)| {
            let position = transform.transform_point3(Vec3::from(position));
            let normal = normal_transform
                .transform_vector3(Vec3::from(normal))
                .normalize_or_zero();
            *bounds_min = bounds_min.min(position);
            *bounds_max = bounds_max.max(position);
            CpuVertex {
                position: position.to_array(),
                normal: normal.to_array(),
                tex_coord,
                joints,
                weights,
            }
        })
        .collect();
    let indices: Vec<u32> = reader
        .read_indices()
        .map(|values| values.into_u32().collect())
        .unwrap_or_else(|| (0..vertices.len() as u32).collect());
    if indices.len() > MAX_INDICES {
        return Err(AssetError::LimitExceeded("indices per primitive"));
    }

    let material = primitive.material();
    let pbr = material.pbr_metallic_roughness();
    let base_color_texture = pbr
        .base_color_texture()
        .map(|info| read_texture(info.texture(), blob))
        .transpose()?;
    let alpha_mode = match material.alpha_mode() {
        gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
        gltf::material::AlphaMode::Mask => AlphaMode::Mask(material.alpha_cutoff().unwrap_or(0.5)),
        gltf::material::AlphaMode::Blend => AlphaMode::Blend,
    };

    Ok(MeshPrimitive {
        vertices,
        indices,
        material: MaterialData {
            base_color: pbr.base_color_factor(),
            base_color_texture,
            alpha_mode,
            double_sided: material.double_sided(),
        },
        skin_index,
    })
}

fn read_texture(texture: gltf::Texture<'_>, blob: &[u8]) -> Result<TextureData, AssetError> {
    let bytes = match texture.source().source() {
        gltf::image::Source::View { view, .. } => {
            let start = view.offset();
            &blob[start..start + view.length()]
        }
        gltf::image::Source::Uri { uri, .. } => {
            return Err(AssetError::ExternalUri(uri.to_owned()));
        }
    };
    let image = image::load_from_memory(bytes)?.into_rgba8();
    let sampler = texture.sampler();
    Ok(TextureData {
        width: image.width(),
        height: image.height(),
        rgba8: image.into_raw(),
        sampler: SamplerData {
            mag_nearest: sampler.mag_filter() == Some(gltf::texture::MagFilter::Nearest),
            min_nearest: matches!(
                sampler.min_filter(),
                Some(
                    gltf::texture::MinFilter::Nearest
                        | gltf::texture::MinFilter::NearestMipmapNearest
                        | gltf::texture::MinFilter::NearestMipmapLinear
                )
            ),
            wrap_u: wrap_mode(sampler.wrap_s()),
            wrap_v: wrap_mode(sampler.wrap_t()),
        },
    })
}

fn wrap_mode(mode: gltf::texture::WrappingMode) -> WrapMode {
    match mode {
        gltf::texture::WrappingMode::ClampToEdge => WrapMode::ClampToEdge,
        gltf::texture::WrappingMode::MirroredRepeat => WrapMode::MirroredRepeat,
        gltf::texture::WrappingMode::Repeat => WrapMode::Repeat,
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_asset_loads_with_required_content() {
        let mut manager = AssetManager::new(default_asset_root()).expect("asset root must exist");
        let handle = manager
            .load_pet(&default_manifest_path())
            .expect("default pet must load");
        let pet = manager.pet(handle).expect("loaded handle must resolve");

        assert_eq!(pet.manifest.id, "quaternius_fox");
        assert_eq!(pet.manifest.skeleton.head_joint.as_deref(), Some("Head"));
        let look_at = pet
            .manifest
            .skeleton
            .look_at
            .expect("default pet must configure look-at axes");
        assert_eq!(look_at.yaw_axis, [0.0, 1.0, 0.0]);
        assert_eq!(look_at.pitch_axis, [1.0, 0.0, 0.0]);
        assert!(
            pet.rig
                .nodes
                .iter()
                .any(|node| node.name.as_deref() == Some("Head"))
        );
        assert!(pet.animation_names.contains("Idle"));
        assert!(pet.animation_names.contains("Walk"));
        assert!(!pet.animations["Idle"].channels.is_empty());
        assert_eq!(pet.rig.skins.len(), 1);
        assert!(pet.rig.skins[0].joints.len() <= MAX_JOINTS);
        assert!(
            pet.primitives
                .iter()
                .any(|primitive| primitive.skin_index == Some(0))
        );
        assert!(!pet.primitives.is_empty());
        assert!(pet.bounds_min.cmplt(pet.bounds_max).all());
    }

    #[test]
    fn missing_manifest_is_reported() {
        let root = tempfile::tempdir().expect("temporary root");
        let mut manager = AssetManager::new(root.path()).expect("root must resolve");
        let error = manager
            .load_pet(&root.path().join("missing.json"))
            .expect_err("missing manifest must fail");
        assert!(matches!(error, AssetError::Canonicalize { .. }));
    }

    #[test]
    fn oversized_manifest_is_rejected_before_parsing() {
        let root = tempfile::tempdir().expect("temporary root");
        let manifest_path = root.path().join("pet.json");
        fs::write(&manifest_path, vec![b' '; MAX_MANIFEST_BYTES as usize + 1])
            .expect("fixture write");
        let mut manager = AssetManager::new(root.path()).expect("root must resolve");
        let error = manager
            .load_pet(&manifest_path)
            .expect_err("oversized manifest must fail");
        assert!(matches!(error, AssetError::FileTooLarge { .. }));
    }

    #[test]
    fn model_path_cannot_escape_pet_root() {
        let root = tempfile::tempdir().expect("temporary root");
        fs::write(root.path().join("outside.glb"), b"not glb").expect("fixture write");
        let pet_root = root.path().join("pet");
        fs::create_dir(&pet_root).expect("pet directory");
        let manifest = valid_manifest_json("../outside.glb", "00", "Idle");
        fs::write(pet_root.join("pet.json"), manifest).expect("fixture write");
        let mut manager = AssetManager::new(root.path()).expect("root must resolve");
        let error = manager
            .load_pet(&pet_root.join("pet.json"))
            .expect_err("path escape must fail");
        assert!(matches!(error, AssetError::OutsideTrustedRoot(_)));
    }

    #[test]
    fn corrupt_glb_is_reported_after_hash_validation() {
        let fixture = fixture_with_model(b"not a glb", "Idle");
        let mut manager = AssetManager::new(fixture.path()).expect("root must resolve");
        let error = manager
            .load_pet(&fixture.path().join("pet/pet.json"))
            .expect_err("corrupt GLB must fail");
        assert!(matches!(error, AssetError::InvalidGltf(_)));
    }

    #[test]
    fn missing_animation_mapping_is_reported() {
        let model = fs::read(default_manifest_path().with_file_name("pet.glb"))
            .expect("default model fixture");
        let fixture = fixture_with_model(&model, "MissingIdle");
        let mut manager = AssetManager::new(fixture.path()).expect("root must resolve");
        let error = manager
            .load_pet(&fixture.path().join("pet/pet.json"))
            .expect_err("bad animation mapping must fail");
        assert!(matches!(
            error,
            AssetError::MissingAnimation {
                semantic: "idle",
                ..
            }
        ));
    }

    fn fixture_with_model(model: &[u8], idle: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("temporary root");
        let pet_root = root.path().join("pet");
        fs::create_dir(&pet_root).expect("pet directory");
        fs::write(pet_root.join("pet.glb"), model).expect("fixture model");
        fs::write(
            pet_root.join("pet.json"),
            valid_manifest_json("pet.glb", &sha256(model), idle),
        )
        .expect("fixture manifest");
        root
    }

    fn valid_manifest_json(model: &str, hash: &str, idle: &str) -> String {
        serde_json::json!({
            "format_version": 1,
            "id": "fixture",
            "name": "Fixture",
            "model": model,
            "animations": { "idle": idle, "walk": "Walk" },
            "skeleton": { "head_joint": "Head" },
            "source": {
                "author": "Fixture",
                "url": "https://example.invalid",
                "license": "CC0-1.0",
                "retrieved_on": "2026-08-31",
                "sha256": hash
            }
        })
        .to_string()
    }
}
