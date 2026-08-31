use std::sync::Arc;

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes, WindowId, WindowLevel},
};

use crate::{
    config::AppConfig,
    error::AppError,
    platform::{self, PlatformBackend},
    render::{RenderOutcome, Renderer},
};

pub const PET_WINDOW_LOGICAL_SIZE: f64 = 320.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowSpec {
    pub logical_width: f64,
    pub logical_height: f64,
    pub transparent: bool,
    pub decorations: bool,
    pub resizable: bool,
    pub always_on_top: bool,
}

impl WindowSpec {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            logical_width: PET_WINDOW_LOGICAL_SIZE,
            logical_height: PET_WINDOW_LOGICAL_SIZE,
            transparent: true,
            decorations: false,
            resizable: false,
            always_on_top: config.always_on_top,
        }
    }

    pub fn window_attributes(self) -> WindowAttributes {
        let logical_size = LogicalSize::new(self.logical_width, self.logical_height);
        let window_level = if self.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        };
        let attributes = Window::default_attributes()
            .with_title("DesktopPet")
            .with_inner_size(logical_size)
            .with_min_inner_size(logical_size)
            .with_max_inner_size(logical_size)
            .with_transparent(self.transparent)
            .with_decorations(self.decorations)
            .with_resizable(self.resizable)
            .with_window_level(window_level)
            .with_visible(false);

        platform::configure_window_attributes(attributes)
    }
}

/// Composition root for long-lived application state.
pub struct Application {
    config: AppConfig,
    window: Option<Arc<Window>>,
    _platform_backend: Option<Box<dyn PlatformBackend>>,
    renderer: Option<Renderer>,
    redraw_pending: bool,
    redraw_request_logged: bool,
    has_presented_frame: bool,
    fatal_error: Option<AppError>,
}

impl Application {
    pub fn new(config: AppConfig) -> Result<Self, AppError> {
        config.validate()?;
        Ok(Self {
            config,
            window: None,
            _platform_backend: None,
            renderer: None,
            redraw_pending: false,
            redraw_request_logged: false,
            has_presented_frame: false,
            fatal_error: None,
        })
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn window_spec(&self) -> WindowSpec {
        WindowSpec::from_config(&self.config)
    }

    pub fn run(&mut self) -> Result<(), AppError> {
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::Wait);
        event_loop.run_app(self)?;

        if let Some(error) = self.fatal_error.take() {
            return Err(error);
        }

        Ok(())
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), AppError> {
        let spec = self.window_spec();
        let window = Arc::new(event_loop.create_window(spec.window_attributes())?);
        let mut platform_backend = platform::create_backend(Arc::clone(&window));
        platform_backend
            .set_always_on_top(spec.always_on_top)
            .map_err(|error| AppError::Platform(error.to_string()))?;
        let renderer = pollster::block_on(Renderer::new(Arc::clone(&window)))?;

        let physical_size = window.inner_size();
        tracing::info!(
            window_id = ?window.id(),
            logical_width = spec.logical_width,
            logical_height = spec.logical_height,
            physical_width = physical_size.width,
            physical_height = physical_size.height,
            scale_factor = window.scale_factor(),
            transparent = spec.transparent,
            decorations = spec.decorations,
            resizable = spec.resizable,
            always_on_top = spec.always_on_top,
            "desktop pet window created"
        );

        self.window = Some(window);
        self._platform_backend = Some(platform_backend);
        self.renderer = Some(renderer);
        self.redraw_pending = true;
        self.redraw_request_logged = false;
        if let Some(window) = self.window.as_ref() {
            window.set_visible(true);
        }
        event_loop.set_control_flow(ControlFlow::Poll);
        Ok(())
    }

