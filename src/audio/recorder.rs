//! WAV file recording using hound
//!
//! Records audio samples to WAV files in 16kHz mono format for Whisper compatibility.

use hound::{WavSpec, WavWriter};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// WAV file recorder
pub struct WavRecorder {
    spec: WavSpec,
    recordings_dir: PathBuf,
}

impl WavRecorder {
    /// Create a new WAV recorder
    ///
    /// Uses 16kHz mono f32 format by default for Whisper compatibility
    pub fn new() -> Self {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        // Default recordings directory
        let recordings_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("adlib")
            .join("recordings");

        Self {
            spec,
            recordings_dir,
        }
    }

    /// Set the recordings directory
    pub fn with_recordings_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.recordings_dir = dir.as_ref().to_path_buf();
        self
    }

    /// Set the sample rate
    pub fn with_sample_rate(mut self, rate: u32) -> Self {
        self.spec.sample_rate = rate;
        self
    }

    /// Get the recordings directory
    pub fn recordings_dir(&self) -> &Path {
        &self.recordings_dir
    }

    /// Ensure the recordings directory exists
    pub fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.recordings_dir)
    }

    /// Generate a unique filename for a new recording
    pub fn generate_filename(&self) -> PathBuf {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let uuid = uuid::Uuid::new_v4().to_string()[..8].to_string();
        self.recordings_dir
            .join(format!("recording_{}_{}.wav", timestamp, uuid))
    }

    /// Save samples to a WAV file
    ///
    /// Returns the path to the saved file
    pub fn save(&self, samples: &[f32], filename: Option<&Path>) -> Result<PathBuf, String> {
        self.ensure_dir()
            .map_err(|e| format!("Failed to create recordings directory: {}", e))?;

        let path = match filename {
            Some(p) => p.to_path_buf(),
            None => self.generate_filename(),
        };

        let file = File::create(&path)
            .map_err(|e| format!("Failed to create file: {}", e))?;

        let writer = BufWriter::new(file);
        let mut wav_writer = WavWriter::new(writer, self.spec)
            .map_err(|e| format!("Failed to create WAV writer: {}", e))?;

        for &sample in samples {
            wav_writer
                .write_sample(sample)
                .map_err(|e| format!("Failed to write sample: {}", e))?;
        }

        wav_writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV file: {}", e))?;

        Ok(path)
    }

    /// Load samples from a WAV file
    ///
    /// Returns the samples and sample rate
    pub fn load(path: impl AsRef<Path>) -> Result<(Vec<f32>, u32), String> {
        let reader = hound::WavReader::open(path.as_ref())
            .map_err(|e| format!("Failed to open WAV file: {}", e))?;

        let spec = reader.spec();
        let sample_rate = spec.sample_rate;

        let samples: Result<Vec<f32>, _> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .collect(),
            hound::SampleFormat::Int => {
                // Convert integer samples to float
                let bits = spec.bits_per_sample;
                let max_value = (1 << (bits - 1)) as f32;
                reader
                    .into_samples::<i32>()
                    .map(|s| s.map(|v| v as f32 / max_value))
                    .collect()
            }
        };

        let samples = samples.map_err(|e| format!("Failed to read samples: {}", e))?;

        Ok((samples, sample_rate))
    }

    /// Get duration of samples in seconds
    pub fn duration_seconds(sample_count: usize, sample_rate: u32) -> f64 {
        sample_count as f64 / sample_rate as f64
    }

    /// List all recordings in the recordings directory
    pub fn list_recordings(&self) -> Result<Vec<PathBuf>, String> {
        self.ensure_dir()
            .map_err(|e| format!("Failed to access recordings directory: {}", e))?;

        let mut recordings: Vec<PathBuf> = std::fs::read_dir(&self.recordings_dir)
            .map_err(|e| format!("Failed to read recordings directory: {}", e))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .map(|ext| ext.to_string_lossy().to_lowercase() == "wav")
                    .unwrap_or(false)
            })
            .collect();

        // Sort by modification time, newest first
        recordings.sort_by(|a, b| {
            let a_time = a.metadata().and_then(|m| m.modified()).ok();
            let b_time = b.metadata().and_then(|m| m.modified()).ok();
            b_time.cmp(&a_time)
        });

        Ok(recordings)
    }
}

impl Default for WavRecorder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_calculation() {
        assert_eq!(WavRecorder::duration_seconds(16000, 16000), 1.0);
        assert_eq!(WavRecorder::duration_seconds(32000, 16000), 2.0);
        assert_eq!(WavRecorder::duration_seconds(8000, 16000), 0.5);
    }
}
