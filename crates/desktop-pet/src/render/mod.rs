//! wgpu rendering boundary.

use std::sync::{Arc, Mutex};

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use thiserror::Error;
use wgpu::util::DeviceExt;
use wgpu::{
    Adapter, AddressMode, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingType, BlendState, BufferBindingType, Color, ColorTargetState, CommandEncoderDescriptor,
    CompareFunction, CompositeAlphaMode, CurrentSurfaceTexture, DepthBiasState, DepthStencilState,
    Device, DeviceDescriptor, Face, FilterMode, FragmentState, FrontFace, Instance,
    InstanceDescriptor, LoadOp, MipmapFilterMode, Operations, PipelineCompilationOptions,
    PipelineLayoutDescriptor, PresentMode, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, RequestAdapterOptions, SamplerBindingType, SamplerDescriptor,
    ShaderModuleDescriptor, ShaderSource, ShaderStages, StencilState, StoreOp, Surface,
    SurfaceConfiguration, SurfaceTexture, TextureDescriptor, TextureDimension, TextureFormat,
    TextureSampleType, TextureUsages, TextureViewDescriptor, TextureViewDimension, VertexAttribute,
    VertexBufferLayout, VertexFormat, VertexState, VertexStepMode,
};
use winit::{dpi::PhysicalSize, window::Window};

use crate::asset::{
    AlphaMode, CpuVertex, MAX_JOINTS, PetAsset, SamplerData, TextureData, WrapMode,
};

const TRIANGLE_SHADER: &str = include_str!("../../../../shaders/triangle.wgsl");
const PET_SHADER: &str = include_str!("../../../../shaders/pet.wgsl");
const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;
const PET_VERTEX_ATTRIBUTES: [VertexAttribute; 5] = [
    VertexAttribute {
        format: VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    },
    VertexAttribute {
        format: VertexFormat::Float32x3,
        offset: 12,
        shader_location: 1,
    },
    VertexAttribute {
        format: VertexFormat::Float32x2,
        offset: 24,
        shader_location: 2,
    },
    VertexAttribute {
        format: VertexFormat::Uint16x4,
        offset: 32,
        shader_location: 3,
    },
    VertexAttribute {
        format: VertexFormat::Float32x4,
        offset: 40,
        shader_location: 4,
    },
];
const TRANSPARENT_CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

