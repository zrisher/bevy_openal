use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::message::MessageReader;
use bevy_ecs::prelude::*;
use bevy_math::Vec3;
use bevy_transform::components::GlobalTransform;
use tracing::{error, warn};

use crate::{
    AudioRenderMode, AudioRuntime, AudioRuntimeConfig, BufferKey, DistanceModel, ListenerFrame,
    PlayOneShotParams, RuntimeError,
};

pub struct BevyOpenalPlugin;

impl Plugin for BevyOpenalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenalSettings>()
            .init_resource::<OpenalStatus>()
            .add_message::<OpenalPlayOneShot>()
            .add_systems(Startup, init_openal_runtime)
            .add_systems(
                Update,
                (
                    apply_settings_system,
                    sync_status_system,
                    sync_listener_system,
                    play_one_shot_system,
                ),
            );
    }
}

#[derive(Resource, Clone)]
pub struct OpenalSettings {
    pub render_mode: AudioRenderMode,
    pub distance_model: DistanceModel,
    pub max_sources: usize,
    pub preferred_device: Option<String>,
    pub muted: bool,
}

impl Default for OpenalSettings {
    fn default() -> Self {
        Self {
            render_mode: AudioRenderMode::Auto,
            distance_model: DistanceModel::InverseClamped,
            max_sources: 64,
            preferred_device: None,
            muted: false,
        }
    }
}

#[derive(Resource, Clone, Default)]
pub struct OpenalStatus {
    pub available: bool,
    pub status: crate::AudioRuntimeStatus,
}

#[derive(Resource)]
pub struct OpenalRuntime {
    runtime: AudioRuntime,
}

impl OpenalRuntime {
    pub fn new(settings: &OpenalSettings) -> Result<Self, RuntimeError> {
        let runtime = AudioRuntime::new(AudioRuntimeConfig {
            initial_render_mode: settings.render_mode,
            distance_model: settings.distance_model,
            max_sources: settings.max_sources,
            preferred_device: settings.preferred_device.clone(),
        })?;
        if settings.muted {
            let _ = runtime.set_muted(true);
        }
        Ok(Self { runtime })
    }

    pub fn runtime(&self) -> &AudioRuntime {
        &self.runtime
    }

    pub fn shutdown(&mut self) {
        self.runtime.shutdown();
    }
}

#[derive(Component)]
pub struct OpenalListener;

#[derive(Message, Copy, Clone)]
pub struct OpenalPlayOneShot {
    pub key: BufferKey,
    pub position: Vec3,
    pub gain: f32,
    pub pitch: f32,
}

fn init_openal_runtime(mut commands: Commands, settings: Res<OpenalSettings>) {
    match OpenalRuntime::new(&settings) {
        Ok(runtime) => {
            commands.insert_resource(runtime);
        }
        Err(err) => {
            error!(error = %err, "OpenAL runtime unavailable");
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
struct AppliedSettings {
    render_mode: AudioRenderMode,
    distance_model: DistanceModel,
    muted: bool,
}

fn apply_settings_system(
    settings: Res<OpenalSettings>,
    runtime: Option<Res<OpenalRuntime>>,
    mut applied: Local<Option<AppliedSettings>>,
) {
    let Some(runtime) = runtime else {
        *applied = None;
        return;
    };
    if runtime.runtime().is_shutdown_requested() {
        return;
    }

    if applied.is_none() {
        *applied = Some(AppliedSettings {
            render_mode: settings.render_mode,
            distance_model: settings.distance_model,
            muted: settings.muted,
        });
        return;
    }

    if !settings.is_changed() {
        return;
    }

    let Some(mut applied_settings) = *applied else {
        return;
    };

    if settings.muted != applied_settings.muted {
        if runtime.runtime().set_muted(settings.muted).is_err() {
            warn!("Failed to apply OpenAL mute");
        } else {
            applied_settings.muted = settings.muted;
        }
    }

    if settings.distance_model != applied_settings.distance_model {
        if runtime
            .runtime()
            .set_distance_model(settings.distance_model)
            .is_err()
        {
            warn!("Failed to apply OpenAL distance model");
        } else {
            applied_settings.distance_model = settings.distance_model;
        }
    }

    if settings.render_mode != applied_settings.render_mode {
        if runtime
            .runtime()
            .set_render_mode(settings.render_mode)
            .is_err()
        {
            warn!("Failed to apply OpenAL render mode");
        } else {
            applied_settings.render_mode = settings.render_mode;
        }
    }

    *applied = Some(applied_settings);
}

fn sync_status_system(runtime: Option<Res<OpenalRuntime>>, mut status: ResMut<OpenalStatus>) {
    let Some(runtime) = runtime else {
        status.available = false;
        status.status = Default::default();
        return;
    };

    status.available = true;
    status.status = runtime.runtime().status();
}

fn sync_listener_system(
    listener_query: Query<&GlobalTransform, With<OpenalListener>>,
    runtime: Option<Res<OpenalRuntime>>,
) {
    let Some(runtime) = runtime else {
        return;
    };
    if runtime.runtime().is_shutdown_requested() {
        return;
    }
    let Some(transform) = listener_query.iter().next() else {
        return;
    };

    let transform = transform.compute_transform();
    let forward = transform.rotation.mul_vec3(Vec3::NEG_Z);
    let up = transform.rotation.mul_vec3(Vec3::Y);
    let listener = ListenerFrame {
        position: transform.translation,
        forward,
        up,
        velocity: Vec3::ZERO,
    };
    if runtime.runtime().set_listener(listener).is_err() {
        warn!("Failed to update OpenAL listener");
    }
}

fn play_one_shot_system(
    mut messages: MessageReader<OpenalPlayOneShot>,
    runtime: Option<Res<OpenalRuntime>>,
) {
    let Some(runtime) = runtime else {
        messages.clear();
        return;
    };
    if runtime.runtime().is_shutdown_requested() {
        messages.clear();
        return;
    }
    for event in messages.read() {
        let params = PlayOneShotParams {
            position: event.position,
            gain: event.gain,
            pitch: event.pitch,
        };
        if runtime.runtime().play_one_shot(event.key, params).is_err() {
            warn!("Failed to play OpenAL one-shot");
        }
    }
}
