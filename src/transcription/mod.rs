//! Audio transcription using Whisper
//!
//! Provides transcription of audio files using whisper-rs (whisper.cpp bindings).

use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Result of a transcription
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    /// Full transcribed text
    pub text: String,
    /// Individual segments with timestamps
    pub segments: Vec<TranscriptionSegment>,
}

/// A segment of transcribed text with timing info
#[derive(Debug, Clone)]
pub struct TranscriptionSegment {
    /// Start time in seconds
    pub start: f64,
    /// End time in seconds
    pub end: f64,
    /// Transcribed text for this segment
    pub text: String,
}

/// Transcription options
#[derive(Debug, Clone)]
pub struct TranscriptionOptions {
    /// Language code (e.g., "en", "auto" for auto-detect)
    pub language: Option<String>,
    /// Whether to translate to English
    pub translate: bool,
    /// Number of threads to use (0 = auto)
    pub n_threads: i32,
}

impl Default for TranscriptionOptions {
    fn default() -> Self {
        Self {
            language: None, // auto-detect
            translate: false,
            n_threads: 0, // auto
        }
    }
}

/// Transcription engine wrapping whisper-rs
pub struct TranscriptionEngine {
    ctx: WhisperContext,
}

impl TranscriptionEngine {
    /// Create a new transcription engine by loading a model
    pub fn new(model_path: &Path) -> Result<Self, String> {
        let ctx_params = WhisperContextParameters::default();

        let ctx = WhisperContext::new_with_params(
            model_path.to_str().ok_or("Invalid model path")?,
            ctx_params,
        )
        .map_err(|e| format!("Failed to load Whisper model: {}", e))?;

        Ok(Self { ctx })
    }

    /// Transcribe audio samples
    ///
    /// `samples` should be mono audio at 16kHz sample rate
    pub fn transcribe(
        &self,
        samples: &[f32],
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult, String> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        // Set language
        if let Some(ref lang) = options.language {
            if lang != "auto" {
                params.set_language(Some(lang));
            }
        }

        // Set translation mode
        params.set_translate(options.translate);

        // Set thread count
        if options.n_threads > 0 {
            params.set_n_threads(options.n_threads);
        }

        // Enable timestamps
        params.set_token_timestamps(true);

        // Create state and run transcription
        let mut state = self.ctx.create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        state.full(params, samples)
            .map_err(|e| format!("Transcription failed: {}", e))?;

        // Extract results
        let num_segments = state.full_n_segments();

        let mut segments = Vec::new();
        let mut full_text = String::new();

        for i in 0..num_segments {
            if let Some(segment) = state.get_segment(i) {
                let text = segment.to_str_lossy()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|_| "[transcription error]".to_string());
                let start_cs = segment.start_timestamp();
                let end_cs = segment.end_timestamp();

                // Convert timestamps from centiseconds to seconds
                let start_sec = start_cs as f64 / 100.0;
                let end_sec = end_cs as f64 / 100.0;

                if !full_text.is_empty() && !text.starts_with(' ') {
                    full_text.push(' ');
                }
                full_text.push_str(&text);

                segments.push(TranscriptionSegment {
                    start: start_sec,
                    end: end_sec,
                    text,
                });
            }
        }

        Ok(TranscriptionResult {
            text: full_text.trim().to_string(),
            segments,
        })
    }

    /// Transcribe a WAV file
    pub fn transcribe_file(
        &self,
        wav_path: &Path,
        options: &TranscriptionOptions,
    ) -> Result<TranscriptionResult, String> {
        // Load and convert audio to 16kHz mono
        let samples = load_wav_as_16khz_mono(wav_path)?;
        self.transcribe(&samples, options)
    }
}

/// Load a WAV file and convert to 16kHz mono f32 samples
fn load_wav_as_16khz_mono(path: &Path) -> Result<Vec<f32>, String> {
    let reader = hound::WavReader::open(path)
        .map_err(|e| format!("Failed to open WAV file: {}", e))?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as usize;

    // Read samples based on format
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            reader.into_samples::<f32>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to read samples: {}", e))?
        }
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_val = (1u32 << (bits - 1)) as f32;
            reader.into_samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max_val))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to read samples: {}", e))?
        }
    };

    // Convert to mono if stereo
    let mono_samples: Vec<f32> = if channels > 1 {
        samples
            .chunks(channels)
            .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        samples
    };

    // Resample to 16kHz if needed
    if sample_rate != 16000 {
        Ok(resample(&mono_samples, sample_rate, 16000))
    } else {
        Ok(mono_samples)
    }
}

/// Simple linear resampling
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;

        let sample = if idx + 1 < samples.len() {
            samples[idx] * (1.0 - frac as f32) + samples[idx + 1] * frac as f32
        } else {
            samples[idx.min(samples.len() - 1)]
        };

        output.push(sample);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample() {
        let samples = vec![0.0, 1.0, 0.0, -1.0];
        let resampled = resample(&samples, 4, 2);
        assert_eq!(resampled.len(), 2);
    }
}
