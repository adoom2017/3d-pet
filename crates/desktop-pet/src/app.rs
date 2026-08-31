use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes, WindowId, WindowLevel},
};

use crate::{
    animation::{AnimationController, AnimationRequest},
    asset::{AssetManager, default_asset_root, default_manifest_path},
    config::AppConfig,
    display::{
        DisplayManager, LogicalSize as DesktopLogicalSize, PhysicalSize as DesktopPhysicalSize,
    },
    error::AppError,
    input::MouseState,
    interaction::{
        DEFAULT_HIT_PADDING_LOGICAL, HitRegion, InteractionAction, InteractionController,
        RectHitRegion,
    },
    pet::{
        BehaviorStateMachine, BrainConfig, HorizontalDirection, MonotonicClock, MovementController,
        PetAnimationIntent, PetBrain, PetIntent, PetObservation, PetState, PetStateMachine,
        SimulationClock, SplitMix64, StateTransition, TransitionContext, TransitionOutcome,
        WanderingPetBrain,
    },
    platform::{self, PlatformBackend},
    render::{RenderOutcome, Renderer},
    time::{FIXED_UPDATE_INTERVAL, FixedStepAccumulator},
};

pub const PET_WINDOW_LOGICAL_SIZE: f64 = 320.0;
const DEFAULT_BRAIN_SEED: u64 = 0x3d50_6574_2026_0831;
const MONITOR_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

