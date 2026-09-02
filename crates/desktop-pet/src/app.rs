use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes, WindowId, WindowLevel},
};

use crate::{
    animation::{AnimationController, AnimationRequest, LookTarget},
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
    time::{FIXED_UPDATE_INTERVAL, FixedStepAccumulator, FrameActivity, FrameScheduler},
};

pub const PET_WINDOW_LOGICAL_SIZE: f64 = 320.0;
const DEFAULT_BRAIN_SEED: u64 = 0x3d50_6574_2026_0831;
const MONITOR_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const BACKLOG_WARNING_INTERVAL: Duration = Duration::from_secs(5);
const RUNTIME_METRICS_INTERVAL: Duration = Duration::from_secs(10);
const OCCLUSION_SUSPEND_DELAY: Duration = Duration::from_millis(500);

fn occlusion_suspends_rendering(occluded_for: Option<Duration>, state: Option<PetState>) -> bool {
    occluded_for.is_some_and(|elapsed| elapsed >= OCCLUSION_SUSPEND_DELAY)
        && matches!(state, None | Some(PetState::Idle | PetState::Sleeping))
}

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
    frame_scheduler: FrameScheduler,
    window_occluded: bool,
    window_occluded_since: Option<Instant>,
    surface_suspended: bool,
    last_backlog_warning: Option<Instant>,
    metrics_period_started: Option<Instant>,
    metrics_presented_frames: u64,
    metrics_fixed_steps: u64,
    metrics_dropped_time: Duration,
    redraw_request_logged: bool,
    has_presented_frame: bool,
    fatal_error: Option<AppError>,
}

