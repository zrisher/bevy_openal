use bevy_app::{App, Plugin};
use bevy_asset::{io::Reader, Asset, AssetApp, AssetLoader, LoadContext};
use bevy_reflect::TypePath;

#[derive(Asset, TypePath, Debug, Clone)]
pub struct OpenalAudioBytes {
    pub bytes: Vec<u8>,
}

#[derive(TypePath)]
pub struct OpenalAudioBytesLoader;

impl AssetLoader for OpenalAudioBytesLoader {
    type Asset = OpenalAudioBytes;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(OpenalAudioBytes { bytes })
    }

    fn extensions(&self) -> &[&str] {
        &["wav", "ogg", "flac", "mp3"]
    }
}

/// Registers `OpenalAudioBytes` as a Bevy asset and installs a raw-bytes loader for common
/// audio file extensions. This is intended to support OpenAL-based playback without enabling
/// Bevy's built-in audio output plugin.
pub struct BevyOpenalAssetsPlugin;

impl Plugin for BevyOpenalAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<OpenalAudioBytes>()
            .register_asset_loader(OpenalAudioBytesLoader);
    }
}