fn log_monitor_snapshot(display_manager: &DisplayManager) {
    if display_manager.monitors().is_empty() {
        tracing::warn!("platform returned no monitors; desktop boundary constraints are disabled");
        return;
    }
    for monitor in display_manager.monitors() {
        tracing::info!(
            monitor_id = monitor.id.0,
            x = monitor.work_area_origin.x,
            y = monitor.work_area_origin.y,
            width = monitor.work_area_size.width,
            height = monitor.work_area_size.height,
            scale_factor = monitor.scale_factor,
            primary = monitor.is_primary,
            "monitor work-area snapshot"
        );
    }
}

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
    platform_backend: Option<Box<dyn PlatformBackend>>,
    renderer: Option<Renderer>,
    _asset_manager: Option<AssetManager>,
    animation: Option<AnimationController>,
    movement: Option<MovementController>,
    display_manager: Option<DisplayManager>,
    mouse_state: MouseState,
    interaction: InteractionController,
    click_through_active: Option<bool>,
    brain: Option<WanderingPetBrain>,
    brain_rng: SplitMix64,
    state_machine: Option<BehaviorStateMachine>,
    simulation_clock: SimulationClock,
    monitor_refresh_elapsed: Duration,
    interaction_epoch: Option<Instant>,
    last_logic_update: Option<Instant>,
    fixed_steps: FixedStepAccumulator,
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
            platform_backend: None,
            renderer: None,
            _asset_manager: None,
            animation: None,
            movement: None,
            display_manager: None,
            mouse_state: MouseState::default(),
            interaction: InteractionController::default(),
            click_through_active: None,
            brain: None,
            brain_rng: SplitMix64::seeded(DEFAULT_BRAIN_SEED),
            state_machine: None,
            simulation_clock: SimulationClock::default(),
            monitor_refresh_elapsed: Duration::ZERO,
            interaction_epoch: None,
            last_logic_update: None,
            fixed_steps: FixedStepAccumulator::default(),
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
        tracing::debug!("creating winit event loop");
        let event_loop = EventLoop::new()?;
        tracing::debug!("winit event loop created");
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
        let initial_position = platform_backend
            .window_position()
            .map_err(|error| AppError::Platform(error.to_string()))?;
        let monitors = platform_backend
            .monitors()
            .map_err(|error| AppError::Platform(error.to_string()))?;
        let display_manager = DisplayManager::new(monitors);
        let window_size = DesktopLogicalSize::new(spec.logical_width, spec.logical_height);
        let initial_position = display_manager.constrain_position(initial_position, window_size);
        platform_backend
            .set_window_position(initial_position)
            .map_err(|error| AppError::Platform(error.to_string()))?;
        log_monitor_snapshot(&display_manager);
        let mut renderer = pollster::block_on(Renderer::new(Arc::clone(&window)))?;
        renderer.set_pet_scale(self.config.scale);
        let mut asset_manager = AssetManager::new(default_asset_root())?;
        let pet_handle = asset_manager.load_pet(&default_manifest_path())?;
        let pet = asset_manager
            .pet(pet_handle)
            .ok_or(crate::asset::AssetError::InvalidHandle)?;
        let mut animation = AnimationController::new(pet)
            .map_err(|error| AppError::Animation(error.to_string()))?;
        animation
            .set_playback_speed(1.0)
            .map_err(|error| AppError::Animation(error.to_string()))?;
        tracing::info!(idle_clip = animation.clip_name(), "Idle animation selected");
        renderer.upload_pet(pet);
        renderer.update_skinning(animation.skin_matrices());

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
        self.platform_backend = Some(platform_backend);
        self.renderer = Some(renderer);
        self._asset_manager = Some(asset_manager);
        self.animation = Some(animation);
        self.movement = Some(MovementController::new(initial_position));
        self.display_manager = Some(display_manager);
        self.mouse_state = MouseState::default();
        self.interaction = InteractionController::default();
        self.click_through_active = None;
        self.brain = Some(
            WanderingPetBrain::new(BrainConfig::default())
                .map_err(|error| AppError::Behavior(error.to_string()))?,
        );
        self.brain_rng = SplitMix64::seeded(DEFAULT_BRAIN_SEED);
        self.state_machine = Some(BehaviorStateMachine::default());
        self.simulation_clock = SimulationClock::default();
        self.monitor_refresh_elapsed = Duration::ZERO;
        self.fixed_steps = FixedStepAccumulator::default();
        let now = Instant::now();
        self.interaction_epoch = Some(now);
        self.last_logic_update = Some(now);
        self.redraw_pending = true;
        self.redraw_request_logged = false;
        self.refresh_pointer_from_platform()?;
        if let Some(window) = self.window.as_ref() {
            window.set_visible(true);
        }
        event_loop.set_control_flow(ControlFlow::Poll);
        Ok(())
    }

    fn fail_and_exit(&mut self, event_loop: &ActiveEventLoop, error: AppError) {
        tracing::error!(error = %error, "fatal application lifecycle error");
        self.restore_mouse_handling();
        self.fatal_error = Some(error);
        event_loop.exit();
    }

    fn advance_movement(&mut self) -> Result<(), AppError> {
        let state = self.state_machine.as_ref().map(PetStateMachine::state);
        let direction = self.state_machine.as_ref().map(PetStateMachine::facing);
        match (state, direction) {
            (Some(PetState::Walking), Some(direction)) => self.advance_walking(direction),
            (Some(PetState::Falling), _) => self.advance_falling(),
            _ => Ok(()),
        }
    }

    fn advance_walking(&mut self, direction: HorizontalDirection) -> Result<(), AppError> {
        let Some(movement) = self.movement.as_mut() else {
            return Ok(());
        };
        let display_manager = self
            .display_manager
            .as_ref()
            .ok_or_else(|| AppError::Platform("display manager is unavailable".to_owned()))?;
        let platform_backend = self
            .platform_backend
            .as_mut()
            .ok_or_else(|| AppError::Platform("platform backend is unavailable".to_owned()))?;
        let boundary = movement.try_advance(FIXED_UPDATE_INTERVAL, |current, proposed| {
            let boundary = display_manager.constrain_horizontal_move(
                current,
                proposed,
                DesktopLogicalSize::new(PET_WINDOW_LOGICAL_SIZE, PET_WINDOW_LOGICAL_SIZE),
                direction,
            );
            platform_backend
                .set_window_position(boundary.position)
                .map_err(|error| {
                    AppError::Platform(format!(
                        "failed to move the window to logical position ({:.3}, {:.3}): {error}",
                        boundary.position.x, boundary.position.y
                    ))
                })?;
            Ok::<_, AppError>((boundary.position, boundary))
        })?;
        if let Some(boundary) = boundary {
            self.mouse_state.update_window_origin(boundary.position);
            self.update_pointer_hit();
            self.sync_click_through()?;
        }
        if let Some(Some(turn)) = boundary.map(|boundary| boundary.turn) {
            tracing::info!(?turn, "desktop work-area boundary requested a turn");
            self.apply_pet_intent(
                PetIntent::Turn { direction: turn },
                TransitionContext::BRAIN,
            );
        }
        Ok(())
    }

    fn advance_falling(&mut self) -> Result<(), AppError> {
        let window_size = DesktopLogicalSize::new(PET_WINDOW_LOGICAL_SIZE, PET_WINDOW_LOGICAL_SIZE);
        let movement = self
            .movement
            .as_mut()
            .ok_or_else(|| AppError::Platform("movement controller is unavailable".to_owned()))?;
        let ground_y = self
            .display_manager
            .as_ref()
            .ok_or_else(|| AppError::Platform("display manager is unavailable".to_owned()))?
            .ground_y(movement.position(), window_size)
            .ok_or_else(|| AppError::Platform("no active monitor is available".to_owned()))?;
        let platform_backend = self
            .platform_backend
            .as_mut()
            .ok_or_else(|| AppError::Platform("platform backend is unavailable".to_owned()))?;
        let advanced = movement.try_advance_falling(
            FIXED_UPDATE_INTERVAL,
            ground_y,
            |proposed| {
                platform_backend
                    .set_window_position(proposed)
                    .map_err(|error| {
                        AppError::Platform(format!(
                            "failed to move the falling window to logical position ({:.3}, {:.3}): {error}",
                            proposed.x, proposed.y
                        ))
                    })?;
                Ok::<_, AppError>((proposed, proposed))
            },
        )?;
        let Some((position, landed)) = advanced else {
            return Ok(());
        };
        self.mouse_state.update_window_origin(position);
        self.update_pointer_hit();
        self.sync_click_through()?;
        if landed {
            tracing::info!(?position, ground_y, "pet reached the work-area ground");
            self.apply_pet_intent(PetIntent::Landed, TransitionContext::PHYSICS);
        }
        Ok(())
    }

    fn refresh_monitors(&mut self) -> Result<(), AppError> {
        let monitors = self
            .platform_backend
            .as_ref()
            .ok_or_else(|| AppError::Platform("platform backend is unavailable".to_owned()))?
            .monitors()
            .map_err(|error| AppError::Platform(error.to_string()))?;
        let display_manager = self
            .display_manager
            .as_mut()
            .ok_or_else(|| AppError::Platform("display manager is unavailable".to_owned()))?;
        if display_manager.refresh(monitors) {
            tracing::info!("monitor topology or work area changed");
            log_monitor_snapshot(display_manager);
        }
        Ok(())
    }

    fn update_pointer_hit(&mut self) {
        let region = self
            .renderer
            .as_ref()
            .and_then(|renderer| {
                let scale_factor = self.window.as_ref()?.scale_factor();
                renderer.pet_projection(scale_factor)
            })
            .and_then(|projection| {
                RectHitRegion::from_pet_projection(projection, DEFAULT_HIT_PADDING_LOGICAL)
            });
        let update = self.interaction.update_hit(
            self.mouse_state.window_logical_position,
            region.as_ref().map(|region| region as &dyn HitRegion),
        );
        if update.changed {
            tracing::info!(
                hit = update.hit,
                window_logical = ?self.mouse_state.window_logical_position,
                desktop = ?self.mouse_state.desktop_position,
                region = ?region,
                "pet pointer hit changed"
            );
        } else {
            tracing::trace!(
                hit = update.hit,
                window_logical = ?self.mouse_state.window_logical_position,
                "pet pointer hit evaluated"
            );
        }
    }

    fn sync_click_through(&mut self) -> Result<(), AppError> {
        let required = self.interaction.click_through_required();
        self.platform_backend
            .as_mut()
            .ok_or_else(|| AppError::Platform("platform backend is unavailable".to_owned()))?
            .set_click_through(required)
            .map_err(|error| AppError::Platform(error.to_string()))?;
        if self.click_through_active != Some(required) {
            tracing::info!(enabled = required, "platform mouse click-through changed");
            self.click_through_active = Some(required);
        }
        Ok(())
    }

    fn refresh_pointer_from_platform(&mut self) -> Result<(), AppError> {
        let desktop = self
            .platform_backend
            .as_ref()
            .ok_or_else(|| AppError::Platform("platform backend is unavailable".to_owned()))?
            .cursor_position()
            .map_err(|error| AppError::Platform(error.to_string()))?;
        if let Some(desktop) = desktop {
            let window_origin = self
                .movement
                .as_ref()
                .map(MovementController::position)
                .unwrap_or_default();
            self.mouse_state
                .update_cursor_desktop(desktop, window_origin);
        } else {
            self.mouse_state.clear_cursor();
        }
        let action = self.interaction.pointer_moved(
            self.mouse_state.desktop_position,
            self.interaction_timestamp(),
        );
        self.apply_interaction_action(action)?;
        self.update_pointer_hit();
        self.sync_click_through()
    }

    fn interaction_timestamp(&self) -> Duration {
        self.interaction_epoch
            .map(|epoch| Instant::now().saturating_duration_since(epoch))
            .unwrap_or_else(|| self.simulation_clock.now())
    }

    fn restore_mouse_handling(&mut self) {
        let _ = self.interaction.cancel_pointer();
        self.mouse_state.set_left_pressed(false);
        if let Some(platform_backend) = self.platform_backend.as_mut()
            && let Err(error) = platform_backend.set_click_through(false)
        {
            tracing::warn!(%error, "failed to restore native mouse handling during shutdown");
        }
        self.click_through_active = Some(false);
    }

    fn apply_interaction_action(&mut self, action: InteractionAction) -> Result<(), AppError> {
        match action {
            InteractionAction::None => {}
            InteractionAction::ClickPet => {
                tracing::info!("complete pet click submitted as interaction intent");
                self.apply_pet_intent(PetIntent::Interact, TransitionContext::EXPLICIT);
            }
            InteractionAction::BeginDrag {
                offset,
                desktop_position,
            } => {
                tracing::info!(?offset, ?desktop_position, "pet drag started");
                self.apply_pet_intent(PetIntent::BeginDrag, TransitionContext::DRAG);
                if let Some(movement) = self.movement.as_mut() {
                    movement.begin_drag();
                }
                self.move_drag_window(desktop_position)?;
            }
            InteractionAction::MoveDrag { desktop_position } => {
                self.move_drag_window(desktop_position)?;
            }
            InteractionAction::EndDrag { release_velocity } => {
                tracing::info!(?release_velocity, "pet drag ended");
                self.apply_pet_intent(PetIntent::EndDrag, TransitionContext::DRAG);
                if let Some(movement) = self.movement.as_mut() {
                    movement.finish_drag(release_velocity);
                }
            }
        }
        Ok(())
    }

    fn move_drag_window(
        &mut self,
        requested: crate::display::DesktopPosition,
    ) -> Result<(), AppError> {
        let confirmed = self
            .display_manager
            .as_ref()
            .ok_or_else(|| AppError::Platform("display manager is unavailable".to_owned()))?
            .constrain_position(
                requested,
                DesktopLogicalSize::new(PET_WINDOW_LOGICAL_SIZE, PET_WINDOW_LOGICAL_SIZE),
            );
        self.platform_backend
            .as_mut()
            .ok_or_else(|| AppError::Platform("platform backend is unavailable".to_owned()))?
            .set_window_position(confirmed)
            .map_err(|error| {
                AppError::Platform(format!(
                    "failed to drag the window to logical position ({:.3}, {:.3}): {error}",
                    confirmed.x, confirmed.y
                ))
            })?;
        if let Some(movement) = self.movement.as_mut() {
            movement.confirm_drag_position(confirmed);
        }
        self.mouse_state.update_window_origin(confirmed);
        self.update_pointer_hit();
        tracing::debug!(?requested, ?confirmed, "pet drag position applied");
        Ok(())
    }

    fn apply_movement_command(&mut self, key: KeyCode) {
        let intent = match key {
            KeyCode::ArrowLeft => PetIntent::Walk {
                direction: HorizontalDirection::Left,
            },
            KeyCode::ArrowRight => PetIntent::Walk {
                direction: HorizontalDirection::Right,
            },
            KeyCode::Space => match self.state_machine.as_ref().map(PetStateMachine::state) {
                Some(PetState::Idle) => PetIntent::Walk {
                    direction: HorizontalDirection::Right,
                },
                Some(_) => PetIntent::StayIdle,
                None => return,
            },
            _ => return,
        };
        self.apply_pet_intent(intent, TransitionContext::EXPLICIT);
    }

    fn apply_pet_intent(&mut self, intent: PetIntent, context: TransitionContext) {
        let Some(state_machine) = self.state_machine.as_mut() else {
            return;
        };
        let transition = state_machine.apply(intent, &context);
        tracing::info!(?intent, ?context, "pet intent evaluated");
        self.dispatch_transition(transition);
    }

    fn dispatch_transition(&mut self, transition: StateTransition) {
        if matches!(transition.outcome, TransitionOutcome::Rejected(_)) {
            tracing::warn!(?transition, "pet state transition rejected");
            return;
        }
        if transition.animation.is_none() && transition.facing.is_none() {
            return;
        }
        let direction = transition
            .facing
            .or_else(|| self.state_machine.as_ref().map(PetStateMachine::facing));
        if let Some(direction) = transition.facing
            && let Some(renderer) = self.renderer.as_mut()
        {
            renderer.set_pet_facing(direction);
        }
        if let Some(animation_intent) = transition.animation {
            let request = match animation_intent {
                PetAnimationIntent::Idle => AnimationRequest::Idle,
                PetAnimationIntent::Walk => AnimationRequest::Walk,
            };
            if let Some(animation) = self.animation.as_mut() {
                animation.request(request);
            }
        }
        if let Some(movement) = self.movement.as_mut() {
            match (transition.next, direction) {
                (PetState::Walking, Some(direction)) => movement.start_walking(direction),
                (PetState::Falling, _) => {}
                _ => movement.stop(),
            }
        }
        self.update_pointer_hit();
        tracing::info!(?transition, "pet state transition applied");
        if let Some(window) = self.window.as_ref() {
            let title = match (transition.next, direction) {
                (PetState::Walking, Some(HorizontalDirection::Left)) => "DesktopPet [Walking Left]",
                (PetState::Walking, Some(HorizontalDirection::Right)) => {
                    "DesktopPet [Walking Right]"
                }
                (PetState::Turning, Some(HorizontalDirection::Left)) => "DesktopPet [Turning Left]",
                (PetState::Turning, Some(HorizontalDirection::Right)) => {
                    "DesktopPet [Turning Right]"
                }
                (PetState::Dragged, _) => "DesktopPet [Dragged]",
                (PetState::Falling, _) => "DesktopPet [Falling]",
                (PetState::Landing, _) => "DesktopPet [Landing]",
                _ => "DesktopPet [Idle]",
            };
            window.set_title(title);
            window.request_redraw();
        }
        self.redraw_pending = true;
    }

    fn update_behavior(&mut self) {
        self.simulation_clock.advance(FIXED_UPDATE_INTERVAL);
        let intent = match (self.brain.as_mut(), self.state_machine.as_ref()) {
            (Some(brain), Some(state_machine)) => brain.update(
                &PetObservation {
                    state: state_machine.state(),
                    facing: state_machine.facing(),
                },
                self.simulation_clock.now(),
                &mut self.brain_rng,
            ),
            _ => None,
        };
        if let Some(intent) = intent {
            self.apply_pet_intent(intent, TransitionContext::BRAIN);
        }
        if let Some(state_machine) = self.state_machine.as_mut() {
            let transition =
                state_machine.fixed_update(FIXED_UPDATE_INTERVAL, &TransitionContext::BRAIN);
            self.dispatch_transition(transition);
        }
    }

    fn run_fixed_updates(
        &mut self,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) -> Result<(), AppError> {
        let Some(previous) = self.last_logic_update.replace(now) else {
            return Ok(());
        };
        let batch = self
            .fixed_steps
            .push(now.saturating_duration_since(previous));
        if !batch.dropped_time.is_zero() {
            tracing::warn!(
                dropped_ms = batch.dropped_time.as_secs_f64() * 1_000.0,
                "fixed update backlog was dropped"
            );
        }
        for _ in 0..batch.steps {
            self.refresh_pointer_from_platform()?;
            self.monitor_refresh_elapsed = self
                .monitor_refresh_elapsed
                .saturating_add(FIXED_UPDATE_INTERVAL);
            if self.monitor_refresh_elapsed >= MONITOR_REFRESH_INTERVAL {
                self.monitor_refresh_elapsed = Duration::ZERO;
                self.refresh_monitors()?;
            }
            self.update_behavior();
            if let Some(animation) = self.animation.as_mut() {
                animation
                    .advance(FIXED_UPDATE_INTERVAL)
                    .map_err(|error| AppError::Animation(error.to_string()))?;
            }
            self.advance_movement()?;
        }
        if batch.steps > 0 {
            if let (Some(renderer), Some(animation)) =
                (self.renderer.as_ref(), self.animation.as_ref())
            {
                renderer.update_skinning(animation.skin_matrices());
            }
            self.redraw_pending = true;
            event_loop.set_control_flow(ControlFlow::Poll);
        }
        Ok(())
    }

    fn render_pending_frame(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId) {
        let result = {
            let Some(renderer) = self.renderer.as_mut() else {
                return;
            };
            renderer.render()
        };
        match result {
            Ok(RenderOutcome::Presented) => {
                self.redraw_pending = false;
                if self.has_presented_frame {
                    tracing::debug!(?window_id, "wgpu frame presented");
                } else {
                    self.has_presented_frame = true;
                    tracing::info!(?window_id, "first wgpu frame presented");
                }
            }
            Ok(RenderOutcome::SkippedOccluded) => {
                self.redraw_pending = !self.has_presented_frame;
                event_loop.set_control_flow(if self.redraw_pending {
                    ControlFlow::Poll
                } else {
                    ControlFlow::Wait
                });
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
}

impl ApplicationHandler for Application {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        tracing::debug!("winit application resumed");
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
                self.restore_mouse_handling();
                event_loop.exit();
            }
            WindowEvent::Destroyed => {
                tracing::debug!(?window_id, "window destroyed");
                self.renderer = None;
                self._asset_manager = None;
                self.animation = None;
                self.movement = None;
                self.display_manager = None;
                self.mouse_state = MouseState::default();
                self.interaction = InteractionController::default();
                self.click_through_active = None;
                self.brain = None;
                self.state_machine = None;
                self.platform_backend = None;
                self.interaction_epoch = None;
                self.last_logic_update = None;
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
                self.update_pointer_hit();
                if let Err(error) = self.sync_click_through() {
                    self.fail_and_exit(event_loop, error);
                    return;
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
                if let Err(error) = self.refresh_monitors() {
                    self.fail_and_exit(event_loop, error);
                    return;
                }
                self.update_pointer_hit();
                if let Err(error) = self.sync_click_through() {
                    self.fail_and_exit(event_loop, error);
                    return;
                }
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let window_origin = self
                    .movement
                    .as_ref()
                    .map(MovementController::position)
                    .unwrap_or_default();
                if let Some(window) = self.window.as_ref() {
                    let size = window.inner_size();
                    self.mouse_state.update_cursor_physical(
                        [position.x, position.y],
                        window_origin,
                        window.scale_factor(),
                        DesktopPhysicalSize::new(size.width, size.height),
                    );
                    self.update_pointer_hit();
                    if let Err(error) = self.sync_click_through() {
                        self.fail_and_exit(event_loop, error);
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.mouse_state.clear_cursor();
                self.update_pointer_hit();
                if let Err(error) = self.sync_click_through() {
                    self.fail_and_exit(event_loop, error);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                if let Err(error) = self.refresh_pointer_from_platform() {
                    self.fail_and_exit(event_loop, error);
                    return;
                }
                tracing::info!(
                    pressed,
                    desktop = ?self.mouse_state.desktop_position,
                    window_logical = ?self.mouse_state.window_logical_position,
                    "pet pointer button changed"
                );
                self.mouse_state.set_left_pressed(pressed);
                let timestamp = self.interaction_timestamp();
                let action = if pressed {
                    let window_origin = self
                        .movement
                        .as_ref()
                        .map(MovementController::position)
                        .unwrap_or_default();
                    self.interaction.pointer_down(
                        self.mouse_state.desktop_position,
                        window_origin,
                        timestamp,
                    )
                } else {
                    self.interaction
                        .pointer_up(self.mouse_state.desktop_position, timestamp)
                };
                if let Err(error) = self.apply_interaction_action(action) {
                    self.fail_and_exit(event_loop, error);
                    return;
                }
                if let Err(error) = self.sync_click_through() {
                    self.fail_and_exit(event_loop, error);
                }
            }
            WindowEvent::Focused(false) => {
                tracing::info!(
                    ?window_id,
                    "window focus lost; cancelling pointer interaction"
                );
                let action = self.interaction.cancel_pointer();
                self.mouse_state.set_left_pressed(false);
                if let Err(error) = self.apply_interaction_action(action) {
                    self.fail_and_exit(event_loop, error);
                    return;
                }
                if let Err(error) = self.refresh_pointer_from_platform() {
                    self.fail_and_exit(event_loop, error);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.mouse_state.set_modifiers(modifiers.state());
            }
            WindowEvent::RedrawRequested => {
                tracing::debug!(?window_id, "processing pending wgpu redraw");
                if self.redraw_pending {
                    self.render_pending_frame(event_loop, window_id);
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && matches!(
                        event.physical_key,
                        PhysicalKey::Code(
                            KeyCode::Space | KeyCode::ArrowLeft | KeyCode::ArrowRight
                        )
                    ) =>
            {
                if let PhysicalKey::Code(key) = event.physical_key {
                    self.apply_movement_command(key);
                    event_loop.set_control_flow(ControlFlow::Poll);
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if let Err(error) = self.run_fixed_updates(event_loop, now) {
            self.fail_and_exit(event_loop, error);
            return;
        }
        if self.redraw_pending
            && let Some(window) = self.window.as_ref()
        {
            let window_id = window.id();
            if !self.redraw_request_logged {
                tracing::info!(?window_id, "requesting pending wgpu redraw");
                self.redraw_request_logged = true;
            }
            window.request_redraw();
            if !self.has_presented_frame {
                self.render_pending_frame(event_loop, window_id);
            }
        }
        if !self.redraw_pending && self.last_logic_update.is_some() {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                now + self.fixed_steps.until_next_step(),
            ));
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.restore_mouse_handling();
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
