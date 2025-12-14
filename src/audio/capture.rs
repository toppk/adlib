//! Audio capture using PipeWire
//!
//! Provides microphone capture with real-time volume metering.

use pipewire as pw;
use pw::spa;
use pw::spa::param::format::{MediaSubtype, MediaType};
use pw::spa::param::format_utils;
use pw::spa::pod::Pod;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// Represents an audio input device
#[derive(Clone, Debug)]
pub struct AudioDevice {
    pub id: u32,
    pub name: String,
    pub description: String,
}

/// Current state of audio capture
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureState {
    Idle,
    Capturing,
    Paused,
    Error,
}

/// Audio capture configuration
#[derive(Clone, Debug)]
pub struct CaptureConfig {
    /// Sample rate (default: 16000 for Whisper compatibility)
    pub sample_rate: u32,
    /// Number of channels (default: 1 for mono)
    pub channels: u32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            channels: 1,
        }
    }
}

/// Shared state for audio capture - thread-safe
#[derive(Clone)]
pub struct SharedCaptureState {
    inner: Arc<Mutex<CaptureStateInner>>,
}

struct CaptureStateInner {
    /// Current RMS volume level (0.0 - 1.0)
    pub volume_level: f32,
    /// Peak volume level for visualization
    pub peak_level: f32,
    /// Recent amplitude samples for waveform display (last ~50 values)
    pub waveform_samples: Vec<f32>,
    /// Captured audio samples (f32, mono, 16kHz)
    pub samples: Vec<f32>,
    /// Total duration in seconds
    pub duration: f64,
    /// Current state
    pub state: CaptureState,
    /// Error message if any
    pub error: Option<String>,
    /// Sample rate being used
    pub sample_rate: u32,
    /// Counter for waveform decimation (to slow down display)
    waveform_counter: u32,
    /// Accumulated RMS for averaging over multiple callbacks
    waveform_rms_sum: f32,
    /// Time of last waveform sample addition (for smooth scrolling)
    last_waveform_time: Option<Instant>,
    /// Interval between waveform samples in seconds (for smooth scrolling)
    waveform_interval_secs: f32,
}

