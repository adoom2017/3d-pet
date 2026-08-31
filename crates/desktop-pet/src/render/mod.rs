//! wgpu rendering boundary.

use std::sync::{Arc, Mutex};

use thiserror::Error;
use wgpu::{
    Adapter, Color, ColorTargetState, CommandEncoderDescriptor, CompositeAlphaMode,
    CurrentSurfaceTexture, Device, DeviceDescriptor, FragmentState, Instance, InstanceDescriptor,
    LoadOp, Operations, PipelineCompilationOptions, PresentMode, PrimitiveState, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    RequestAdapterOptions, ShaderModuleDescriptor, ShaderSource, StoreOp, Surface,
    SurfaceConfiguration, SurfaceTexture, TextureFormat, TextureViewDescriptor, VertexState,
};
use winit::{dpi::PhysicalSize, window::Window};

const TRIANGLE_SHADER: &str = include_str!("../../../../shaders/triangle.wgsl");
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
        self.configure_surface();
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
        encode_triangle_pass(&mut encoder, &view, &self.pipeline);
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
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
    fn offscreen_triangle_has_opaque_content_and_transparent_clear() {
        pollster::block_on(assert_offscreen_pixels());
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
