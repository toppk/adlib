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
/// Uses a rolling buffer approach based on whisper.cpp stream example:
/// - Step size: 500ms (process every half second for responsiveness)
/// - Window size: 5 seconds (analyze 5 seconds at a time)
/// - Keep: 200ms (overlap between chunks)
/// - Real-time correction: current window text updates as more audio arrives
pub struct LiveTranscriber {
    ctx: WhisperContext,
    buffer: Vec<f32>,
    samples_since_last_process: usize,
    /// Confirmed/committed text (won't change)
    confirmed_text: String,
    /// Tentative text from current window (may be corrected)
    tentative_text: String,
    /// Number of times we've seen similar tentative text (for committing)
    stable_count: usize,
    /// Number of consecutive silent processing cycles
    silence_count: usize,
    /// Previous tentative text for comparison
    prev_tentative: String,
    /// Calibrated VAD threshold based on ambient noise
    vad_threshold: f32,
    /// Whether calibration is complete
    calibrated: bool,
    /// Samples collected during calibration
    calibration_samples: Vec<f32>,
}

impl LiveTranscriber {
    /// Sample rate expected by Whisper
    pub const SAMPLE_RATE: u32 = 16000;
    /// Process every 500ms (like whisper-stream --step 500)
    const STEP_SAMPLES: usize = 500 * 16; // 8000 samples = 0.5 seconds
    /// Analyze 5 seconds at a time (like whisper-stream --length 5000)
    const WINDOW_SAMPLES: usize = 5 * 16000; // 80000 samples = 5 seconds
    /// Keep 200ms overlap (like whisper-stream default)
    const KEEP_SAMPLES: usize = 200 * 16; // 3200 samples = 0.2 seconds
    /// Calibration duration in samples (1 second - faster startup)
    const CALIBRATION_SAMPLES: usize = 1 * 16000;
    /// Minimum VAD threshold (even in quiet rooms) - high to avoid hallucinations
    const MIN_VAD_THRESHOLD: f32 = 0.03;
    /// Multiplier above ambient noise for VAD threshold
    const VAD_MULTIPLIER: f32 = 4.0;
    /// Number of stable iterations before committing text (same text repeated)
    const STABLE_THRESHOLD: usize = 4; // ~2 seconds at 500ms step
    /// Number of silent iterations before committing text (faster than stable)
    const SILENCE_COMMIT_THRESHOLD: usize = 2; // ~1 second of silence

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
            buffer: Vec::with_capacity(Self::WINDOW_SAMPLES),
            samples_since_last_process: 0,
            confirmed_text: String::new(),
            tentative_text: String::new(),
            stable_count: 0,
            silence_count: 0,
            prev_tentative: String::new(),
            vad_threshold: 0.02, // Default, will be calibrated
            calibrated: false,
            calibration_samples: Vec::with_capacity(Self::CALIBRATION_SAMPLES),
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
        // During calibration, collect samples to measure ambient noise
        if !self.calibrated {
            let remaining = Self::CALIBRATION_SAMPLES - self.calibration_samples.len();
            if remaining > 0 {
                let to_add = samples.len().min(remaining);
                self.calibration_samples.extend_from_slice(&samples[..to_add]);

                // Check if calibration is complete
                if self.calibration_samples.len() >= Self::CALIBRATION_SAMPLES {
                    self.complete_calibration();
                }
            }
            return; // Don't add to main buffer during calibration
        }

        self.buffer.extend_from_slice(samples);
        self.samples_since_last_process += samples.len();