    fn fail_and_exit(&mut self, event_loop: &ActiveEventLoop, error: AppError) {
        tracing::error!(error = %error, "fatal application lifecycle error");
        self.fatal_error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none()
            && let Err(error) = self.create_window(event_loop)
        {
            self.fail_and_exit(event_loop, error);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                tracing::info!(?window_id, "window close requested");
                event_loop.exit();
            }
            WindowEvent::Destroyed => {
                tracing::debug!(?window_id, "window destroyed");
                self.renderer = None;
                self.window = None;
            }
            WindowEvent::Resized(size) => {
                tracing::debug!(
                    ?window_id,
                    physical_width = size.width,
                    physical_height = size.height,
                    zero_sized = size.width == 0 || size.height == 0,
                    "window resized"
                );
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size);
                }
                self.redraw_pending = size.width > 0 && size.height > 0;
                event_loop.set_control_flow(if self.redraw_pending {
                    ControlFlow::Poll
                } else {
                    ControlFlow::Wait
                });
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                tracing::debug!(?window_id, scale_factor, "window scale factor changed");
                if let (Some(renderer), Some(window)) =
                    (self.renderer.as_mut(), self.window.as_ref())
                {
                    renderer.resize(window.inner_size());
                }
                self.redraw_pending = true;
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            WindowEvent::RedrawRequested => {
                tracing::debug!(?window_id, "processing pending wgpu redraw");
                let Some(renderer) = self.renderer.as_mut() else {
                    return;
                };
                let result = renderer.render();
                match result {
                    Ok(RenderOutcome::Presented) => {
                        self.redraw_pending = false;
                        event_loop.set_control_flow(ControlFlow::Wait);
                        if self.has_presented_frame {
                            tracing::debug!(?window_id, "wgpu frame presented");
                        } else {
                            self.has_presented_frame = true;
                            tracing::info!(?window_id, "first wgpu frame presented");
                        }
                    }
                    Ok(RenderOutcome::SkippedOccluded) => {
                        self.redraw_pending = false;
                        event_loop.set_control_flow(ControlFlow::Wait);
                    }
                    Ok(RenderOutcome::Reconfigured | RenderOutcome::SkippedTimeout) => {
                        self.redraw_pending = true;
                        event_loop.set_control_flow(ControlFlow::Poll);
                        if let Some(window) = self.window.as_ref() {
                            window.request_redraw();
                        }
                    }
                    Err(error) => self.fail_and_exit(event_loop, error.into()),
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && event.physical_key == PhysicalKey::Code(KeyCode::Escape) =>
            {
                tracing::info!(?window_id, "Escape requested application exit");
                event_loop.exit();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.redraw_pending
            && let Some(window) = self.window.as_ref()
        {
            if !self.redraw_request_logged {
                tracing::info!(window_id = ?window.id(), "requesting pending wgpu redraw");
                self.redraw_request_logged = true;
            }
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        tracing::debug!("winit event loop exiting");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::Size;

    #[test]
    fn default_window_spec_matches_phase_one_contract() {
        let app = Application::new(AppConfig::default()).expect("default config must be valid");

        assert_eq!(
            app.window_spec(),
            WindowSpec {
                logical_width: 320.0,
                logical_height: 320.0,
                transparent: true,
                decorations: false,
                resizable: false,
                always_on_top: true,
            }
        );
    }

    #[test]
    fn window_attributes_match_spec() {
        let spec = WindowSpec::from_config(&AppConfig::default());
        let attributes = spec.window_attributes();
        let expected_size = Some(Size::Logical(LogicalSize::new(320.0, 320.0)));

        assert_eq!(attributes.inner_size, expected_size);
        assert_eq!(attributes.min_inner_size, expected_size);
        assert_eq!(attributes.max_inner_size, expected_size);
        assert!(attributes.transparent);
        assert!(!attributes.decorations);
        assert!(!attributes.resizable);
        assert!(!attributes.visible);
        assert_eq!(attributes.window_level, WindowLevel::AlwaysOnTop);
    }

    #[test]
    fn always_on_top_follows_validated_config() {
        let config = AppConfig {
            always_on_top: false,
            ..AppConfig::default()
        };

        let attributes = WindowSpec::from_config(&config).window_attributes();

        assert_eq!(attributes.window_level, WindowLevel::Normal);
    }
}