#[derive(Debug, Error)]
pub enum RendererError {
    #[error("failed to create a presentation surface: {0}")]
    CreateSurface(#[from] wgpu::CreateSurfaceError),

    #[error("failed to select a compatible GPU adapter: {0}")]
    RequestAdapter(#[from] wgpu::RequestAdapterError),

    #[error("failed to create the GPU device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),

    #[error("the selected adapter exposes no compatible surface format")]
    NoSurfaceFormat,

    #[error("the window surface does not support alpha compositing")]
    NoTransparentAlphaMode,

    #[error("surface acquisition produced a GPU validation error")]
    SurfaceValidation,

    #[error("GPU ran out of memory")]
    GpuOutOfMemory,

    #[error("GPU validation failed: {0}")]
    GpuValidation(String),

    #[error("internal GPU failure: {0}")]
    GpuInternal(String),

    #[error("GPU device was lost: {0}")]
    DeviceLost(String),
}

#[derive(Debug)]
enum GpuFault {
    OutOfMemory,
    Validation(String),
    Internal(String),
    DeviceLost(String),
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_projection_model: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
    options: [f32; 4],
}

struct GpuPrimitive {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    material_bind_group: wgpu::BindGroup,
    double_sided: bool,
    skin_binding_index: usize,
    _texture: wgpu::Texture,
    _sampler: wgpu::Sampler,
}

struct GpuSkin {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct GpuPet {
    primitives: Vec<GpuPrimitive>,
    camera_buffer: wgpu::Buffer,
    skins: Vec<GpuSkin>,
    bounds_min: Vec3,
    bounds_max: Vec3,
}

struct DepthTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderOutcome {
    Presented,
    Reconfigured,
    SkippedTimeout,
    SkippedOccluded,
}

pub struct Renderer {
    window: Arc<Window>,
    instance: Instance,
    surface: Surface<'static>,
    _adapter: Adapter,
    device: Device,
    queue: Queue,
    pipeline: RenderPipeline,
    pet_pipeline: RenderPipeline,
    pet_double_sided_pipeline: RenderPipeline,
    camera_bind_group_layout: wgpu::BindGroupLayout,
    material_bind_group_layout: wgpu::BindGroupLayout,
    pet: Option<GpuPet>,
    depth_target: DepthTarget,
    config: SurfaceConfiguration,
    configured: bool,
    gpu_fault: Arc<Mutex<Option<GpuFault>>>,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, RendererError> {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(Arc::clone(&window))?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("DesktopPet device"),
                ..Default::default()
            })
            .await?;

        let gpu_fault = Arc::new(Mutex::new(None));
        let uncaptured_fault = Arc::clone(&gpu_fault);
        device.on_uncaptured_error(Arc::new(move |error| {
            let fault = match error {
                wgpu::Error::OutOfMemory { .. } => GpuFault::OutOfMemory,
                wgpu::Error::Validation { description, .. } => GpuFault::Validation(description),
                wgpu::Error::Internal { description, .. } => GpuFault::Internal(description),
            };
            tracing::error!(?fault, "uncaptured wgpu error");
            *lock_fault(&uncaptured_fault) = Some(fault);
        }));
        let device_lost_fault = Arc::clone(&gpu_fault);
        device.set_device_lost_callback(move |reason, message| {
            tracing::error!(?reason, %message, "wgpu device lost");
            if reason != wgpu::DeviceLostReason::Destroyed {
                *lock_fault(&device_lost_fault) = Some(GpuFault::DeviceLost(message));
            }
        });

        let size = window.inner_size();
        let capabilities = surface.get_capabilities(&adapter);
        let format =
            select_surface_format(&capabilities.formats).ok_or(RendererError::NoSurfaceFormat)?;
        let alpha_mode = select_alpha_mode(&capabilities.alpha_modes)
            .ok_or(RendererError::NoTransparentAlphaMode)?;
        let present_mode = select_present_mode(&capabilities.present_modes);
        let pipeline = create_triangle_pipeline(&device, format);
        let camera_bind_group_layout = create_camera_bind_group_layout(&device);
        let material_bind_group_layout = create_material_bind_group_layout(&device);
        let pet_pipeline = create_pet_pipeline(
            &device,
            format,
            &camera_bind_group_layout,
            &material_bind_group_layout,
            Some(Face::Back),
        );
        let pet_double_sided_pipeline = create_pet_pipeline(
            &device,
            format,
            &camera_bind_group_layout,
            &material_bind_group_layout,
            None,
        );
        let config = SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        let configured = is_non_zero(size);
        if configured {
            surface.configure(&device, &config);
        }
        let depth_target = create_depth_target(&device, config.width, config.height);

        let info = adapter.get_info();
        tracing::info!(
            adapter_name = %info.name,
            backend = ?info.backend,
            device_type = ?info.device_type,
            driver = %info.driver,
            surface_format = ?format,
            present_mode = ?present_mode,
            alpha_mode = ?alpha_mode,
            physical_width = size.width,
            physical_height = size.height,
            "wgpu renderer initialized"
        );

        Ok(Self {
            window,
            instance,
            surface,
            _adapter: adapter,
            device,
            queue,
            pipeline,
            pet_pipeline,
            pet_double_sided_pipeline,
            camera_bind_group_layout,
            material_bind_group_layout,
            pet: None,
            depth_target,
            config,
            configured,
            gpu_fault,
        })
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if !is_non_zero(size) {
            self.configured = false;
            return;
        }

        self.config.width = size.width;
        self.config.height = size.height;
        self.depth_target = create_depth_target(&self.device, size.width, size.height);
        if let Some(pet) = self.pet.as_ref() {
            write_camera_uniform(
                &self.queue,
                &pet.camera_buffer,
                pet.bounds_min,
                pet.bounds_max,
                size.width,
                size.height,
            );
        }
        self.configure_surface();
    }

    pub(crate) fn upload_pet(&mut self, asset: &PetAsset) {
        let camera_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("DesktopPet camera uniform"),
                contents: bytemuck::bytes_of(&camera_uniform(
                    asset.bounds_min,
                    asset.bounds_max,
                    self.config.width,
                    self.config.height,
                )),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let skins = (0..=asset.rig.skins.len())
            .map(|_| create_gpu_skin(&self.device, &self.camera_bind_group_layout, &camera_buffer))
            .collect();
        let primitives = asset
            .primitives
            .iter()
            .map(|primitive| {
                upload_primitive(
                    &self.device,
                    &self.queue,
                    &self.material_bind_group_layout,
                    primitive,
                )
            })
            .collect();

        tracing::info!(
            pet_id = %asset.manifest.id,
            pet_name = %asset.manifest.name,
            primitives = asset.primitives.len(),
            animations = asset.animation_names.len(),
            bounds_min = ?asset.bounds_min,
            bounds_max = ?asset.bounds_max,
            head_joint = ?asset.manifest.skeleton.head_joint,
            "static pet uploaded to GPU"
        );
        self.pet = Some(GpuPet {
            primitives,
            camera_buffer,
            skins,
            bounds_min: asset.bounds_min,
            bounds_max: asset.bounds_max,
        });
    }

    pub(crate) fn update_skinning(&self, skin_matrices: &[Vec<Mat4>]) {
        let Some(pet) = self.pet.as_ref() else {
            return;
        };
        for (gpu_skin, matrices) in pet.skins.iter().skip(1).zip(skin_matrices) {
            let palette = joint_palette(matrices);
            self.queue
                .write_buffer(&gpu_skin.buffer, 0, bytemuck::cast_slice(&palette));
        }
    }

    pub fn render(&mut self) -> Result<RenderOutcome, RendererError> {
        self.check_gpu_fault()?;
        if !self.configured {
            return Ok(RenderOutcome::SkippedOccluded);
        }

        match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) => {
                self.draw_and_present(frame);
                Ok(RenderOutcome::Presented)
            }
            CurrentSurfaceTexture::Suboptimal(frame) => {
                self.draw_and_present(frame);
                self.configure_surface();
                Ok(RenderOutcome::Reconfigured)
            }
            CurrentSurfaceTexture::Timeout => Ok(RenderOutcome::SkippedTimeout),
            CurrentSurfaceTexture::Occluded => Ok(RenderOutcome::SkippedOccluded),
            CurrentSurfaceTexture::Outdated => {
                self.configure_surface();
                Ok(RenderOutcome::Reconfigured)
            }
            CurrentSurfaceTexture::Lost => {
                self.surface = self.instance.create_surface(Arc::clone(&self.window))?;
                self.configure_surface();
                Ok(RenderOutcome::Reconfigured)
            }
            CurrentSurfaceTexture::Validation => Err(RendererError::SurfaceValidation),
        }
    }

    fn configure_surface(&mut self) {
        self.surface.configure(&self.device, &self.config);
        self.configured = true;
    }

    fn check_gpu_fault(&self) -> Result<(), RendererError> {
        match lock_fault(&self.gpu_fault).take() {
            None => Ok(()),
            Some(GpuFault::OutOfMemory) => Err(RendererError::GpuOutOfMemory),
            Some(GpuFault::Validation(message)) => Err(RendererError::GpuValidation(message)),
            Some(GpuFault::Internal(message)) => Err(RendererError::GpuInternal(message)),
            Some(GpuFault::DeviceLost(message)) => Err(RendererError::DeviceLost(message)),
        }
    }

    fn draw_and_present(&self, frame: SurfaceTexture) {
        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("DesktopPet frame encoder"),
            });
        if let Some(pet) = self.pet.as_ref() {
            encode_pet_pass(
                &mut encoder,
                &view,
                &self.depth_target.view,
                &self.pet_pipeline,
                &self.pet_double_sided_pipeline,
                pet,
            );
        } else {
            encode_triangle_pass(&mut encoder, &view, &self.pipeline);
        }
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.queue.present(frame);
    }
}

