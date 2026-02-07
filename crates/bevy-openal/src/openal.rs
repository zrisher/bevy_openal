use glam::Vec3;
use libloading::Library;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr::{self, NonNull};
use thiserror::Error;
use tracing::{debug, warn};

use crate::{
    AudioRenderMode, BufferKey, DecodedAudioMono16, DistanceModel, ListenerFrame, PlayOneShotParams,
};

pub type ALboolean = i8;
pub type ALenum = c_int;
pub type ALfloat = f32;
pub type ALint = c_int;
pub type ALsizei = c_int;
pub type ALuint = u32;
pub type ALvoid = c_void;

pub type ALCboolean = i8;
pub type ALCchar = c_char;
pub type ALCenum = c_int;
pub type ALCint = c_int;
pub type ALCsizei = c_int;

#[repr(C)]
pub struct ALCdevice {
    _private: [u8; 0],
}

#[repr(C)]
pub struct ALCcontext {
    _private: [u8; 0],
}

const AL_TRUE: ALboolean = 1;
const ALC_TRUE: ALCboolean = 1;

const AL_NONE: ALenum = 0;
const AL_INVERSE_DISTANCE: ALenum = 0xD001;
const AL_INVERSE_DISTANCE_CLAMPED: ALenum = 0xD002;
const AL_LINEAR_DISTANCE: ALenum = 0xD003;
const AL_LINEAR_DISTANCE_CLAMPED: ALenum = 0xD004;
const AL_EXPONENT_DISTANCE: ALenum = 0xD005;
const AL_EXPONENT_DISTANCE_CLAMPED: ALenum = 0xD006;
const AL_POSITION: ALenum = 0x1004;
const AL_VELOCITY: ALenum = 0x1006;
const AL_LOOPING: ALenum = 0x1007;
const AL_ORIENTATION: ALenum = 0x100F;
const AL_GAIN: ALenum = 0x100A;
const AL_PITCH: ALenum = 0x1003;
const AL_BUFFER: ALenum = 0x1009;
const AL_SOURCE_STATE: ALenum = 0x1010;
const AL_STOPPED: ALenum = 0x1014;

const AL_FORMAT_MONO16: ALenum = 0x1101;

