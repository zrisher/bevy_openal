use glam::Vec3;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, error, info};

use crate::openal::{OpenalEngine, OpenalError};
use crate::DecodedAudioMono16;

pub type BufferKey = u32;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum AudioRenderMode {
    #[default]
    Auto,
    StereoClean,
    HeadphonesHrtf,
    SurroundAuto,
}

impl AudioRenderMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AudioRenderMode::Auto => "auto",
            AudioRenderMode::StereoClean => "stereo",
            AudioRenderMode::HeadphonesHrtf => "hrtf",
            AudioRenderMode::SurroundAuto => "surround",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "auto" => Some(AudioRenderMode::Auto),
            "stereo" | "stereo-clean" => Some(AudioRenderMode::StereoClean),
            "hrtf" | "headphones" => Some(AudioRenderMode::HeadphonesHrtf),
            "surround" | "surround-auto" => Some(AudioRenderMode::SurroundAuto),
            _ => None,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum DistanceModel {
    #[default]
    None,
    Inverse,
    InverseClamped,
    Linear,
    LinearClamped,
    Exponent,
    ExponentClamped,
}

impl DistanceModel {
    pub fn as_str(self) -> &'static str {
        match self {
            DistanceModel::None => "none",
            DistanceModel::Inverse => "inverse",
            DistanceModel::InverseClamped => "inverse-clamp",
            DistanceModel::Linear => "linear",
            DistanceModel::LinearClamped => "linear-clamp",
            DistanceModel::Exponent => "exponent",
            DistanceModel::ExponentClamped => "exponent-clamp",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "none" | "off" => Some(DistanceModel::None),
            "inverse" => Some(DistanceModel::Inverse),
            "inverse-clamp" | "inverse-clamped" => Some(DistanceModel::InverseClamped),
            "linear" => Some(DistanceModel::Linear),
            "linear-clamp" | "linear-clamped" => Some(DistanceModel::LinearClamped),
            "exponent" => Some(DistanceModel::Exponent),
            "exponent-clamp" | "exponent-clamped" => Some(DistanceModel::ExponentClamped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioRuntimeConfig {
    pub initial_render_mode: AudioRenderMode,
    pub distance_model: DistanceModel,
    pub max_sources: usize,
    pub preferred_device: Option<String>,
}

impl Default for AudioRuntimeConfig {
    fn default() -> Self {
        Self {
            initial_render_mode: AudioRenderMode::Auto,
            distance_model: DistanceModel::InverseClamped,
            max_sources: 64,
            preferred_device: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AudioRuntimeStatus {
    pub library_loaded: bool,
    pub device_open: bool,
    pub context_created: bool,
    pub render_mode: AudioRenderMode,
    pub output_mode: Option<String>,
    pub output_mode_raw: Option<i32>,
    pub distance_model: DistanceModel,
    pub hrtf_active: bool,
    pub muted: bool,
    pub loaded_buffers: usize,
    pub active_sources: usize,
    pub last_error: Option<String>,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("audio runtime is not available")]
    NotAvailable,
    #[error("audio runtime thread stopped unexpectedly")]
    ThreadStopped,
}

#[derive(Debug, Copy, Clone)]
pub struct ListenerFrame {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub velocity: Vec3,
}

impl Default for ListenerFrame {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            forward: Vec3::NEG_Z,
            up: Vec3::Y,
            velocity: Vec3::ZERO,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct PlayOneShotParams {
    pub position: Vec3,
    pub gain: f32,
    pub pitch: f32,
}

impl Default for PlayOneShotParams {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            gain: 1.0,
            pitch: 1.0,
        }
    }
}

enum AudioCommand {
    Shutdown,
    SetMuted(bool),
    SetRenderMode(AudioRenderMode),
    SetDistanceModel(DistanceModel),
    SetListener(ListenerFrame),
    CreateBuffer {
        key: BufferKey,
        decoded: DecodedAudioMono16,
    },
    PlayOneShot {
        key: BufferKey,
        params: PlayOneShotParams,
    },
    StartLoop {
        key: BufferKey,
        params: PlayOneShotParams,
    },
    StopLoop,
}

pub struct AudioRuntime {
    tx: mpsc::Sender<AudioCommand>,
    status: Arc<Mutex<AudioRuntimeStatus>>,
    thread: Option<thread::JoinHandle<()>>,
    shutdown_requested: AtomicBool,
}

impl AudioRuntime {
    pub fn new(config: AudioRuntimeConfig) -> Result<Self, RuntimeError> {
        let (tx, rx) = mpsc::channel::<AudioCommand>();
        let status = Arc::new(Mutex::new(AudioRuntimeStatus {
            render_mode: config.initial_render_mode,
            distance_model: config.distance_model,
            ..Default::default()
        }));

        let thread_status = Arc::clone(&status);
        let thread = thread::Builder::new()
            .name("zrg-audio".to_string())
            .spawn(move || audio_thread_main(config, rx, thread_status))
            .map_err(|_| RuntimeError::NotAvailable)?;

        Ok(Self {
            tx,
            status,
            thread: Some(thread),
            shutdown_requested: AtomicBool::new(false),
        })
    }

    pub fn status(&self) -> AudioRuntimeStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::Relaxed)
    }

    pub fn set_render_mode(&self, mode: AudioRenderMode) -> Result<(), RuntimeError> {
        self.tx
            .send(AudioCommand::SetRenderMode(mode))
            .map_err(|_| RuntimeError::ThreadStopped)
    }

    pub fn set_distance_model(&self, model: DistanceModel) -> Result<(), RuntimeError> {
        self.tx
            .send(AudioCommand::SetDistanceModel(model))
            .map_err(|_| RuntimeError::ThreadStopped)
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), RuntimeError> {
        self.tx
            .send(AudioCommand::SetMuted(muted))
            .map_err(|_| RuntimeError::ThreadStopped)
    }

    pub fn set_listener(&self, listener: ListenerFrame) -> Result<(), RuntimeError> {
        self.tx
            .send(AudioCommand::SetListener(listener))
            .map_err(|_| RuntimeError::ThreadStopped)
    }

    pub fn create_buffer(
        &self,
        key: BufferKey,
        decoded: DecodedAudioMono16,
    ) -> Result<(), RuntimeError> {
        self.tx
            .send(AudioCommand::CreateBuffer { key, decoded })
            .map_err(|_| RuntimeError::ThreadStopped)
    }

    pub fn play_one_shot(
        &self,
        key: BufferKey,
        params: PlayOneShotParams,
    ) -> Result<(), RuntimeError> {
        self.tx
            .send(AudioCommand::PlayOneShot { key, params })
            .map_err(|_| RuntimeError::ThreadStopped)
    }

    pub fn start_loop(
        &self,
        key: BufferKey,
        params: PlayOneShotParams,
    ) -> Result<(), RuntimeError> {
        self.tx
            .send(AudioCommand::StartLoop { key, params })
            .map_err(|_| RuntimeError::ThreadStopped)
    }

    pub fn stop_loop(&self) -> Result<(), RuntimeError> {
        self.tx
            .send(AudioCommand::StopLoop)
            .map_err(|_| RuntimeError::ThreadStopped)
    }

    pub fn shutdown(&mut self) {
        self.shutdown_requested.store(true, Ordering::Relaxed);
        let _ = self.tx.send(AudioCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for AudioRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn audio_thread_main(
    config: AudioRuntimeConfig,
    rx: mpsc::Receiver<AudioCommand>,
    status: Arc<Mutex<AudioRuntimeStatus>>,
) {
    let mut render_mode = config.initial_render_mode;
    let mut muted = false;
    let mut distance_model = config.distance_model;
    let mut buffers: HashMap<BufferKey, DecodedAudioMono16> = HashMap::new();
    let mut loop_state: Option<(BufferKey, PlayOneShotParams)> = None;

    let preferred_device = config.preferred_device.as_deref();

    let mut engine = match OpenalEngine::new(
        render_mode,
        preferred_device,
        config.max_sources,
        distance_model,
    ) {
        Ok(engine) => {
            let mut engine = engine;
            rebuild_buffers(
                &mut engine,
                &buffers,
                &status,
                render_mode,
                distance_model,
                muted,
            );
            rebuild_loop(
                &mut engine,
                &loop_state,
                &status,
                render_mode,
                distance_model,
                muted,
            );
            update_status_ok(&status, render_mode, muted, &engine);
            info!(render_mode = %render_mode.as_str(), "Audio runtime started");
            Some(engine)
        }
        Err(err) => {
            update_status_error(&status, render_mode, distance_model, muted, &err);
            error!(error = %err, "Audio runtime failed to initialize");
            None
        }
    };

    let mut last_listener = ListenerFrame::default();

    loop {
        if let Some(engine) = engine.as_mut() {
            engine.cleanup_finished_sources();
            update_counts(&status, engine);
        }

        match rx.recv_timeout(Duration::from_millis(5)) {
            Ok(AudioCommand::Shutdown) => {
                debug!("Audio runtime shutting down");
                break;
            }
            Ok(AudioCommand::SetMuted(value)) => {
                muted = value;
                if let Some(engine) = engine.as_ref() {
                    if let Err(err) = engine.set_muted(muted) {
                        update_status_error(&status, render_mode, distance_model, muted, &err);
                    } else {
                        update_status_ok(&status, render_mode, muted, engine);
                    }
                }
            }
            Ok(AudioCommand::SetRenderMode(mode)) => {
                render_mode = mode;
                match engine.as_mut() {
                    Some(engine) => {
                        match engine.recreate(render_mode, preferred_device, distance_model) {
                            Ok(()) => {
                                rebuild_buffers(
                                    engine,
                                    &buffers,
                                    &status,
                                    render_mode,
                                    distance_model,
                                    muted,
                                );
                                rebuild_loop(
                                    engine,
                                    &loop_state,
                                    &status,
                                    render_mode,
                                    distance_model,
                                    muted,
                                );
                                let _ = engine.set_muted(muted);
                                let _ = engine.set_listener(last_listener);
                                update_status_ok(&status, render_mode, muted, engine);
                                info!(render_mode = %render_mode.as_str(), "Audio render mode changed");
                            }
                            Err(err) => {
                                update_status_error(
                                    &status,
                                    render_mode,
                                    distance_model,
                                    muted,
                                    &err,
                                );
                            }
                        }
                    }
                    None => {
                        match OpenalEngine::new(
                            render_mode,
                            preferred_device,
                            config.max_sources,
                            distance_model,
                        ) {
                            Ok(new_engine) => {
                                let mut new_engine = new_engine;
                                rebuild_buffers(
                                    &mut new_engine,
                                    &buffers,
                                    &status,
                                    render_mode,
                                    distance_model,
                                    muted,
                                );
                                rebuild_loop(
                                    &mut new_engine,
                                    &loop_state,
                                    &status,
                                    render_mode,
                                    distance_model,
                                    muted,
                                );
                                let _ = new_engine.set_muted(muted);
                                let _ = new_engine.set_listener(last_listener);
                                update_status_ok(&status, render_mode, muted, &new_engine);
                                info!(render_mode = %render_mode.as_str(), "Audio runtime started");
                                engine = Some(new_engine);
                            }
                            Err(err) => {
                                update_status_error(
                                    &status,
                                    render_mode,
                                    distance_model,
                                    muted,
                                    &err,
                                );
                            }
                        }
                    }
                }
            }
            Ok(AudioCommand::SetDistanceModel(model)) => {
                distance_model = model;
                if let Some(engine) = engine.as_mut() {
                    if let Err(err) = engine.set_distance_model(distance_model) {
                        update_status_error(&status, render_mode, distance_model, muted, &err);
                    } else {
                        update_status_ok(&status, render_mode, muted, engine);
                    }
                }
            }
            Ok(AudioCommand::SetListener(listener)) => {
                last_listener = listener;
                if let Some(engine) = engine.as_ref() {
                    if let Err(err) = engine.set_listener(listener) {
                        update_status_error(&status, render_mode, distance_model, muted, &err);
                    }
                }
            }
            Ok(AudioCommand::CreateBuffer { key, decoded }) => {
                buffers.insert(key, decoded.clone());
                if let Some(engine) = engine.as_mut() {
                    if let Err(err) = engine.create_buffer(key, &decoded) {
                        update_status_error(&status, render_mode, distance_model, muted, &err);
                    } else {
                        update_status_ok(&status, render_mode, muted, engine);
                    }
                }
            }
            Ok(AudioCommand::StartLoop { key, params }) => {
                loop_state = Some((key, params));
                if let Some(engine) = engine.as_mut() {
                    if let Err(err) = engine.start_loop(key, params) {
                        if let Ok(mut st) = status.lock() {
                            st.last_error = Some(err.to_string());
                        }
                    } else {
                        update_status_ok(&status, render_mode, muted, engine);
                    }
                }
            }
            Ok(AudioCommand::StopLoop) => {
                loop_state = None;
                if let Some(engine) = engine.as_mut() {
                    if let Err(err) = engine.stop_loop() {
                        if let Ok(mut st) = status.lock() {
                            st.last_error = Some(err.to_string());
                        }
                    } else {
                        update_status_ok(&status, render_mode, muted, engine);
                    }
                }
            }
            Ok(AudioCommand::PlayOneShot { key, params }) => {
                if let Some(engine) = engine.as_mut() {
                    if let Err(err) = engine.play_one_shot(key, params) {
                        if let Ok(mut st) = status.lock() {
                            st.last_error = Some(err.to_string());
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }

    if let Some(engine) = engine.as_mut() {
        engine.shutdown();
    }
    info!("Audio runtime stopped");
}

fn update_counts(status: &Arc<Mutex<AudioRuntimeStatus>>, engine: &OpenalEngine) {
    let Ok(mut st) = status.lock() else {
        return;
    };
    st.loaded_buffers = engine.loaded_buffers();
    st.active_sources = engine.active_sources();
}

fn rebuild_buffers(
    engine: &mut OpenalEngine,
    buffers: &HashMap<BufferKey, DecodedAudioMono16>,
    status: &Arc<Mutex<AudioRuntimeStatus>>,
    render_mode: AudioRenderMode,
    distance_model: DistanceModel,
    muted: bool,
) {
    for (key, decoded) in buffers {
        if let Err(err) = engine.create_buffer(*key, decoded) {
            update_status_error(status, render_mode, distance_model, muted, &err);
        }
    }
}

fn rebuild_loop(
    engine: &mut OpenalEngine,
    loop_state: &Option<(BufferKey, PlayOneShotParams)>,
    status: &Arc<Mutex<AudioRuntimeStatus>>,
    render_mode: AudioRenderMode,
    distance_model: DistanceModel,
    muted: bool,
) {
    if let Some((key, params)) = loop_state {
        if let Err(err) = engine.start_loop(*key, *params) {
            update_status_error(status, render_mode, distance_model, muted, &err);
        }
    }
}

fn update_status_ok(
    status: &Arc<Mutex<AudioRuntimeStatus>>,
    render_mode: AudioRenderMode,
    muted: bool,
    engine: &OpenalEngine,
) {
    let Ok(mut st) = status.lock() else {
        return;
    };

    let (hrtf_active, output_mode_name, output_mode_raw, engine_distance_model) = engine.status();
    st.library_loaded = true;
    st.device_open = true;
    st.context_created = true;
    st.render_mode = render_mode;
    st.output_mode = output_mode_name.map(|s| s.to_string());
    st.output_mode_raw = output_mode_raw;
    st.distance_model = engine_distance_model;
    st.hrtf_active = hrtf_active;
    st.muted = muted;
    st.last_error = None;
}

fn update_status_error(
    status: &Arc<Mutex<AudioRuntimeStatus>>,
    render_mode: AudioRenderMode,
    distance_model: DistanceModel,
    muted: bool,
    err: &OpenalError,
) {
    let Ok(mut st) = status.lock() else {
        return;
    };
    st.render_mode = render_mode;
    st.distance_model = distance_model;
    st.muted = muted;
    st.last_error = Some(err.to_string());
}

#[cfg(test)]
mod tests {
    use super::AudioRenderMode;

    #[test]
    fn parse_render_modes() {
        assert_eq!(AudioRenderMode::parse("auto"), Some(AudioRenderMode::Auto));
        assert_eq!(
            AudioRenderMode::parse("stereo"),
            Some(AudioRenderMode::StereoClean)
        );
        assert_eq!(
            AudioRenderMode::parse("stereo-clean"),
            Some(AudioRenderMode::StereoClean)
        );
        assert_eq!(
            AudioRenderMode::parse("hrtf"),
            Some(AudioRenderMode::HeadphonesHrtf)
        );
        assert_eq!(
            AudioRenderMode::parse("headphones"),
            Some(AudioRenderMode::HeadphonesHrtf)
        );
        assert_eq!(
            AudioRenderMode::parse("surround"),
            Some(AudioRenderMode::SurroundAuto)
        );
        assert_eq!(
            AudioRenderMode::parse("surround-auto"),
            Some(AudioRenderMode::SurroundAuto)
        );
        assert_eq!(AudioRenderMode::parse("nope"), None);
    }
}