fn create_camera_bind_group_layout(device: &Device) -> BindGroupLayout {
    device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("DesktopPet camera bind group layout"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

fn create_material_bind_group_layout(device: &Device) -> BindGroupLayout {
    device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("DesktopPet material bind group layout"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

fn create_pet_pipeline(
    device: &Device,
    format: TextureFormat,
    camera_layout: &BindGroupLayout,
    material_layout: &BindGroupLayout,
    cull_mode: Option<Face>,
) -> RenderPipeline {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("DesktopPet pet shader"),
        source: ShaderSource::Wgsl(PET_SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("DesktopPet pet pipeline layout"),
        bind_group_layouts: &[Some(camera_layout), Some(material_layout)],
        immediate_size: 0,
    });
    let vertex_layout = VertexBufferLayout {
        array_stride: std::mem::size_of::<CpuVertex>() as wgpu::BufferAddress,
        step_mode: VertexStepMode::Vertex,
        attributes: &PET_VERTEX_ATTRIBUTES,
    };

    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("DesktopPet pet pipeline"),
        layout: Some(&layout),
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            buffers: &[Some(vertex_layout)],
        },
        primitive: PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: FrontFace::Ccw,
            cull_mode,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::Less),
            stencil: StencilState::default(),
            bias: DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            targets: &[Some(ColorTargetState {
                format,
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn create_depth_target(device: &Device, width: u32, height: u32) -> DepthTarget {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("DesktopPet depth target"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    DepthTarget {
        _texture: texture,
        view,
    }
}

fn camera_uniform(bounds_min: Vec3, bounds_max: Vec3, width: u32, height: u32) -> CameraUniform {
    let center = (bounds_min + bounds_max) * 0.5;
    let radius = ((bounds_max - bounds_min).length() * 0.5).max(0.001);
    let aspect = width.max(1) as f32 / height.max(1) as f32;
    let vertical_fov = 35.0_f32.to_radians();
    let limiting_half_fov = if aspect < 1.0 {
        (vertical_fov * 0.5).tan().mul_add(aspect, 0.0).atan()
    } else {
        vertical_fov * 0.5
    };
    let distance = radius / limiting_half_fov.sin() * 1.15;
    let view_direction = Vec3::new(1.8, 1.0, 3.2).normalize();
    let eye = center + view_direction * distance;
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let near = (distance - radius * 1.5).max(0.01);
    let far = distance + radius * 2.5;
    let projection = Mat4::perspective_rh(vertical_fov, aspect, near, far);
    CameraUniform {
        view_projection_model: (projection * view).to_cols_array_2d(),
    }
}

fn write_camera_uniform(
    queue: &Queue,
    buffer: &wgpu::Buffer,
    bounds_min: Vec3,
    bounds_max: Vec3,
    width: u32,
    height: u32,
) {
    queue.write_buffer(
        buffer,
        0,
        bytemuck::bytes_of(&camera_uniform(bounds_min, bounds_max, width, height)),
    );
}

fn create_gpu_skin(
    device: &Device,
    layout: &BindGroupLayout,
    camera_buffer: &wgpu::Buffer,
) -> GpuSkin {
    let palette = joint_palette(&[]);
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("DesktopPet joint palette"),
        contents: bytemuck::cast_slice(&palette),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("DesktopPet camera and skin bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buffer.as_entire_binding(),
            },
        ],
    });
    GpuSkin { buffer, bind_group }
}

fn joint_palette(matrices: &[Mat4]) -> Vec<[[f32; 4]; 4]> {
    let mut palette = vec![Mat4::IDENTITY.to_cols_array_2d(); MAX_JOINTS];
    for (target, matrix) in palette.iter_mut().zip(matrices) {
        *target = matrix.to_cols_array_2d();
    }
    palette
}

fn upload_primitive(
    device: &Device,
    queue: &Queue,
    material_layout: &BindGroupLayout,
    primitive: &crate::asset::MeshPrimitive,
) -> GpuPrimitive {
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("DesktopPet pet vertex buffer"),
        contents: bytemuck::cast_slice(&primitive.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("DesktopPet pet index buffer"),
        contents: bytemuck::cast_slice(&primitive.indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let material_uniform =
        material_uniform(primitive.material.base_color, primitive.material.alpha_mode);
    let material_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("DesktopPet material uniform"),
        contents: bytemuck::bytes_of(&material_uniform),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let texture_data = primitive.material.base_color_texture.as_ref();
    let (texture, sampler) = upload_texture(device, queue, texture_data);
    let texture_view = texture.create_view(&TextureViewDescriptor::default());
    let material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("DesktopPet material bind group"),
        layout: material_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: material_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    GpuPrimitive {
        vertex_buffer,
        index_buffer,
        index_count: primitive.indices.len() as u32,
        material_bind_group,
        double_sided: primitive.material.double_sided,
        skin_binding_index: primitive.skin_index.map_or(0, |index| index + 1),
        _texture: texture,
        _sampler: sampler,
    }
}

fn material_uniform(base_color: [f32; 4], alpha_mode: AlphaMode) -> MaterialUniform {
    let (mode, cutoff) = match alpha_mode {
        AlphaMode::Opaque => (0.0, 0.0),
        AlphaMode::Mask(cutoff) => (1.0, cutoff),
        AlphaMode::Blend => (2.0, 0.0),
    };
    MaterialUniform {
        base_color,
        options: [mode, cutoff, 0.0, 0.0],
    }
}

fn upload_texture(
    device: &Device,
    queue: &Queue,
    texture_data: Option<&TextureData>,
) -> (wgpu::Texture, wgpu::Sampler) {
    let fallback = [255_u8; 4];
    let (width, height, bytes, sampler_data) = match texture_data {
        Some(texture) => (
            texture.width,
            texture.height,
            texture.rgba8.as_slice(),
            texture.sampler,
        ),
        None => (1, 1, fallback.as_slice(), default_sampler_data()),
    };
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("DesktopPet base color texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytes,
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("DesktopPet base color sampler"),
        address_mode_u: address_mode(sampler_data.wrap_u),
        address_mode_v: address_mode(sampler_data.wrap_v),
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: filter_mode(sampler_data.mag_nearest),
        min_filter: filter_mode(sampler_data.min_nearest),
        mipmap_filter: mipmap_filter_mode(sampler_data.min_nearest),
        ..Default::default()
    });
    (texture, sampler)
}

fn default_sampler_data() -> SamplerData {
    SamplerData {
        mag_nearest: false,
        min_nearest: false,
        wrap_u: WrapMode::ClampToEdge,
        wrap_v: WrapMode::ClampToEdge,
    }
}

fn address_mode(mode: WrapMode) -> AddressMode {
    match mode {
        WrapMode::ClampToEdge => AddressMode::ClampToEdge,
        WrapMode::MirroredRepeat => AddressMode::MirrorRepeat,
        WrapMode::Repeat => AddressMode::Repeat,
    }
}

fn filter_mode(nearest: bool) -> FilterMode {
    if nearest {
        FilterMode::Nearest
    } else {
        FilterMode::Linear
    }
}

fn mipmap_filter_mode(nearest: bool) -> MipmapFilterMode {
    if nearest {
        MipmapFilterMode::Nearest
    } else {
        MipmapFilterMode::Linear
    }
}

fn encode_pet_pass(
    encoder: &mut wgpu::CommandEncoder,
    color_view: &wgpu::TextureView,
    depth_view: &wgpu::TextureView,
    pipeline: &RenderPipeline,
    double_sided_pipeline: &RenderPipeline,
    pet: &GpuPet,
) {
    let color_attachment = Some(RenderPassColorAttachment {
        view: color_view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(TRANSPARENT_CLEAR),
            store: StoreOp::Store,
        },
    });
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("DesktopPet static pet pass"),
        color_attachments: &[color_attachment],
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: depth_view,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(1.0),
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    });
    for primitive in &pet.primitives {
        pass.set_pipeline(if primitive.double_sided {
            double_sided_pipeline
        } else {
            pipeline
        });
        pass.set_bind_group(0, &pet.skins[primitive.skin_binding_index].bind_group, &[]);
        pass.set_bind_group(1, &primitive.material_bind_group, &[]);
        pass.set_vertex_buffer(0, primitive.vertex_buffer.slice(..));
        pass.set_index_buffer(primitive.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..primitive.index_count, 0, 0..1);
    }
}

fn lock_fault(fault: &Mutex<Option<GpuFault>>) -> std::sync::MutexGuard<'_, Option<GpuFault>> {
    fault
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn is_non_zero(size: PhysicalSize<u32>) -> bool {
    size.width > 0 && size.height > 0
}

fn select_surface_format(formats: &[TextureFormat]) -> Option<TextureFormat> {
    formats
        .iter()
        .copied()
        .find(TextureFormat::is_srgb)
        .or_else(|| formats.first().copied())
}

fn select_alpha_mode(modes: &[CompositeAlphaMode]) -> Option<CompositeAlphaMode> {
    [
        CompositeAlphaMode::PreMultiplied,
        CompositeAlphaMode::PostMultiplied,
        CompositeAlphaMode::Inherit,
    ]
    .into_iter()
    .find(|candidate| modes.contains(candidate))
}

fn select_present_mode(modes: &[PresentMode]) -> PresentMode {
    if modes.contains(&PresentMode::Fifo) {
        PresentMode::Fifo
    } else {
        modes.first().copied().unwrap_or(PresentMode::AutoVsync)
    }
}

fn create_triangle_pipeline(device: &Device, format: TextureFormat) -> RenderPipeline {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("DesktopPet triangle shader"),
        source: ShaderSource::Wgsl(TRIANGLE_SHADER.into()),
    });
    let target = Some(ColorTargetState {
        format,
        blend: Some(wgpu::BlendState::REPLACE),
        write_mask: wgpu::ColorWrites::ALL,
    });

    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("DesktopPet triangle pipeline"),
        layout: None,
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: PipelineCompilationOptions::default(),
            targets: &[target],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn encode_triangle_pass(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    pipeline: &RenderPipeline,
) {
    let color_attachment = Some(RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(TRANSPARENT_CLEAR),
            store: StoreOp::Store,
        },
    });
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("DesktopPet triangle pass"),
        color_attachments: &[color_attachment],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn zero_sized_surfaces_are_not_configurable() {
        assert!(!is_non_zero(PhysicalSize::new(0, 320)));
        assert!(!is_non_zero(PhysicalSize::new(320, 0)));
        assert!(is_non_zero(PhysicalSize::new(640, 640)));
    }

    #[test]
    fn transparent_alpha_mode_is_required_and_preferred() {
        assert_eq!(select_alpha_mode(&[CompositeAlphaMode::Opaque]), None);
        assert_eq!(
            select_alpha_mode(&[
                CompositeAlphaMode::PostMultiplied,
                CompositeAlphaMode::PreMultiplied,
            ]),
            Some(CompositeAlphaMode::PreMultiplied)
        );
    }

    #[test]
    fn camera_matrix_is_finite_for_normal_and_degenerate_bounds() {
        for uniform in [
            camera_uniform(Vec3::splat(-1.0), Vec3::splat(1.0), 320, 320),
            camera_uniform(Vec3::ZERO, Vec3::ZERO, 0, 0),
            camera_uniform(Vec3::new(-4.0, -1.0, -1.0), Vec3::ONE, 160, 320),
        ] {
            assert!(
                uniform
                    .view_projection_model
                    .into_iter()
                    .flatten()
                    .all(f32::is_finite)
            );
        }
    }

    #[test]
    fn material_alpha_modes_have_stable_shader_values() {
        assert_eq!(
            material_uniform([1.0; 4], AlphaMode::Opaque).options[0],
            0.0
        );
        assert_eq!(
            material_uniform([1.0; 4], AlphaMode::Mask(0.37)).options[..2],
            [1.0, 0.37]
        );
        assert_eq!(material_uniform([1.0; 4], AlphaMode::Blend).options[0], 2.0);
    }

    #[test]
    fn offscreen_triangle_has_opaque_content_and_transparent_clear() {
        pollster::block_on(assert_offscreen_pixels());
    }

    #[test]
    fn offscreen_default_pet_has_visible_content_and_transparent_clear() {
        pollster::block_on(assert_offscreen_pet_pixels());
    }

    async fn assert_offscreen_pet_pixels() {
        const WIDTH: u32 = 128;
        const HEIGHT: u32 = 128;
        const BYTES_PER_ROW: u32 = WIDTH * 4;

        let instance = Instance::default();
        let adapter = match instance
            .request_adapter(&RequestAdapterOptions::default())
            .await
        {
            Ok(adapter) => adapter,
            Err(error) => {
                eprintln!("skipping offscreen pet smoke: no adapter: {error}");
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("offscreen pet test device"),
                ..Default::default()
            })
            .await
            .expect("adapter must create a pet test device");
        let mut assets = crate::asset::AssetManager::new(crate::asset::default_asset_root())
            .expect("asset root must exist");
        let handle = assets
            .load_pet(&crate::asset::default_manifest_path())
            .expect("default pet must load");
        let asset = assets.pet(handle).expect("loaded pet must resolve");
        let camera_layout = create_camera_bind_group_layout(&device);
        let material_layout = create_material_bind_group_layout(&device);
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("offscreen pet camera"),
            contents: bytemuck::bytes_of(&camera_uniform(
                asset.bounds_min,
                asset.bounds_max,
                WIDTH,
                HEIGHT,
            )),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let skins: Vec<GpuSkin> = (0..=asset.rig.skins.len())
            .map(|_| create_gpu_skin(&device, &camera_layout, &camera_buffer))
            .collect();
        let controller = crate::animation::AnimationController::idle(asset)
            .expect("default Idle animation must initialize");
        for (gpu_skin, matrices) in skins.iter().skip(1).zip(controller.skin_matrices()) {
            let palette = joint_palette(matrices);
            queue.write_buffer(&gpu_skin.buffer, 0, bytemuck::cast_slice(&palette));
        }
        let pet = GpuPet {
            primitives: asset
                .primitives
                .iter()
                .map(|primitive| upload_primitive(&device, &queue, &material_layout, primitive))
                .collect(),
            camera_buffer,
            skins,
            bounds_min: asset.bounds_min,
            bounds_max: asset.bounds_max,
        };
        let pipeline = create_pet_pipeline(
            &device,
            TextureFormat::Rgba8Unorm,
            &camera_layout,
            &material_layout,
            Some(Face::Back),
        );
        let double_sided_pipeline = create_pet_pipeline(
            &device,
            TextureFormat::Rgba8Unorm,
            &camera_layout,
            &material_layout,
            None,
        );
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("offscreen pet target"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = create_depth_target(&device, WIDTH, HEIGHT);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen pet readback"),
            size: u64::from(BYTES_PER_ROW * HEIGHT),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("offscreen pet encoder"),
        });
        encode_pet_pass(
            &mut encoder,
            &view,
            &depth.view,
            &pipeline,
            &double_sided_pipeline,
            &pet,
        );
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(BYTES_PER_ROW),
                    rows_per_image: Some(HEIGHT),
                },
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);

        let (sender, receiver) = mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).expect("mapping receiver must exist");
            });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU polling must succeed");
        receiver
            .recv()
            .expect("mapping callback must run")
            .expect("readback mapping must succeed");
        let bytes = readback
            .slice(..)
            .get_mapped_range()
            .expect("mapped readback range must be accessible");
        let visible_pixels = bytes.chunks_exact(4).filter(|pixel| pixel[3] > 0).count();
        let transparent_pixels = bytes.chunks_exact(4).filter(|pixel| pixel[3] == 0).count();
        assert!(visible_pixels > 100, "pet must produce visible pixels");
        assert!(
            transparent_pixels > 100,
            "clear area must remain transparent"
        );
    }

    async fn assert_offscreen_pixels() {
        const WIDTH: u32 = 64;
        const HEIGHT: u32 = 64;
        const BYTES_PER_ROW: u32 = 256;

        let instance = Instance::default();
        let adapter = match instance
            .request_adapter(&RequestAdapterOptions::default())
            .await
        {
            Ok(adapter) => adapter,
            Err(error) => {
                eprintln!("skipping offscreen renderer smoke: no adapter: {error}");
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("offscreen test device"),
                ..Default::default()
            })
            .await
            .expect("adapter must create a test device");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen triangle target"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen triangle readback"),
            size: u64::from(BYTES_PER_ROW * HEIGHT),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let pipeline = create_triangle_pipeline(&device, TextureFormat::Rgba8Unorm);
        let view = texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("offscreen triangle encoder"),
        });
        encode_triangle_pass(&mut encoder, &view, &pipeline);
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(BYTES_PER_ROW),
                    rows_per_image: Some(HEIGHT),
                },
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);

        let (sender, receiver) = mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).expect("mapping receiver must exist");
            });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU polling must succeed");
        receiver
            .recv()
            .expect("mapping callback must run")
            .expect("readback mapping must succeed");

        let bytes = readback
            .slice(..)
            .get_mapped_range()
            .expect("mapped readback range must be accessible");
        let corner_alpha = bytes[3];
        let center_offset = (HEIGHT / 2 * BYTES_PER_ROW + WIDTH / 2 * 4) as usize;
        let center_alpha = bytes[center_offset + 3];
        assert_eq!(corner_alpha, 0, "clear pixels must preserve alpha zero");
        assert_eq!(center_alpha, 255, "triangle center must be opaque");
    }
}