impl SharedCaptureState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(CaptureStateInner {
                volume_level: 0.0,
                peak_level: 0.0,
                waveform_samples: Vec::with_capacity(96),
                samples: Vec::new(),
                duration: 0.0,
                state: CaptureState::Idle,
                error: None,
                sample_rate: 16000,
                waveform_counter: 0,
                waveform_rms_sum: 0.0,
                last_waveform_time: None,
                waveform_interval_secs: 0.08, // ~80ms default
            })),
        }
    }

    pub fn volume_level(&self) -> f32 {
        self.inner.lock().unwrap().volume_level
    }

    pub fn peak_level(&self) -> f32 {
        self.inner.lock().unwrap().peak_level
    }

    pub fn waveform_samples(&self) -> Vec<f32> {
        self.inner.lock().unwrap().waveform_samples.clone()
    }

    pub fn state(&self) -> CaptureState {
        self.inner.lock().unwrap().state
    }

    pub fn duration(&self) -> f64 {
        self.inner.lock().unwrap().duration
    }

    pub fn samples(&self) -> Vec<f32> {
        self.inner.lock().unwrap().samples.clone()
    }

    pub fn sample_rate(&self) -> u32 {
        self.inner.lock().unwrap().sample_rate
    }

    pub fn error(&self) -> Option<String> {
        self.inner.lock().unwrap().error.clone()
    }

    pub fn set_state(&self, state: CaptureState) {
        self.inner.lock().unwrap().state = state;
    }

    pub fn set_error(&self, error: String) {
        let mut inner = self.inner.lock().unwrap();
        inner.error = Some(error);
        inner.state = CaptureState::Error;
    }

    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.samples.clear();
        inner.waveform_samples.clear();
        inner.duration = 0.0;
        inner.volume_level = 0.0;
        inner.peak_level = 0.0;
        inner.error = None;
        inner.state = CaptureState::Idle;
        inner.waveform_counter = 0;
        inner.waveform_rms_sum = 0.0;
        inner.last_waveform_time = None;
    }

    /// Get scroll phase for smooth waveform animation (0.0 to 1.0)
    /// Returns how far we've progressed toward the next sample shift
    pub fn waveform_scroll_phase(&self) -> f32 {
        let inner = self.inner.lock().unwrap();
        if let Some(last_time) = inner.last_waveform_time {
            let elapsed = last_time.elapsed().as_secs_f32();
            (elapsed / inner.waveform_interval_secs).min(1.0)
        } else {
            0.0
        }
    }

    /// Process incoming audio samples
    pub fn process_samples(&self, samples: &[f32], sample_rate: u32) {
        let mut inner = self.inner.lock().unwrap();
        inner.sample_rate = sample_rate;

        if samples.is_empty() {
            return;
        }

        // Calculate RMS volume
        let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
        let rms = (sum_squares / samples.len() as f32).sqrt();

        // Smooth volume level for display
        inner.volume_level = inner.volume_level * 0.7 + rms * 0.3;

        // Track peak with slow decay
        let max = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        inner.peak_level = (inner.peak_level * 0.95).max(max);

        // Add to waveform display with decimation for slower scrolling
        // Accumulate RMS over multiple callbacks, then average
        // Decimation factor of 4 gives ~3-4 seconds of visible history
        const WAVEFORM_DECIMATION: u32 = 4;
        inner.waveform_rms_sum += rms;
        inner.waveform_counter += 1;

        if inner.waveform_counter >= WAVEFORM_DECIMATION {
            // Calculate actual interval for smooth scrolling
            if let Some(last_time) = inner.last_waveform_time {
                inner.waveform_interval_secs = last_time.elapsed().as_secs_f32();
            }
            inner.last_waveform_time = Some(Instant::now());

            // Push averaged RMS value
            let avg_rms = inner.waveform_rms_sum / WAVEFORM_DECIMATION as f32;
            inner.waveform_samples.push(avg_rms);
            if inner.waveform_samples.len() > 96 {
                inner.waveform_samples.remove(0);
            }
            // Reset accumulator
            inner.waveform_counter = 0;
            inner.waveform_rms_sum = 0.0;
        }

        // Append samples for recording
        inner.samples.extend_from_slice(samples);
        inner.duration = inner.samples.len() as f64 / sample_rate as f64;
    }
}

impl Default for SharedCaptureState {
    fn default() -> Self {
        Self::new()
    }
}

/// Audio capture manager using PipeWire
pub struct AudioCapture {
    state: SharedCaptureState,
    is_running: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
    sender: Option<pw::channel::Sender<PipeWireCommand>>,
}

enum PipeWireCommand {
    Stop,
}

impl AudioCapture {
    /// Create a new audio capture instance
    pub fn new() -> Self {
        Self {
            state: SharedCaptureState::new(),
            is_running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
            sender: None,
        }
    }

    /// Get shared capture state for UI updates
    pub fn shared_state(&self) -> SharedCaptureState {
        self.state.clone()
    }

    /// Check if capture is running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Start capturing audio
    pub fn start(&mut self) -> Result<(), String> {
        if self.is_running.load(Ordering::SeqCst) {
            return Err("Capture already running".to_string());
        }

        self.state.reset();
        self.state.set_state(CaptureState::Capturing);
        self.is_running.store(true, Ordering::SeqCst);

        let state = self.state.clone();
        let is_running = self.is_running.clone();

        // Create channel for stopping the loop
        let (sender, receiver) = pw::channel::channel::<PipeWireCommand>();
        self.sender = Some(sender);

        let handle = thread::spawn(move || {
            if let Err(e) = run_capture_loop(state.clone(), is_running.clone(), receiver) {
                state.set_error(e);
            }
            is_running.store(false, Ordering::SeqCst);
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    /// Stop capturing audio and return the samples
    pub fn stop(&mut self) -> Result<Vec<f32>, String> {
        if !self.is_running.load(Ordering::SeqCst) {
            return Err("Capture not running".to_string());
        }

        // Send stop command
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(PipeWireCommand::Stop);
        }

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }

        self.is_running.store(false, Ordering::SeqCst);
        self.state.set_state(CaptureState::Idle);

        Ok(self.state.samples())
    }
}

impl Default for AudioCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        if self.is_running.load(Ordering::SeqCst) {
            let _ = self.stop();
        }
    }
}

