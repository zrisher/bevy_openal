use rodio::Source;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct DecodedAudioMono16 {
    pub sample_rate_hz: u32,
    pub samples: Vec<i16>,
}

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("audio decode failed: {0}")]
    Decoder(#[from] rodio::decoder::DecoderError),
}

pub fn decode_to_mono_i16(bytes: &[u8]) -> Result<DecodedAudioMono16, DecodeError> {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let decoder = rodio::Decoder::new(cursor)?;
    let sample_rate_hz = decoder.sample_rate();
    let channels = decoder.channels() as usize;

    let samples: Vec<i16> = decoder.convert_samples::<i16>().collect();
    let samples = downmix_to_mono_i16(samples, channels);

    Ok(DecodedAudioMono16 {
        sample_rate_hz,
        samples,
    })
}

fn downmix_to_mono_i16(samples: Vec<i16>, channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return samples;
    }

    let frame_count = samples.len() / channels;
    let mut mono = Vec::with_capacity(frame_count);

    for frame in samples.chunks_exact(channels) {
        let sum: i32 = frame.iter().map(|&sample| sample as i32).sum();
        let avg = sum / channels as i32;
        mono.push(avg.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    }

    mono
}