impl Application {
    pub fn new(config: AppConfig) -> Result<Self, AppError> {
        config.validate()?;
        let frame_scheduler = FrameScheduler::new(config.fps);
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
            frame_scheduler,
            window_occluded: false,
            window_occluded_since: None,
            surface_suspended: false,
            last_backlog_warning: None,
            metrics_period_started: None,
            metrics_presented_frames: 0,
            metrics_fixed_steps: 0,
            metrics_dropped_time: Duration::ZERO,
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
        let event_loop = platform::create_event_loop()?;
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
        self.frame_scheduler = FrameScheduler::new(self.config.fps);
        self.frame_scheduler
            .set_activity(FrameActivity::Idle, Duration::ZERO);
        self.window_occluded = false;
        self.window_occluded_since = None;
        self.surface_suspended = false;
        self.last_backlog_warning = None;
        self.metrics_period_started = Some(now);
        self.metrics_presented_frames = 0;
        self.metrics_fixed_steps = 0;
        self.metrics_dropped_time = Duration::ZERO;
        self.redraw_request_logged = false;
        self.refresh_pointer_from_platform()?;
        if let Some(window) = self.window.as_ref() {
            window.set_visible(true);
        }
        self.platform_backend
            .as_mut()
            .ok_or_else(|| AppError::Platform("platform backend is unavailable".to_owned()))?
            .reassert_window_order()
            .map_err(|error| AppError::Platform(error.to_string()))?;
        event_loop.set_control_flow(ControlFlow::WaitUntil(now));
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
        let display_manager = self
            .display_manager
            .as_ref()
            .ok_or_else(|| AppError::Platform("display manager is unavailable".to_owned()))?;
        let ground_y = display_manager
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
                let confirmed = display_manager.constrain_position(proposed, window_size);
                platform_backend
                    .set_window_position(confirmed)
                    .map_err(|error| {
                        AppError::Platform(format!(
                            "failed to move the falling window to logical position ({:.3}, {:.3}): {error}",
                            confirmed.x, confirmed.y
                        ))
                    })?;
                Ok::<_, AppError>((confirmed, confirmed))
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
            self.frame_scheduler.mark_dirty();
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
                (PetState::Sleeping, _) => "DesktopPet [Sleeping]",
                _ => "DesktopPet [Idle]",
            };
            window.set_title(title);
        }
        self.frame_scheduler.mark_dirty();
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

    fn update_look_target(&mut self) {
        let target = self
            .animation
            .as_ref()
            .and_then(AnimationController::head_model_position)
            .zip(self.renderer.as_ref().and_then(|renderer| {
                let scale_factor = self.window.as_ref()?.scale_factor();
                renderer.pet_projection(scale_factor)
            }))
            .and_then(|(head, projection)| projection.model_to_window_logical(head))
            .zip(self.mouse_state.window_logical_position)
            .and_then(|(head, mouse)| {
                LookTarget::from_window_points(head, mouse, PET_WINDOW_LOGICAL_SIZE * 0.5)
            })
            .map(|mut target| {
                if self.state_machine.as_ref().map(PetStateMachine::facing)
                    == Some(HorizontalDirection::Right)
                {
                    target.yaw_radians = -target.yaw_radians;
                }
                target
            });
        if let Some(animation) = self.animation.as_mut() {
            animation.set_look_target(target);
        }
    }

    fn run_fixed_updates(&mut self, now: Instant) -> Result<(), AppError> {
        let Some(previous) = self.last_logic_update.replace(now) else {
            return Ok(());
        };
        let batch = self
            .fixed_steps
            .push(now.saturating_duration_since(previous));
        self.metrics_fixed_steps = self.metrics_fixed_steps.saturating_add(batch.steps as u64);
        self.metrics_dropped_time = self.metrics_dropped_time.saturating_add(batch.dropped_time);
        if !batch.dropped_time.is_zero()
            && self
                .last_backlog_warning
                .is_none_or(|last| now.saturating_duration_since(last) >= BACKLOG_WARNING_INTERVAL)
        {
            tracing::warn!(
                dropped_ms = batch.dropped_time.as_secs_f64() * 1_000.0,
                "fixed update backlog was dropped"
            );
            self.last_backlog_warning = Some(now);
        }
        // CursorMoved events already update the local and desktop coordinates. A single global
        // refresh per event-loop turn covers event coalescing and drag releases without issuing
        // an AppKit mouseLocation query for every fixed simulation step.
        if batch.steps > 0 {
            self.refresh_pointer_from_platform()?;
        }
        for _ in 0..batch.steps {
            self.monitor_refresh_elapsed = self
                .monitor_refresh_elapsed
                .saturating_add(FIXED_UPDATE_INTERVAL);
            if self.monitor_refresh_elapsed >= MONITOR_REFRESH_INTERVAL {
                self.monitor_refresh_elapsed = Duration::ZERO;
                self.refresh_monitors()?;
            }
            self.update_behavior();
            self.update_look_target();
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
            self.frame_scheduler.mark_dirty();
        }
        Ok(())
    }

    fn render_pending_frame(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId) {
        let now = self.scheduler_timestamp();
        let result = {
            let Some(renderer) = self.renderer.as_mut() else {
                return;
            };
            renderer.render()
        };
        match result {
            Ok(RenderOutcome::Presented) => {
                self.surface_suspended = false;
                self.frame_scheduler.presented(now);
                self.metrics_presented_frames = self.metrics_presented_frames.saturating_add(1);
                if self.has_presented_frame {
                    tracing::debug!(?window_id, "wgpu frame presented");
                } else {
                    self.has_presented_frame = true;
                    tracing::info!(?window_id, "first wgpu frame presented");
                }
            }
            Ok(RenderOutcome::SkippedOccluded) => {
                self.frame_scheduler.defer_redraw(now);
            }
            Ok(RenderOutcome::Reconfigured | RenderOutcome::SkippedTimeout) => {
                self.frame_scheduler.defer_redraw(now);
            }
            Err(error) => self.fail_and_exit(event_loop, error.into()),
        }
    }

    fn scheduler_timestamp(&self) -> Duration {
        self.interaction_epoch
            .map(|epoch| Instant::now().saturating_duration_since(epoch))
            .unwrap_or_default()
    }

    fn reset_logic_timing(&mut self) {
        self.last_logic_update = Some(Instant::now());
        self.fixed_steps.reset();
    }

    fn frame_activity(&self, now: Instant) -> FrameActivity {
        let occluded_for = self
            .window_occluded
            .then(|| now.saturating_duration_since(self.window_occluded_since.unwrap_or(now)));
        if occlusion_suspends_rendering(
            occluded_for,
            self.state_machine.as_ref().map(PetStateMachine::state),
        ) || self.surface_suspended
            || self.window.is_none()
        {
            return FrameActivity::Static;
        }
        match self.state_machine.as_ref().map(PetStateMachine::state) {
            Some(PetState::Idle) => FrameActivity::Idle,
            Some(PetState::Sleeping) => FrameActivity::Sleeping,
            Some(_) => FrameActivity::Active,
            None => FrameActivity::Static,
        }
    }

    fn brain_wall_deadline(&self, scheduler_now: Duration) -> Option<Duration> {
        let remaining = self
            .brain
            .as_ref()?
            .next_deadline()?
            .saturating_sub(self.simulation_clock.now());
        Some(scheduler_now + remaining)
    }

    fn report_runtime_metrics(&mut self, now: Instant, activity: FrameActivity) {
        let Some(started) = self.metrics_period_started else {
            self.metrics_period_started = Some(now);
            return;
        };
        let elapsed = now.saturating_duration_since(started);
        if elapsed < RUNTIME_METRICS_INTERVAL {
            return;
        }
        let elapsed_seconds = elapsed.as_secs_f64();
        tracing::info!(
            mode = activity.label(),
            target_fps = ?activity.target_fps(self.config.fps),
            presented_fps = self.metrics_presented_frames as f64 / elapsed_seconds,
            fixed_update_hz = self.metrics_fixed_steps as f64 / elapsed_seconds,
            dropped_ms = self.metrics_dropped_time.as_secs_f64() * 1_000.0,
            "runtime frame metrics"
        );
        self.metrics_period_started = Some(now);
        self.metrics_presented_frames = 0;
        self.metrics_fixed_steps = 0;
        self.metrics_dropped_time = Duration::ZERO;
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
                self.fixed_steps.reset();
                self.frame_scheduler.clear_dirty();
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
                let was_suspended = self.surface_suspended;
                self.surface_suspended = size.width == 0 || size.height == 0;
                self.update_pointer_hit();
                if let Err(error) = self.sync_click_through() {
                    self.fail_and_exit(event_loop, error);
                }
                if self.surface_suspended {
                    self.frame_scheduler.clear_dirty();
                } else {
                    if was_suspended {
                        self.reset_logic_timing();
                    }
                    self.frame_scheduler.mark_dirty();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                tracing::debug!(?window_id, scale_factor, "window scale factor changed");
                if let (Some(renderer), Some(window)) =
                    (self.renderer.as_mut(), self.window.as_ref())
                {
                    renderer.resize(window.inner_size());
                }
                self.surface_suspended = false;
                self.frame_scheduler.mark_dirty();
                if let Err(error) = self.refresh_monitors() {
                    self.fail_and_exit(event_loop, error);
                    return;
                }
                self.update_pointer_hit();
                if let Err(error) = self.sync_click_through() {
                    self.fail_and_exit(event_loop, error);
                }
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
                    self.frame_scheduler.mark_dirty();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.mouse_state.clear_cursor();
                self.update_pointer_hit();
                if let Err(error) = self.sync_click_through() {
                    self.fail_and_exit(event_loop, error);
                }
                self.frame_scheduler.mark_dirty();
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
                self.frame_scheduler.mark_dirty();
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
                self.frame_scheduler.mark_dirty();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.mouse_state.set_modifiers(modifiers.state());
                self.frame_scheduler.mark_dirty();
            }
            WindowEvent::Occluded(occluded) => {
                self.window_occluded = occluded;
                if occluded {
                    self.window_occluded_since.get_or_insert_with(Instant::now);
                    if let Some(platform_backend) = self.platform_backend.as_mut()
                        && let Err(error) = platform_backend.reassert_window_order()
                    {
                        self.fail_and_exit(event_loop, AppError::Platform(error.to_string()));
                        return;
                    }
                    self.frame_scheduler.mark_dirty();
                } else {
                    self.window_occluded_since = None;
                    self.surface_suspended = false;
                    self.reset_logic_timing();
                    self.frame_scheduler.mark_dirty();
                }
                tracing::info!(occluded, "window occlusion state changed");
            }
            WindowEvent::RedrawRequested => {
                tracing::debug!(?window_id, "processing pending wgpu redraw");
                if self.frame_scheduler.is_dirty() {
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
                    self.frame_scheduler.mark_dirty();
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
        let scheduler_now = self
            .interaction_epoch
            .map(|epoch| now.saturating_duration_since(epoch))
            .unwrap_or_default();
        let mut activity = self.frame_activity(now);
        if self.frame_scheduler.set_activity(activity, scheduler_now) {
            tracing::info!(
                mode = activity.label(),
                target_fps = ?activity.target_fps(self.config.fps),
                "frame scheduler activity changed"
            );
        }
        if activity == FrameActivity::Static {
            self.last_logic_update = Some(now);
            self.fixed_steps.reset();
        } else if let Err(error) = self.run_fixed_updates(now) {
            self.fail_and_exit(event_loop, error);
            return;
        }
        activity = self.frame_activity(now);
        if self.frame_scheduler.set_activity(activity, scheduler_now) {
            tracing::info!(
                mode = activity.label(),
                target_fps = ?activity.target_fps(self.config.fps),
                "frame scheduler activity changed"
            );
        }
        self.report_runtime_metrics(now, activity);
        let brain_deadline = (activity != FrameActivity::Static)
            .then(|| self.brain_wall_deadline(scheduler_now))
            .flatten();
        let occlusion_deadline = self.window_occluded_since.and_then(|started| {
            let deadline = started.checked_add(OCCLUSION_SUSPEND_DELAY)?;
            (deadline > now).then(|| scheduler_now + deadline.saturating_duration_since(now))
        });
        let external_deadline = [brain_deadline, occlusion_deadline]
            .into_iter()
            .flatten()
            .min();
        let decision = self
            .frame_scheduler
            .decision(scheduler_now, external_deadline);
        if decision.request_redraw
            && let Some(window) = self.window.as_ref()
        {
            let window_id = window.id();
            if !self.redraw_request_logged {
                tracing::info!(?window_id, "requesting pending wgpu redraw");
                self.redraw_request_logged = true;
            }
            window.request_redraw();
        }
        let wake = decision.next_wake.and_then(|deadline| {
            self.interaction_epoch
                .map(|epoch| epoch.checked_add(deadline).unwrap_or(now))
        });
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
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

    #[test]
    fn transient_occlusion_does_not_suspend_rendering() {
        assert!(!occlusion_suspends_rendering(
            Some(OCCLUSION_SUSPEND_DELAY - Duration::from_millis(1)),
            Some(PetState::Idle),
        ));
    }

    #[test]
    fn stable_occlusion_suspends_inactive_pet() {
        for state in [PetState::Idle, PetState::Sleeping] {
            assert!(occlusion_suspends_rendering(
                Some(OCCLUSION_SUSPEND_DELAY),
                Some(state),
            ));
        }
    }

    #[test]
    fn active_motion_ignores_occlusion() {
        for state in [
            PetState::Dragged,
            PetState::Falling,
            PetState::Landing,
            PetState::Walking,
            PetState::Turning,
        ] {
            assert!(!occlusion_suspends_rendering(
                Some(Duration::from_secs(10)),
                Some(state),
            ));
        }
    }
}