/// Run the PipeWire capture loop in a background thread
fn run_capture_loop(
    state: SharedCaptureState,
    _is_running: Arc<AtomicBool>,
    receiver: pw::channel::Receiver<PipeWireCommand>,
) -> Result<(), String> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .map_err(|e| format!("Failed to create PipeWire main loop: {}", e))?;

    let context = pw::context::ContextRc::new(&mainloop, None)
        .map_err(|e| format!("Failed to create PipeWire context: {}", e))?;

    let core = context
        .connect_rc(None)
        .map_err(|e| format!("Failed to connect to PipeWire: {}", e))?;

    // Set up channel receiver to stop the loop
    let mainloop_weak = mainloop.downgrade();
    let _receiver = receiver.attach(mainloop.loop_(), move |cmd| {
        match cmd {
            PipeWireCommand::Stop => {
                if let Some(mainloop) = mainloop_weak.upgrade() {
                    mainloop.quit();
                }
            }
        }
    });

    // User data for the stream callbacks
    struct UserData {
        format: spa::param::audio::AudioInfoRaw,
        state: SharedCaptureState,
    }

    let user_data = UserData {
        format: Default::default(),
        state: state.clone(),
    };

    // Create capture stream
    let props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::APP_NAME => "Adlib Voice Recorder",
    };

    let stream = pw::stream::StreamBox::new(&core, "adlib-capture", props)
        .map_err(|e| format!("Failed to create PipeWire stream: {}", e))?;

    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }

            let (media_type, media_subtype) = match format_utils::parse_format(param) {
                Ok(v) => v,
                Err(_) => return,
            };

            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }

            user_data
                .format
                .parse(param)
                .expect("Failed to parse audio format");
        })
        .process(|stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };

            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }

            let data = &mut datas[0];
            let n_channels = user_data.format.channels().max(1);
            let sample_rate = user_data.format.rate();
            let n_samples = data.chunk().size() / (std::mem::size_of::<f32>() as u32);

            if let Some(raw_samples) = data.data() {
                // Convert bytes to f32 samples and mix to mono if needed
                let mut mono_samples = Vec::with_capacity((n_samples / n_channels) as usize);

                for i in (0..n_samples).step_by(n_channels as usize) {
                    let start = i as usize * std::mem::size_of::<f32>();
                    let end = start + std::mem::size_of::<f32>();
                    if end <= raw_samples.len() {
                        let sample = f32::from_le_bytes(
                            raw_samples[start..end].try_into().unwrap_or([0; 4]),
                        );
                        mono_samples.push(sample);
                    }
                }

                user_data.state.process_samples(&mono_samples, sample_rate);
            }
        })
        .register()
        .map_err(|e| format!("Failed to register stream listener: {}", e))?;

    // Set up audio format - request F32LE at native rate
    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);

    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };

    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|e| format!("Failed to serialize audio format: {:?}", e))?
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).unwrap()];

    // Connect the stream
    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| format!("Failed to connect stream: {}", e))?;

    // Run until stopped
    mainloop.run();

    Ok(())
}

/// Calculate RMS volume from samples
pub fn calculate_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
}

/// Calculate peak volume from samples
pub fn calculate_peak(samples: &[f32]) -> f32 {
    samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max)
}
