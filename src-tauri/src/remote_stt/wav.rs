//! In-memory WAV encoder for the remote STT upload path.

use std::io::Cursor;

/// Encode mono f32 PCM (range -1.0..=1.0) at the given sample rate as a
/// 16-bit PCM WAV file in memory.
pub fn encode_f32_to_wav_bytes(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, String> {
    use hound::{SampleFormat, WavSpec, WavWriter};

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut buf = Cursor::new(Vec::<u8>::new());
    {
        let mut writer =
            WavWriter::new(&mut buf, spec).map_err(|e| format!("WAV writer init failed: {e}"))?;
        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let value = (clamped * i16::MAX as f32) as i16;
            writer
                .write_sample(value)
                .map_err(|e| format!("WAV write_sample failed: {e}"))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("WAV finalize failed: {e}"))?;
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::WavReader;
    use std::io::Cursor;

    #[test]
    fn encodes_empty_samples_into_valid_wav() {
        let bytes = encode_f32_to_wav_bytes(&[], 16_000).unwrap();
        let reader = WavReader::new(Cursor::new(bytes)).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.bits_per_sample, 16);
    }

    #[test]
    fn round_trip_preserves_sample_count() {
        let samples: Vec<f32> = (0..1_000)
            .map(|i| (i as f32 / 1_000.0) * 0.5 - 0.25)
            .collect();
        let bytes = encode_f32_to_wav_bytes(&samples, 16_000).unwrap();
        let mut reader = WavReader::new(Cursor::new(bytes)).unwrap();
        let count = reader.samples::<i16>().count();
        assert_eq!(count, samples.len());
    }

    #[test]
    fn clamps_out_of_range_samples() {
        let samples = vec![2.0f32, -2.0, 0.0];
        let bytes = encode_f32_to_wav_bytes(&samples, 16_000).unwrap();
        let mut reader = WavReader::new(Cursor::new(bytes)).unwrap();
        let decoded: Vec<i16> = reader.samples::<i16>().filter_map(Result::ok).collect();
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0], i16::MAX);
        assert_eq!(decoded[1], -i16::MAX);
        assert_eq!(decoded[2], 0);
    }
}