#[derive(Debug, Error)]
pub enum OpenalError {
    #[error("failed to load OpenAL library")]
    LibraryLoadFailed,
    #[error("missing required OpenAL symbol: {0}")]
    MissingSymbol(&'static str),
    #[error("failed to open OpenAL device")]
    OpenDeviceFailed,
    #[error("failed to create OpenAL context")]
    CreateContextFailed,
    #[error("failed to make OpenAL context current")]
    MakeContextCurrentFailed,
    #[error("OpenAL operation failed: {0}")]
    AlError(&'static str),
    #[error("OpenAL buffer key already exists: {0}")]
    BufferKeyExists(BufferKey),
    #[error("OpenAL buffer key missing: {0}")]
    BufferKeyMissing(BufferKey),
    #[error("OpenAL buffer data exceeds 32-bit limits")]
    BufferDataTooLarge,
    #[error("OpenAL sample rate exceeds 32-bit limits")]
    SampleRateTooLarge,
    #[error("OpenAL returned an invalid buffer handle")]
    InvalidBufferHandle,
    #[error("OpenAL returned an invalid source handle")]
    InvalidSourceHandle,
    #[error("OpenAL source limit reached")]
    SourceLimitReached,
}

type AlGenBuffers = unsafe extern "C" fn(ALsizei, *mut ALuint);
type AlDeleteBuffers = unsafe extern "C" fn(ALsizei, *const ALuint);
type AlBufferData = unsafe extern "C" fn(ALuint, ALenum, *const ALvoid, ALsizei, ALsizei);
type AlGenSources = unsafe extern "C" fn(ALsizei, *mut ALuint);
type AlDeleteSources = unsafe extern "C" fn(ALsizei, *const ALuint);
type AlSourcei = unsafe extern "C" fn(ALuint, ALenum, ALint);
type AlSourcef = unsafe extern "C" fn(ALuint, ALenum, ALfloat);
type AlSource3f = unsafe extern "C" fn(ALuint, ALenum, ALfloat, ALfloat, ALfloat);
type AlSourcePlay = unsafe extern "C" fn(ALuint);
type AlSourceStop = unsafe extern "C" fn(ALuint);
type AlGetSourcei = unsafe extern "C" fn(ALuint, ALenum, *mut ALint);
type AlListener3f = unsafe extern "C" fn(ALenum, ALfloat, ALfloat, ALfloat);
type AlListenerfv = unsafe extern "C" fn(ALenum, *const ALfloat);
type AlListenerf = unsafe extern "C" fn(ALenum, ALfloat);
type AlDistanceModel = unsafe extern "C" fn(ALenum);
type AlGetError = unsafe extern "C" fn() -> ALenum;

type AlcOpenDevice = unsafe extern "C" fn(*const ALCchar) -> *mut ALCdevice;
type AlcCloseDevice = unsafe extern "C" fn(*mut ALCdevice) -> ALCboolean;
type AlcCreateContext = unsafe extern "C" fn(*mut ALCdevice, *const ALCint) -> *mut ALCcontext;
type AlcDestroyContext = unsafe extern "C" fn(*mut ALCcontext);
type AlcMakeContextCurrent = unsafe extern "C" fn(*mut ALCcontext) -> ALCboolean;
type AlcGetError = unsafe extern "C" fn(*mut ALCdevice) -> ALCenum;
type AlcGetIntegerv = unsafe extern "C" fn(*mut ALCdevice, ALCenum, ALCsizei, *mut ALCint);
type AlcIsExtensionPresent = unsafe extern "C" fn(*mut ALCdevice, *const ALCchar) -> ALCboolean;
type AlcGetEnumValue = unsafe extern "C" fn(*mut ALCdevice, *const ALCchar) -> ALCenum;

struct OpenalApi {
    _lib: Library,

    al_gen_buffers: AlGenBuffers,
    al_delete_buffers: AlDeleteBuffers,
    al_buffer_data: AlBufferData,
    al_gen_sources: AlGenSources,
    al_delete_sources: AlDeleteSources,
    al_source_i: AlSourcei,
    al_source_f: AlSourcef,
    al_source_3f: AlSource3f,
    al_source_play: AlSourcePlay,
    al_source_stop: AlSourceStop,
    al_get_source_i: AlGetSourcei,
    al_listener_3f: AlListener3f,
    al_listener_fv: AlListenerfv,
    al_listener_f: AlListenerf,
    al_distance_model: AlDistanceModel,
    al_get_error: AlGetError,

    alc_open_device: AlcOpenDevice,
    alc_close_device: AlcCloseDevice,
    alc_create_context: AlcCreateContext,
    alc_destroy_context: AlcDestroyContext,
    alc_make_context_current: AlcMakeContextCurrent,
    alc_get_error: AlcGetError,
    alc_get_integerv: AlcGetIntegerv,
    alc_is_extension_present: AlcIsExtensionPresent,
    alc_get_enum_value: AlcGetEnumValue,
}

impl OpenalApi {
    fn load() -> Result<Self, OpenalError> {
        let lib = load_openal_library().ok_or(OpenalError::LibraryLoadFailed)?;

        unsafe {
            Ok(Self {
                al_gen_buffers: load_symbol(&lib, b"alGenBuffers\0")?,
                al_delete_buffers: load_symbol(&lib, b"alDeleteBuffers\0")?,
                al_buffer_data: load_symbol(&lib, b"alBufferData\0")?,
                al_gen_sources: load_symbol(&lib, b"alGenSources\0")?,
                al_delete_sources: load_symbol(&lib, b"alDeleteSources\0")?,
                al_source_i: load_symbol(&lib, b"alSourcei\0")?,
                al_source_f: load_symbol(&lib, b"alSourcef\0")?,
                al_source_3f: load_symbol(&lib, b"alSource3f\0")?,
                al_source_play: load_symbol(&lib, b"alSourcePlay\0")?,
                al_source_stop: load_symbol(&lib, b"alSourceStop\0")?,
                al_get_source_i: load_symbol(&lib, b"alGetSourcei\0")?,
                al_listener_3f: load_symbol(&lib, b"alListener3f\0")?,
                al_listener_fv: load_symbol(&lib, b"alListenerfv\0")?,
                al_listener_f: load_symbol(&lib, b"alListenerf\0")?,
                al_distance_model: load_symbol(&lib, b"alDistanceModel\0")?,
                al_get_error: load_symbol(&lib, b"alGetError\0")?,
                alc_open_device: load_symbol(&lib, b"alcOpenDevice\0")?,
                alc_close_device: load_symbol(&lib, b"alcCloseDevice\0")?,
                alc_create_context: load_symbol(&lib, b"alcCreateContext\0")?,
                alc_destroy_context: load_symbol(&lib, b"alcDestroyContext\0")?,
                alc_make_context_current: load_symbol(&lib, b"alcMakeContextCurrent\0")?,
                alc_get_error: load_symbol(&lib, b"alcGetError\0")?,
                alc_get_integerv: load_symbol(&lib, b"alcGetIntegerv\0")?,
                alc_is_extension_present: load_symbol(&lib, b"alcIsExtensionPresent\0")?,
                alc_get_enum_value: load_symbol(&lib, b"alcGetEnumValue\0")?,
                _lib: lib,
            })
        }
    }

    fn alc_enum_value(&self, device: *mut ALCdevice, name: &CStr) -> ALCenum {
        unsafe { (self.alc_get_enum_value)(device, name.as_ptr()) }
    }

    fn alc_has_extension(&self, device: *mut ALCdevice, name: &CStr) -> bool {
        unsafe { (self.alc_is_extension_present)(device, name.as_ptr()) == AL_TRUE as ALCboolean }
    }

    fn check_al(&self, context: &'static str) -> Result<(), OpenalError> {
        let err = unsafe { (self.al_get_error)() };
        if err == AL_NONE {
            return Ok(());
        }
        warn!(al_error = err, context, "OpenAL error");
        Err(OpenalError::AlError(context))
    }

    fn check_alc(&self, device: *mut ALCdevice, context: &'static str) -> Result<(), OpenalError> {
        let err = unsafe { (self.alc_get_error)(device) };
        if err == AL_NONE {
            return Ok(());
        }
        warn!(alc_error = err, context, "OpenAL ALC error");
        Err(OpenalError::AlError(context))
    }
}

unsafe fn load_symbol<T: Copy>(lib: &Library, symbol: &'static [u8]) -> Result<T, OpenalError> {
    lib.get::<T>(symbol)
        .map(|sym| *sym)
        .map_err(|_| OpenalError::MissingSymbol(std::str::from_utf8(symbol).unwrap_or("symbol")))
}

fn load_openal_library() -> Option<Library> {
    #[cfg(windows)]
    const CANDIDATES: [&str; 1] = ["OpenAL32.dll"];
    #[cfg(target_os = "linux")]
    const CANDIDATES: [&str; 2] = ["libopenal.so.1", "libopenal.so"];
    #[cfg(target_os = "macos")]
    const CANDIDATES: [&str; 2] = ["libopenal.dylib", "OpenAL.framework/OpenAL"];

    let mut candidate_paths = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            for candidate in CANDIDATES {
                candidate_paths.push(exe_dir.join(candidate));
            }
        }
    }

