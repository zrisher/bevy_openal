#[cfg(feature = "bevy-assets")]
mod bevy_assets;
mod bevy_plugin;
mod decode;
mod openal;
mod runtime;

#[cfg(feature = "build-support")]
pub mod build_support;

#[cfg(feature = "bevy-assets")]
pub use bevy_assets::{BevyOpenalAssetsPlugin, OpenalAudioBytes, OpenalAudioBytesLoader};
pub use bevy_plugin::{
    BevyOpenalPlugin, OpenalListener, OpenalPlayOneShot, OpenalRuntime, OpenalSettings,
    OpenalStatus,
};
pub use decode::{decode_to_mono_i16, DecodeError, DecodedAudioMono16};
pub use runtime::{
    AudioRenderMode, AudioRuntime, AudioRuntimeConfig, AudioRuntimeStatus, BufferKey,
    DistanceModel, ListenerFrame, PlayOneShotParams, RuntimeError,
};
