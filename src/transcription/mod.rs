//! Audio transcription using Whisper
//!
//! Provides transcription of audio files using whisper-rs (whisper.cpp bindings).

#![allow(dead_code)]

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
#[derive(Debug, Clone, Default)]
pub struct TranscriptionOptions {
    /// Language code (e.g., "en", "auto" for auto-detect)
    pub language: Option<String>,
    /// Whether to translate to English
    pub translate: bool,
    /// Number of threads to use (0 = auto)
    pub n_threads: i32,
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
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        state
            .full(params, samples)
            .map_err(|e| format!("Transcription failed: {}", e))?;

        // Extract results
        let num_segments = state.full_n_segments();

        let mut segments = Vec::new();
        let mut full_text = String::new();

        for i in 0..num_segments {
            if let Some(segment) = state.get_segment(i) {
                let text = segment
                    .to_str_lossy()
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
    let reader =
        hound::WavReader::open(path).map_err(|e| format!("Failed to open WAV file: {}", e))?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as usize;

    // Read samples based on format
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read samples: {}", e))?,
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_val = (1u32 << (bits - 1)) as f32;
            reader
                .into_samples::<i32>()
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
pub fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
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

/// Live transcriber for real-time streaming transcription
///
/// Transcribes accumulated audio in real-time with instant feedback.
/// Unlike a rolling window, this transcribes ALL accumulated audio each cycle,
/// so no speech is lost. Text updates/corrects as more audio arrives.
pub struct LiveTranscriber {
    ctx: WhisperContext,
    /// All accumulated audio samples for current segment
    buffer: Vec<f32>,
    samples_since_last_process: usize,
    /// Committed text from previous segments (finalized)
    committed_text: String,
    /// Current transcription of the buffer (updates in real-time)
    current_text: String,
    /// Number of consecutive silent processing cycles
    silence_count: usize,
    /// Calibrated VAD threshold based on ambient noise
    vad_threshold: f32,
    /// Whether calibration is complete
    calibrated: bool,
    /// Samples collected during calibration (3 seconds of quiet audio)
    calibration_samples: Vec<f32>,
    /// Consecutive quiet samples collected (reset if loud audio detected)
    quiet_streak_samples: usize,
}

impl LiveTranscriber {
    /// Sample rate expected by Whisper
    pub const SAMPLE_RATE: u32 = 16000;
    /// Process every 500ms for responsive feedback
    const STEP_SAMPLES: usize = 500 * 16; // 8000 samples = 0.5 seconds
    /// Maximum buffer size (30 seconds) - commit and clear if exceeded
    const MAX_BUFFER_SAMPLES: usize = 30 * 16000;
    /// Calibration duration in samples (3 seconds of quiet audio)
    const CALIBRATION_SAMPLES: usize = 3 * 16000;
    /// Chunk size for checking if audio is quiet (100ms)
    const CALIBRATION_CHUNK_SAMPLES: usize = 1600;
    /// Pre-calibration threshold to detect "quiet" audio
    /// This is a fixed threshold used before we know the actual ambient noise level
    const PRE_CALIBRATION_QUIET_THRESHOLD: f32 = 0.04;
    /// Minimum VAD threshold (even in quiet rooms)
    const MIN_VAD_THRESHOLD: f32 = 0.02;
    /// Multiplier above ambient noise for VAD threshold
    const VAD_MULTIPLIER: f32 = 3.0;
    /// Number of silent iterations before committing (1.5 seconds of silence)
    const SILENCE_COMMIT_THRESHOLD: usize = 3;

    /// Create a new live transcriber with a model
    pub fn new(model_path: &Path) -> Result<Self, String> {
        let ctx_params = WhisperContextParameters::default();

        let ctx = WhisperContext::new_with_params(
            model_path.to_str().ok_or("Invalid model path")?,
            ctx_params,
        )
        .map_err(|e| format!("Failed to load Whisper model: {}", e))?;

        Ok(Self {
            ctx,
            buffer: Vec::with_capacity(Self::MAX_BUFFER_SAMPLES),
            samples_since_last_process: 0,
            committed_text: String::new(),
            current_text: String::new(),
            silence_count: 0,
            vad_threshold: 0.02,
            calibrated: false,
            calibration_samples: Vec::with_capacity(Self::CALIBRATION_SAMPLES),
            quiet_streak_samples: 0,
        })
    }

    /// Check if calibration is complete
    pub fn is_calibrated(&self) -> bool {
        self.calibrated
    }

    /// Get calibration progress (0.0 to 1.0)
    pub fn calibration_progress(&self) -> f32 {
        if self.calibrated {
            1.0
        } else {
            self.calibration_samples.len() as f32 / Self::CALIBRATION_SAMPLES as f32
        }
    }

    /// Add new audio samples to the buffer
    pub fn add_samples(&mut self, samples: &[f32]) {
        // During calibration, wait for 3 seconds of quiet audio
        if !self.calibrated {
            // Process samples in chunks to check quietness
            let mut offset = 0;
            while offset < samples.len() {
                let chunk_end = (offset + Self::CALIBRATION_CHUNK_SAMPLES).min(samples.len());
                let chunk = &samples[offset..chunk_end];

                // Check if this chunk is quiet
                let chunk_rms = Self::calculate_rms(chunk);
                let is_quiet = chunk_rms < Self::PRE_CALIBRATION_QUIET_THRESHOLD;

                if is_quiet {
                    // Add to calibration samples
                    self.calibration_samples.extend_from_slice(chunk);
                    self.quiet_streak_samples += chunk.len();

                    // Check if we have enough quiet samples
                    if self.calibration_samples.len() >= Self::CALIBRATION_SAMPLES {
                        self.complete_calibration();
                        // Process any remaining samples normally
                        if chunk_end < samples.len() {
                            self.buffer.extend_from_slice(&samples[chunk_end..]);
                            self.samples_since_last_process += samples.len() - chunk_end;
                        }
                        return;
                    }
                } else {
                    // Loud audio detected - reset calibration
                    if !self.calibration_samples.is_empty() {
                        eprintln!(
                            "[CALIBRATION] Reset - loud audio detected (RMS: {:.4})",
                            chunk_rms
                        );
                    }
                    self.calibration_samples.clear();
                    self.quiet_streak_samples = 0;
                }

                offset = chunk_end;
            }
            return; // Don't add to main buffer during calibration
        }

        self.buffer.extend_from_slice(samples);
        self.samples_since_last_process += samples.len();
    }

    /// Check if buffer is getting too long and should be force-committed
    pub fn should_force_commit(&self) -> bool {
        self.buffer.len() >= Self::MAX_BUFFER_SAMPLES
    }

    /// Complete calibration by calculating VAD threshold from ambient noise
    fn complete_calibration(&mut self) {
        let ambient_rms = Self::calculate_rms(&self.calibration_samples);
        // Set threshold to be VAD_MULTIPLIER times the ambient noise, with a minimum
        self.vad_threshold = (ambient_rms * Self::VAD_MULTIPLIER).max(Self::MIN_VAD_THRESHOLD);
        self.calibrated = true;
        self.calibration_samples.clear(); // Free memory
        eprintln!(
            "VAD calibrated: ambient RMS = {:.4}, threshold = {:.4}",
            ambient_rms, self.vad_threshold
        );
    }

    /// Check if we have enough samples to process
    pub fn ready_to_process(&self) -> bool {
        self.calibrated && self.samples_since_last_process >= Self::STEP_SAMPLES
    }

    /// Calculate RMS (root mean square) of audio samples
    fn calculate_rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
        (sum_squares / samples.len() as f32).sqrt()
    }

    /// Check if text looks like a Whisper hallucination on silence
    fn is_hallucination(text: &str) -> bool {
        let lower = text.to_lowercase();
        let trimmed = lower.trim();

        // Common hallucination patterns (all lowercase - we compare against lowercased text)
        let hallucination_patterns = [
            // Music and audio markers
            "[music",
            "(music",
            "♪",
            "🎵",
            "[blank_audio]",
            "[silence",
            "(silence",
            "[audio",
            "(audio",
            // Non-speech sounds
            "[sigh",
            "(sigh",
            "[crying",
            "(crying",
            "[laughter",
            "(laughter",
            "[applause",
            "(applause",
            "[noise",
            "(noise",
            "[inaudible",
            "(inaudible",
            "[unintelligible",
            "(unintelligible",
            "[background",
            "(background",
            "[ambient",
            "(ambient",
            "[static",
            "(static",
            "[breathing",
            "(breathing",
            "[cough",
            "(cough",
            "[sneeze",
            "(sneeze",
            "[whisper",
            "(whisper",
            "[mumbl",
            "(mumbl",
            "[squeak",
            "(squeak",
            "[click",
            "(click",
            "[beep",
            "(beep",
            "[tone",
            "(tone",
            "[bell",
            "(bell",
            "[ring",
            "(ring",
            // Emotional/dramatic markers
            "[dramatic",
            "(dramatic",
            "[sad",
            "(sad",
            "[happy",
            "(happy",
            "[whistl",
            "(whistl",
            "[humm",
            "(humm",
            "[mimick",
            "(mimick",
            // Foreign language markers
            "[speaking",
            "(speaking",
            "[foreign",
            "(foreign",
            // Tech sounds
            "[xbox",
            "(xbox",
            "[windows",
            "(windows",
            // Whisper garbage patterns
            "...",
            "shh",
            "shhh",
            "hmm",
            "hush",
            "fash",
            "shook",
            "whoosh",
            "air whoosh",
            // Common false positives on silence - short phrases
            "you are the only",
            "your house",
            "i'll show you",
            "yet the few",
            "a few days",
            "and you have",
            "thank you",
            "thanks for",
            "bye",
            "goodbye",
            "i'm sorry",
            "sorry",
            "please come",
            "come forward",
            "famous for",
            "you will be",
        ];

        for pattern in hallucination_patterns {
            if trimmed.contains(pattern) {
                return true;
            }
        }

        // Also filter very short outputs that are just punctuation or single chars
        if trimmed.len() <= 2 {
            return true;
        }

        // Filter if it's mostly non-alphabetic
        let alpha_count = trimmed.chars().filter(|c| c.is_alphabetic()).count();
        if alpha_count < 3 {
            return true;
        }

        // Filter repetitive patterns (e.g., "and... and... and...")
        // Count how many times " and" appears - if it's too repetitive, it's garbage
        let and_count = trimmed.matches(" and").count() + trimmed.matches("and ").count();
        if and_count >= 3 {
            return true;
        }

        // Filter if text is very short with just common words
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        if words.len() <= 3 {
            // Check if all words are common filler words
            let filler_words = [
                "and", "the", "a", "an", "to", "of", "in", "is", "it", "you", "i",
            ];
            let filler_count = words.iter().filter(|w| filler_words.contains(w)).count();
            if filler_count == words.len() {
                return true;
            }
        }

        false
    }

    /// Process the current buffer and return transcription update
    /// Transcribes ALL accumulated audio for real-time feedback
    pub fn process(&mut self) -> Result<bool, String> {
        if self.buffer.is_empty() {
            return Ok(false);
        }

        // Reset counter
        self.samples_since_last_process = 0;

        // Check recent audio for VAD (last 500ms)
        let vad_samples = if self.buffer.len() > Self::STEP_SAMPLES {
            &self.buffer[self.buffer.len() - Self::STEP_SAMPLES..]
        } else {
            &self.buffer[..]
        };

        let rms = Self::calculate_rms(vad_samples);
        let is_silence = rms < self.vad_threshold;

        if is_silence {
            self.silence_count += 1;
            // Commit current segment after silence threshold
            if self.silence_count >= Self::SILENCE_COMMIT_THRESHOLD && !self.current_text.is_empty()
            {
                self.commit_segment();
                return Ok(true);
            }
            return Ok(false);
        }

        // Speech detected - reset silence counter
        self.silence_count = 0;

        // Transcribe ALL accumulated audio
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        state
            .full(params, &self.buffer)
            .map_err(|e| format!("Transcription failed: {}", e))?;

        // Extract text from all segments
        let num_segments = state.full_n_segments();
        let mut full_text = String::new();

        for i in 0..num_segments {
            if let Some(segment) = state.get_segment(i) {
                let text = segment
                    .to_str_lossy()
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                if !text.trim().is_empty() && !Self::is_hallucination(&text) {
                    if !full_text.is_empty() && !text.starts_with(' ') {
                        full_text.push(' ');
                    }
                    full_text.push_str(&text);
                }
            }
        }

        let full_text = full_text.trim().to_string();

        // Update current text if changed
        if !full_text.is_empty() && full_text != self.current_text {
            self.current_text = full_text;
            eprintln!(
                "[LIVE] '{}'",
                &self.current_text[..self.current_text.len().min(80)]
            );
            return Ok(true);
        }

        Ok(false)
    }

    /// Commit current segment to committed text and start fresh
    fn commit_segment(&mut self) {
        if !self.current_text.is_empty() {
            eprintln!(
                "[COMMIT] '{}' ({} chars)",
                &self.current_text[..self.current_text.len().min(60)],
                self.current_text.len()
            );
            if !self.committed_text.is_empty() {
                self.committed_text.push_str("\n\n"); // Paragraph break between segments
            }
            self.committed_text.push_str(&self.current_text);
            self.current_text.clear();
            self.buffer.clear(); // Start fresh for next segment
            self.silence_count = 0;
        }
    }

    /// Get the full transcript (committed + current)
    pub fn get_transcript(&self) -> String {
        if self.committed_text.is_empty() {
            self.current_text.clone()
        } else if self.current_text.is_empty() {
            self.committed_text.clone()
        } else {
            format!("{}\n\n{}", self.committed_text, self.current_text)
        }
    }

    /// Get just the committed text
    pub fn get_confirmed(&self) -> &str {
        &self.committed_text
    }

    /// Get just the current (live, may change) text
    pub fn get_tentative(&self) -> &str {
        &self.current_text
    }

    /// Clear the buffer and all text
    pub fn clear(&mut self) {
        eprintln!("[CLEAR] Clearing all transcript data");
        self.buffer.clear();
        self.samples_since_last_process = 0;
        self.committed_text.clear();
        self.current_text.clear();
        self.silence_count = 0;
        // Reset calibration so it recalibrates on next start
        self.calibrated = false;
        self.calibration_samples.clear();
        self.quiet_streak_samples = 0;
        self.vad_threshold = 0.02;
    }

    /// Get the current buffer duration in seconds
    pub fn buffer_duration(&self) -> f64 {
        self.buffer.len() as f64 / Self::SAMPLE_RATE as f64
    }
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