    for candidate in &candidate_paths {
        match unsafe { Library::new(candidate) } {
            Ok(lib) => {
                debug!(library = %candidate.display(), "Loaded OpenAL library");
                return Some(lib);
            }
            Err(err) => {
                debug!(library = %candidate.display(), error = %err, "Failed to load OpenAL library");
            }
        }
    }

    for candidate in CANDIDATES {
        match unsafe { Library::new(candidate) } {
            Ok(lib) => {
                debug!(library = candidate, "Loaded OpenAL library");
                return Some(lib);
            }
            Err(err) => {
                debug!(library = candidate, error = %err, "Failed to load OpenAL library");
            }
        }
    }
    None
}

pub struct OpenalEngine {
    api: OpenalApi,
    device: Option<NonNull<ALCdevice>>,
    context: Option<NonNull<ALCcontext>>,
    buffers: HashMap<BufferKey, ALuint>,
    sources: Vec<ALuint>,
    loop_source: Option<(BufferKey, ALuint)>,
    max_sources: usize,
    hrtf_active: bool,
    output_mode_name: Option<&'static str>,
    output_mode_raw: Option<ALCint>,
    distance_model: DistanceModel,
}

impl OpenalEngine {
    pub fn new(
        render_mode: AudioRenderMode,
        preferred_device: Option<&str>,
        max_sources: usize,
        distance_model: DistanceModel,
    ) -> Result<Self, OpenalError> {
        let api = OpenalApi::load()?;

        let device_name = preferred_device.and_then(|name| CString::new(name).ok());
        let device_name_ptr = device_name
            .as_ref()
            .map_or(ptr::null(), |name| name.as_ptr());

        let device_ptr = unsafe { (api.alc_open_device)(device_name_ptr) };
        let device = NonNull::new(device_ptr).ok_or(OpenalError::OpenDeviceFailed)?;
        api.check_alc(device.as_ptr(), "alcOpenDevice")?;

        let attributes = build_context_attributes(&api, device.as_ptr(), render_mode);
        let context_ptr = unsafe { (api.alc_create_context)(device.as_ptr(), attributes.as_ptr()) };
        let context = match NonNull::new(context_ptr) {
            Some(context) => context,
            None => {
                unsafe {
                    (api.alc_close_device)(device.as_ptr());
                }
                return Err(OpenalError::CreateContextFailed);
            }
        };
        api.check_alc(device.as_ptr(), "alcCreateContext")?;

        if unsafe { (api.alc_make_context_current)(context.as_ptr()) } != AL_TRUE as ALCboolean {
            unsafe {
                (api.alc_destroy_context)(context.as_ptr());
                (api.alc_close_device)(device.as_ptr());
            }
            return Err(OpenalError::MakeContextCurrentFailed);
        }
        api.check_alc(device.as_ptr(), "alcMakeContextCurrent")?;

        let hrtf_active = query_hrtf_active(&api, device.as_ptr());
        let (output_mode_name, output_mode_raw) = query_output_mode(&api, device.as_ptr());

        let mut engine = Self {
            api,
            device: Some(device),
            context: Some(context),
            buffers: HashMap::new(),
            sources: Vec::new(),
            loop_source: None,
            max_sources,
            hrtf_active,
            output_mode_name,
            output_mode_raw,
            distance_model,
        };

        engine.set_distance_model(distance_model)?;

        Ok(engine)
    }

