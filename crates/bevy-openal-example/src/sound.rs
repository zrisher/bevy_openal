use crate::cli;
use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::prelude::{Commands, Query, Res, ResMut, Resource, With};
use bevy_math::Vec3;
use bevy_openal::{
    BufferKey, DecodedAudioMono16, OpenalListener, OpenalRuntime, PlayOneShotParams,
};
use bevy_time::{Time, Timer, TimerMode};
use bevy_transform::components::GlobalTransform;
use bevy_transform::prelude::Transform;
use std::collections::HashMap;

pub(crate) const DEFAULT_SAMPLE_RATE_HZ: u32 = 48_000;
const DEFAULT_BEEP_NAME: &str = "beep";
const DEFAULT_BEEP_SECONDS: f32 = 0.25;
pub(crate) const DEFAULT_BEEP_FREQ_HZ: f32 = 880.0;
const ORBIT_FIRE_INTERVAL_SECS: f32 = 0.2;

#[derive(Resource)]
pub(crate) struct BufferRegistry {
    next_key: BufferKey,
    name_to_key: HashMap<String, BufferKey>,
}

impl Default for BufferRegistry {
    fn default() -> Self {
        Self {
            next_key: 1,
            name_to_key: HashMap::new(),
        }
    }
}

impl BufferRegistry {
    pub(crate) fn allocate_key(&mut self) -> BufferKey {
        let key = self.next_key;
        self.next_key = self.next_key.wrapping_add(1);
        if self.next_key == 0 {
            self.next_key = 1;
        }
        key
    }

    pub(crate) fn insert(&mut self, name: String, key: BufferKey) -> Option<BufferKey> {
        self.name_to_key.insert(name, key)
    }

    pub(crate) fn get(&self, name: &str) -> Option<BufferKey> {
        self.name_to_key.get(name).copied()
    }
}

#[derive(Default, Resource)]
pub(crate) struct ListenerTarget {
    position: Vec3,
}

impl ListenerTarget {
    pub(crate) fn set_position(&mut self, position: Vec3) {
        self.position = position;
    }
}

#[derive(Resource)]
pub(crate) struct OrbitState {
    active: bool,
    buffer_key: BufferKey,
    radius: f32,
    seconds_per_rev: f32,
    angle: f32,
    timer: Timer,
    plane: OrbitPlane,
}

enum OrbitPlane {
    Horizontal,
    Vertical,
}

impl Default for OrbitState {
    fn default() -> Self {
        Self {
            active: false,
            buffer_key: 0,
            radius: 0.0,
            seconds_per_rev: 1.0,
            angle: 0.0,
            timer: Timer::from_seconds(ORBIT_FIRE_INTERVAL_SECS, TimerMode::Repeating),
            plane: OrbitPlane::Horizontal,
        }
    }
}

#[derive(Default, Resource)]
pub(crate) struct DefaultSampleState {
    loaded: bool,
}

#[derive(Default, Resource)]
pub(crate) struct LoopTracker {
    name: Option<String>,
}

impl LoopTracker {
    pub(crate) fn current_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub(crate) fn set_name(&mut self, name: Option<String>) {
        self.name = name;
    }

    pub(crate) fn clear(&mut self) {
        self.name = None;
    }
}

impl OrbitState {
    pub(crate) fn start_horizontal(&mut self, key: BufferKey, radius: f32, seconds_per_rev: f32) {
        self.start_orbit(key, radius, seconds_per_rev, OrbitPlane::Horizontal);
    }

    pub(crate) fn start_vertical(&mut self, key: BufferKey, radius: f32, seconds_per_rev: f32) {
        self.start_orbit(key, radius, seconds_per_rev, OrbitPlane::Vertical);
    }

    pub(crate) fn stop(&mut self) {
        self.active = false;
    }

    fn start_orbit(
        &mut self,
        key: BufferKey,
        radius: f32,
        seconds_per_rev: f32,
        plane: OrbitPlane,
    ) {
        self.active = true;
        self.buffer_key = key;
        self.radius = radius;
        self.seconds_per_rev = seconds_per_rev.max(0.1);
        self.angle = 0.0;
        self.timer.reset();
        self.plane = plane;
    }
}