        // Keep buffer from growing too large - trim to window size + some extra
        let max_buffer = Self::WINDOW_SAMPLES + Self::STEP_SAMPLES;
        if self.buffer.len() > max_buffer {
            let trim_amount = self.buffer.len() - Self::WINDOW_SAMPLES;
            self.buffer.drain(0..trim_amount);
        }
    }

    /// Complete calibration by calculating VAD threshold from ambient noise
    fn complete_calibration(&mut self) {
        let ambient_rms = Self::calculate_rms(&self.calibration_samples);
        // Set threshold to be VAD_MULTIPLIER times the ambient noise, with a minimum
        self.vad_threshold = (ambient_rms * Self::VAD_MULTIPLIER).max(Self::MIN_VAD_THRESHOLD);
        self.calibrated = true;
        self.calibration_samples.clear(); // Free memory
        eprintln!("VAD calibrated: ambient RMS = {:.4}, threshold = {:.4}", ambient_rms, self.vad_threshold);
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
            "[music", "(music", "♪", "🎵", "[blank_audio]", "[silence",
            "(silence", "[audio", "(audio",
            // Non-speech sounds
            "[sigh", "(sigh", "[crying", "(crying", "[laughter", "(laughter",
            "[applause", "(applause", "[noise", "(noise",
            "[inaudible", "(inaudible", "[unintelligible", "(unintelligible",
            "[background", "(background", "[ambient", "(ambient",
            "[static", "(static", "[breathing", "(breathing",
            "[cough", "(cough", "[sneeze", "(sneeze",
            "[whisper", "(whisper", "[mumbl", "(mumbl",
            "[squeak", "(squeak", "[click", "(click",
            "[beep", "(beep", "[tone", "(tone",
            "[bell", "(bell", "[ring", "(ring",
            // Emotional/dramatic markers
            "[dramatic", "(dramatic", "[sad", "(sad", "[happy", "(happy",
            "[whistl", "(whistl", "[humm", "(humm",
            "[mimick", "(mimick",
            // Foreign language markers
            "[speaking", "(speaking", "[foreign", "(foreign",
            // Tech sounds
            "[xbox", "(xbox", "[windows", "(windows",
            // Whisper garbage patterns
            "...", "shh", "shhh", "hmm", "hush", "fash",
            "shook", "whoosh", "air whoosh",
            // Common false positives on silence - short phrases
            "you are the only", "your house", "i'll show you",
            "yet the few", "a few days", "and you have",
            "thank you", "thanks for", "bye", "goodbye",
            "i'm sorry", "sorry", "please come", "come forward",
            "famous for", "you will be",
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
            let filler_words = ["and", "the", "a", "an", "to", "of", "in", "is", "it", "you", "i"];
            let filler_count = words.iter().filter(|w| filler_words.contains(w)).count();
            if filler_count == words.len() {
                return true;
            }
        }

        false
    }

    /// Process the current buffer and return transcription update
    /// Returns (has_update, tentative_text) - tentative text may change as more audio arrives
    pub fn process(&mut self) -> Result<bool, String> {
        if self.buffer.len() < Self::STEP_SAMPLES {
            return Ok(false);
        }

        // Reset counter
        self.samples_since_last_process = 0;

        // Use up to WINDOW_SAMPLES from buffer (most recent audio)
        let samples_to_process = if self.buffer.len() > Self::WINDOW_SAMPLES {
            &self.buffer[self.buffer.len() - Self::WINDOW_SAMPLES..]
        } else {
            &self.buffer[..]
        };

        // VAD: Check if there's enough audio energy to be speech
        let rms = Self::calculate_rms(samples_to_process);
        if rms < self.vad_threshold {
            // Too quiet - likely silence
            // If we had tentative text, commit it faster during silence
            if !self.tentative_text.is_empty() {
                self.silence_count += 1;
                if self.silence_count >= Self::SILENCE_COMMIT_THRESHOLD {
                    self.commit_tentative();
                }
            }
            return Ok(false);
        }

        // Speech detected - reset silence counter
        self.silence_count = 0;

        // Set up params for streaming (like whisper-stream)
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_single_segment(true);
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_no_context(true); // Don't use context from previous - reduces hallucination
        // Suppress output on silence/noise
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);

        // Create state and run transcription
        let mut state = self.ctx.create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        state.full(params, samples_to_process)
            .map_err(|e| format!("Transcription failed: {}", e))?;

        // Extract text from segments
        let num_segments = state.full_n_segments();
        let mut window_text = String::new();

        for i in 0..num_segments {
            if let Some(segment) = state.get_segment(i) {
                let text = segment.to_str_lossy()
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                // Skip hallucinations
                if Self::is_hallucination(&text) {
                    continue;
                }

                if !text.trim().is_empty() {
                    if !window_text.is_empty() && !text.starts_with(' ') {
                        window_text.push(' ');
                    }
                    window_text.push_str(&text);
                }
            }
        }

        let window_text = window_text.trim().to_string();

        // Final check - skip if it's all hallucination
        // Also require minimum word count - 5 seconds of real speech should have 4+ words
        let word_count = window_text.split_whitespace().count();
        if window_text.is_empty() || Self::is_hallucination(&window_text) || word_count < 4 {
            // Silence detected in transcription - increment stability
            if !self.tentative_text.is_empty() {
                self.stable_count += 1;
                if self.stable_count >= Self::STABLE_THRESHOLD {
                    self.commit_tentative();
                }
            }
            return Ok(false);
        }

        // Check if text is stable (same as previous)
        if window_text == self.prev_tentative {
            self.stable_count += 1;
            if self.stable_count >= Self::STABLE_THRESHOLD {
                self.commit_tentative();
            }
        } else {
            // Text changed - reset stability counter
            self.stable_count = 0;
            self.tentative_text = window_text.clone();
            self.prev_tentative = window_text;
        }

        Ok(true)
    }

    /// Commit tentative text to confirmed and clear window
    fn commit_tentative(&mut self) {
        if !self.tentative_text.is_empty() {
            if !self.confirmed_text.is_empty() {
                self.confirmed_text.push(' ');
            }
            self.confirmed_text.push_str(&self.tentative_text);
            self.tentative_text.clear();
            self.prev_tentative.clear();
            self.stable_count = 0;
            self.silence_count = 0;
            // Clear most of the buffer, keeping overlap
            if self.buffer.len() > Self::KEEP_SAMPLES {
                let keep_start = self.buffer.len() - Self::KEEP_SAMPLES;
                self.buffer.drain(0..keep_start);
            }
        }
    }

    /// Get the full transcript (confirmed + tentative)
    pub fn get_transcript(&self) -> String {
        if self.confirmed_text.is_empty() {
            self.tentative_text.clone()
        } else if self.tentative_text.is_empty() {
            self.confirmed_text.clone()
        } else {
            format!("{} {}", self.confirmed_text, self.tentative_text)
        }
    }

    /// Get just the confirmed (committed) text
    pub fn get_confirmed(&self) -> &str {
        &self.confirmed_text
    }

    /// Get just the tentative (may change) text
    pub fn get_tentative(&self) -> &str {
        &self.tentative_text
    }

    /// Clear the buffer and all text
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.samples_since_last_process = 0;
        self.confirmed_text.clear();
        self.tentative_text.clear();
        self.prev_tentative.clear();
        self.stable_count = 0;
        self.silence_count = 0;
        // Reset calibration so it recalibrates on next start
        self.calibrated = false;
        self.calibration_samples.clear();
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