    pub fn status(&self) -> (bool, Option<&'static str>, Option<ALCint>, DistanceModel) {
        (
            self.hrtf_active,
            self.output_mode_name,
            self.output_mode_raw,
            self.distance_model,
        )
    }

    pub fn loaded_buffers(&self) -> usize {
        self.buffers.len()
    }

    pub fn active_sources(&self) -> usize {
        self.sources.len() + usize::from(self.loop_source.is_some())
    }

    pub fn recreate(
        &mut self,
        render_mode: AudioRenderMode,
        preferred_device: Option<&str>,
        distance_model: DistanceModel,
    ) -> Result<(), OpenalError> {
        self.shutdown();

        *self = Self::new(
            render_mode,
            preferred_device,
            self.max_sources,
            distance_model,
        )?;
        Ok(())
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), OpenalError> {
        let gain = if muted { 0.0 } else { 1.0 };
        unsafe { (self.api.al_listener_f)(AL_GAIN, gain) };
        self.api.check_al("alListenerf(AL_GAIN)")?;
        Ok(())
    }

    pub fn set_listener(&self, listener: ListenerFrame) -> Result<(), OpenalError> {
        let position = sanitize_vec3(listener.position);
        let velocity = sanitize_vec3(listener.velocity);
        unsafe {
            (self.api.al_listener_3f)(AL_POSITION, position.x, position.y, position.z);
            (self.api.al_listener_3f)(AL_VELOCITY, velocity.x, velocity.y, velocity.z);
        }
        self.api.check_al("alListener3f")?;

        let forward = sanitize_unit_vector(listener.forward, Vec3::NEG_Z);
        let up = sanitize_unit_vector(listener.up, Vec3::Y);
        let orientation: [ALfloat; 6] = [forward.x, forward.y, forward.z, up.x, up.y, up.z];
        unsafe { (self.api.al_listener_fv)(AL_ORIENTATION, orientation.as_ptr()) };
        self.api.check_al("alListenerfv(AL_ORIENTATION)")?;

        Ok(())
    }

    pub fn set_distance_model(&mut self, model: DistanceModel) -> Result<(), OpenalError> {
        let value = match model {
            DistanceModel::None => AL_NONE,
            DistanceModel::Inverse => AL_INVERSE_DISTANCE,
            DistanceModel::InverseClamped => AL_INVERSE_DISTANCE_CLAMPED,
            DistanceModel::Linear => AL_LINEAR_DISTANCE,
            DistanceModel::LinearClamped => AL_LINEAR_DISTANCE_CLAMPED,
            DistanceModel::Exponent => AL_EXPONENT_DISTANCE,
            DistanceModel::ExponentClamped => AL_EXPONENT_DISTANCE_CLAMPED,
        };
        unsafe { (self.api.al_distance_model)(value) };
        self.api.check_al("alDistanceModel")?;
        self.distance_model = model;
        Ok(())
    }