pub(crate) fn setup_listener(mut commands: Commands) {
    commands.spawn((
        OpenalListener,
        Transform::default(),
        GlobalTransform::default(),
    ));
}

pub(crate) fn ensure_default_sample(
    runtime: Option<Res<OpenalRuntime>>,
    mut registry: ResMut<BufferRegistry>,
    mut state: ResMut<DefaultSampleState>,
) {
    if state.loaded {
        return;
    }
    let Some(runtime) = runtime else {
        return;
    };

    let decoded = generate_sine(
        DEFAULT_SAMPLE_RATE_HZ,
        DEFAULT_BEEP_SECONDS,
        DEFAULT_BEEP_FREQ_HZ,
    );
    let key = registry.allocate_key();
    if runtime.runtime().create_buffer(key, decoded).is_ok() {
        registry.insert(DEFAULT_BEEP_NAME.to_string(), key);
        state.loaded = true;
        println!("Loaded default sample: {DEFAULT_BEEP_NAME}");
        cli::print_help_hint();
        cli::print_prompt();
    } else {
        println!("Failed to create default sample buffer");
        cli::print_prompt();
    }
}

pub(crate) fn apply_listener_target(
    target: Res<ListenerTarget>,
    mut query: Query<&mut Transform, With<OpenalListener>>,
) {
    if !target.is_changed() {
        return;
    }
    let Ok(mut transform) = query.single_mut() else {
        return;
    };
    transform.translation = target.position;
}

pub(crate) fn update_orbit(
    time: Res<Time>,
    runtime: Option<Res<OpenalRuntime>>,
    mut orbit: ResMut<OrbitState>,
) {
    if !orbit.active {
        return;
    }
    let Some(runtime) = runtime else {
        return;
    };

    let angle_speed = std::f32::consts::TAU / orbit.seconds_per_rev;
    orbit.angle = (orbit.angle + angle_speed * time.delta_secs()) % std::f32::consts::TAU;
    orbit.timer.tick(time.delta());
    if orbit.timer.just_finished() {
        let position = match orbit.plane {
            OrbitPlane::Horizontal => Vec3::new(
                orbit.radius * orbit.angle.cos(),
                0.0,
                orbit.radius * orbit.angle.sin(),
            ),
            OrbitPlane::Vertical => Vec3::new(
                0.0,
                orbit.radius * orbit.angle.sin(),
                orbit.radius * orbit.angle.cos(),
            ),
        };
        let params = PlayOneShotParams {
            position,
            gain: 1.0,
            pitch: 1.0,
        };
        let _ = runtime.runtime().play_one_shot(orbit.buffer_key, params);
    }
}

pub(crate) fn generate_sine(sample_rate_hz: u32, seconds: f32, freq_hz: f32) -> DecodedAudioMono16 {
    let frame_count = (seconds.max(0.0) * sample_rate_hz as f32).round() as usize;
    let mut samples = Vec::with_capacity(frame_count);
    let amplitude = 0.5;
    let step = std::f32::consts::TAU * freq_hz / sample_rate_hz as f32;
    for i in 0..frame_count {
        let value = (i as f32 * step).sin() * amplitude;
        samples.push((value * i16::MAX as f32) as i16);
    }
    DecodedAudioMono16 {
        sample_rate_hz,
        samples,
    }
}

pub(crate) fn generate_noise(sample_rate_hz: u32, seconds: f32) -> DecodedAudioMono16 {
    let frame_count = (seconds.max(0.0) * sample_rate_hz as f32).round() as usize;
    let mut samples = Vec::with_capacity(frame_count);
    let mut rng = SimpleRng::new(0x1234_5678);
    for _ in 0..frame_count {
        let value = rng.next_f32() * 2.0 - 1.0;
        samples.push((value * 0.4 * i16::MAX as f32) as i16);
    }
    DecodedAudioMono16 {
        sample_rate_hz,
        samples,
    }
}

struct SimpleRng {
    state: u32,
}

impl SimpleRng {
    fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    fn next_f32(&mut self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }
}