    pub fn create_buffer(
        &mut self,
        key: BufferKey,
        decoded: &DecodedAudioMono16,
    ) -> Result<(), OpenalError> {
        if self.buffers.contains_key(&key) {
            return Err(OpenalError::BufferKeyExists(key));
        }

        let data_len = decoded
            .samples
            .len()
            .checked_mul(std::mem::size_of::<i16>())
            .and_then(al_size_from_usize)
            .ok_or(OpenalError::BufferDataTooLarge)?;
        let sample_rate =
            al_size_from_u32(decoded.sample_rate_hz).ok_or(OpenalError::SampleRateTooLarge)?;

        let mut buffer = 0;
        unsafe { (self.api.al_gen_buffers)(1, &mut buffer) };
        self.api.check_al("alGenBuffers")?;
        if buffer == 0 {
            return Err(OpenalError::InvalidBufferHandle);
        }

        unsafe {
            (self.api.al_buffer_data)(
                buffer,
                AL_FORMAT_MONO16,
                decoded.samples.as_ptr() as *const ALvoid,
                data_len,
                sample_rate,
            );
        }
        self.api.check_al("alBufferData")?;

        self.buffers.insert(key, buffer);
        Ok(())
    }

    pub fn play_one_shot(
        &mut self,
        key: BufferKey,
        params: PlayOneShotParams,
    ) -> Result<(), OpenalError> {
        if self.sources.len() + usize::from(self.loop_source.is_some()) >= self.max_sources {
            return Err(OpenalError::SourceLimitReached);
        }
        let Some(&buffer) = self.buffers.get(&key) else {
            return Err(OpenalError::BufferKeyMissing(key));
        };

        let mut source = 0;
        unsafe { (self.api.al_gen_sources)(1, &mut source) };
        self.api.check_al("alGenSources")?;
        if source == 0 {
            return Err(OpenalError::InvalidSourceHandle);
        }

        let position = sanitize_vec3(params.position);
        unsafe {
            (self.api.al_source_i)(source, AL_BUFFER, buffer as ALint);
            (self.api.al_source_f)(source, AL_GAIN, params.gain);
            (self.api.al_source_f)(source, AL_PITCH, params.pitch);
            (self.api.al_source_3f)(source, AL_POSITION, position.x, position.y, position.z);
            (self.api.al_source_play)(source);
        }
        self.api.check_al("alSourcePlay")?;

        self.sources.push(source);
        Ok(())
    }

    pub fn start_loop(
        &mut self,
        key: BufferKey,
        params: PlayOneShotParams,
    ) -> Result<(), OpenalError> {
        let Some(&buffer) = self.buffers.get(&key) else {
            return Err(OpenalError::BufferKeyMissing(key));
        };

        let position = sanitize_vec3(params.position);
        if let Some((existing_key, source)) = self.loop_source {
            if existing_key == key {
                unsafe {
                    (self.api.al_source_i)(source, AL_BUFFER, buffer as ALint);
                    (self.api.al_source_i)(source, AL_LOOPING, AL_TRUE as ALint);
                    (self.api.al_source_f)(source, AL_GAIN, params.gain);
                    (self.api.al_source_f)(source, AL_PITCH, params.pitch);
                    (self.api.al_source_3f)(
                        source,
                        AL_POSITION,
                        position.x,
                        position.y,
                        position.z,
                    );
                    (self.api.al_source_play)(source);
                }
                self.api.check_al("alSourcePlay(loop)")?;
                return Ok(());
            }
            self.stop_loop()?;
        }

        if self.sources.len() + usize::from(self.loop_source.is_some()) >= self.max_sources {
            return Err(OpenalError::SourceLimitReached);
        }

        let mut source = 0;
        unsafe { (self.api.al_gen_sources)(1, &mut source) };
        self.api.check_al("alGenSources(loop)")?;
        if source == 0 {
            return Err(OpenalError::InvalidSourceHandle);
        }

        unsafe {
            (self.api.al_source_i)(source, AL_BUFFER, buffer as ALint);
            (self.api.al_source_i)(source, AL_LOOPING, AL_TRUE as ALint);
            (self.api.al_source_f)(source, AL_GAIN, params.gain);
            (self.api.al_source_f)(source, AL_PITCH, params.pitch);
            (self.api.al_source_3f)(source, AL_POSITION, position.x, position.y, position.z);
            (self.api.al_source_play)(source);
        }
        self.api.check_al("alSourcePlay(loop)")?;

        self.loop_source = Some((key, source));
        Ok(())
    }

    pub fn stop_loop(&mut self) -> Result<(), OpenalError> {
        let Some((_, source)) = self.loop_source.take() else {
            return Ok(());
        };

        unsafe {
            (self.api.al_source_stop)(source);
            (self.api.al_delete_sources)(1, &source);
        }
        let _ = self.api.check_al("alDeleteSources(loop)");
        Ok(())
    }

    pub fn cleanup_finished_sources(&mut self) {
        let mut i = 0;
        while i < self.sources.len() {
            let source = self.sources[i];
            let mut state: ALint = 0;
            unsafe { (self.api.al_get_source_i)(source, AL_SOURCE_STATE, &mut state) };
            if self.api.check_al("alGetSourcei").is_err() {
                i += 1;
                continue;
            }
            if state as ALenum == AL_STOPPED {
                unsafe { (self.api.al_delete_sources)(1, &source) };
                let _ = self.api.check_al("alDeleteSources");
                self.sources.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub fn shutdown(&mut self) {
        let _ = self.stop_loop();
        for source in self.sources.drain(..) {
            unsafe { (self.api.al_delete_sources)(1, &source) };
        }
        for buffer in self.buffers.drain().map(|(_, buffer)| buffer) {
            unsafe { (self.api.al_delete_buffers)(1, &buffer) };
        }

        unsafe {
            (self.api.alc_make_context_current)(ptr::null_mut());
        }

        if let Some(context) = self.context.take() {
            unsafe {
                (self.api.alc_destroy_context)(context.as_ptr());
            }
        }

        if let Some(device) = self.device.take() {
            unsafe {
                (self.api.alc_close_device)(device.as_ptr());
            }
        }

        self.hrtf_active = false;
        self.output_mode_name = None;
        self.output_mode_raw = None;
        self.distance_model = DistanceModel::None;
    }
}

impl Drop for OpenalEngine {
    fn drop(&mut self) {
        if self.device.is_some() {
            self.shutdown();
        }
    }
}

fn al_size_from_usize(value: usize) -> Option<ALsizei> {
    i32::try_from(value).ok().map(|value| value as ALsizei)
}

fn al_size_from_u32(value: u32) -> Option<ALsizei> {
    i32::try_from(value).ok().map(|value| value as ALsizei)
}

fn sanitize_vec3(v: Vec3) -> Vec3 {
    if v.is_finite() {
        v
    } else {
        Vec3::ZERO
    }
}

fn sanitize_unit_vector(v: Vec3, fallback: Vec3) -> Vec3 {
    if !v.is_finite() || v.length_squared() < 0.0001 {
        return fallback;
    }
    v.normalize()
}
fn build_context_attributes(
    api: &OpenalApi,
    device: *mut ALCdevice,
    render_mode: AudioRenderMode,
) -> Vec<ALCint> {
    let mut attrs: Vec<ALCint> = Vec::new();

    let hrtf_ext = c"ALC_SOFT_HRTF";
    if render_mode == AudioRenderMode::HeadphonesHrtf && api.alc_has_extension(device, hrtf_ext) {
        let hrtf_key = api.alc_enum_value(device, cstr("ALC_HRTF_SOFT\0"));
        if hrtf_key != 0 {
            attrs.push(hrtf_key as ALCint);
            attrs.push(ALC_TRUE as ALCint);
        }
    }

    let output_ext = c"ALC_SOFT_output_mode";
    if api.alc_has_extension(device, output_ext) {
        let output_mode_key = api.alc_enum_value(device, cstr("ALC_OUTPUT_MODE_SOFT\0"));
        if output_mode_key != 0 {
            match render_mode {
                AudioRenderMode::StereoClean | AudioRenderMode::HeadphonesHrtf => {
                    let stereo = api.alc_enum_value(device, cstr("ALC_STEREO_SOFT\0"));
                    if stereo != 0 {
                        attrs.push(output_mode_key as ALCint);
                        attrs.push(stereo as ALCint);
                    }
                }
                AudioRenderMode::SurroundAuto => {
                    let any = api.alc_enum_value(device, cstr("ALC_ANY_SOFT\0"));
                    if any != 0 {
                        attrs.push(output_mode_key as ALCint);
                        attrs.push(any as ALCint);
                    }
                }
                AudioRenderMode::Auto => {
                    let any = api.alc_enum_value(device, cstr("ALC_ANY_SOFT\0"));
                    if any != 0 {
                        attrs.push(output_mode_key as ALCint);
                        attrs.push(any as ALCint);
                    }
                }
            }
        }
    }

    attrs.push(0);
    attrs
}

fn query_hrtf_active(api: &OpenalApi, device: *mut ALCdevice) -> bool {
    let hrtf_ext = c"ALC_SOFT_HRTF";
    if !api.alc_has_extension(device, hrtf_ext) {
        return false;
    }

    let enabled_key = api.alc_enum_value(device, cstr("ALC_HRTF_SOFT\0"));
    if enabled_key != 0 {
        let mut value: ALCint = 0;
        unsafe { (api.alc_get_integerv)(device, enabled_key, 1, &mut value) };
        if api
            .check_alc(device, "alcGetIntegerv(ALC_HRTF_SOFT)")
            .is_ok()
        {
            return value != 0;
        }
    }

    let status_key = api.alc_enum_value(device, cstr("ALC_HRTF_STATUS_SOFT\0"));
    if status_key == 0 {
        return false;
    }

    let mut value: ALCint = 0;
    unsafe { (api.alc_get_integerv)(device, status_key, 1, &mut value) };
    if api
        .check_alc(device, "alcGetIntegerv(ALC_HRTF_STATUS_SOFT)")
        .is_err()
    {
        return false;
    }

    let enabled = api.alc_enum_value(device, cstr("ALC_HRTF_ENABLED_SOFT\0")) as ALCint;
    let required = api.alc_enum_value(device, cstr("ALC_HRTF_REQUIRED_SOFT\0")) as ALCint;
    let detected =
        api.alc_enum_value(device, cstr("ALC_HRTF_HEADPHONES_DETECTED_SOFT\0")) as ALCint;

    value != 0 && (value == enabled || value == required || value == detected)
}

fn query_output_mode(
    api: &OpenalApi,
    device: *mut ALCdevice,
) -> (Option<&'static str>, Option<ALCint>) {
    let output_ext = c"ALC_SOFT_output_mode";
    if !api.alc_has_extension(device, output_ext) {
        return (None, None);
    }

    let output_key = api.alc_enum_value(device, cstr("ALC_OUTPUT_MODE_SOFT\0"));
    if output_key == 0 {
        return (None, None);
    }

    let mut value: ALCint = 0;
    unsafe { (api.alc_get_integerv)(device, output_key, 1, &mut value) };
    if api
        .check_alc(device, "alcGetIntegerv(ALC_OUTPUT_MODE_SOFT)")
        .is_err()
    {
        return (None, None);
    }

    const OUTPUT_MODES: [(&str, &str); 11] = [
        ("ALC_MONO_SOFT\0", "mono"),
        ("ALC_STEREO_SOFT\0", "stereo"),
        ("ALC_STEREO_BASIC_SOFT\0", "stereo-basic"),
        ("ALC_STEREO_UHJ_SOFT\0", "stereo-uhj"),
        ("ALC_STEREO_HRTF_SOFT\0", "stereo-hrtf"),
        ("ALC_QUAD_SOFT\0", "quad"),
        ("ALC_5POINT1_SOFT\0", "5.1"),
        ("ALC_6POINT1_SOFT\0", "6.1"),
        ("ALC_7POINT1_SOFT\0", "7.1"),
        ("ALC_BFORMAT3D_SOFT\0", "bformat3d"),
        ("ALC_ANY_SOFT\0", "auto"),
    ];

    let mut enum_values = Vec::new();
    for (enum_name, label) in OUTPUT_MODES {
        let mode = api.alc_enum_value(device, cstr(enum_name));
        if mode != 0 {
            enum_values.push((label, mode as ALCint));
            if value == mode as ALCint {
                return (Some(label), Some(value));
            }
        }
    }

    warn!(
        output_mode = value,
        output_key = output_key,
        enum_values = ?enum_values,
        "OpenAL output mode not recognized"
    );
    (Some("unknown"), Some(value))
}

fn cstr(bytes: &'static str) -> &'static CStr {
    CStr::from_bytes_with_nul(bytes.as_bytes()).expect("CStr must be nul-terminated")
}
