//! Audio playback using PipeWire
//!
//! Provides audio playback with real-time position tracking and waveform data.

#![allow(dead_code)]

use pipewire as pw;
use pw::spa;
use pw::spa::param::format::{MediaSubtype, MediaType};
use pw::spa::param::format_utils;
use pw::spa::pod::Pod;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

/// Shared state for audio playback - thread-safe
#[derive(Clone)]
pub struct SharedPlaybackState {
    inner: Arc<Mutex<PlaybackStateInner>>,
}

struct PlaybackStateInner {
    /// Audio samples to play
    samples: Vec<f32>,
    /// Sample rate
    sample_rate: u32,
    /// Current playback position (sample index)
    position: usize,
    /// Total duration in seconds
    duration: f64,
    /// Is playback active
    is_playing: bool,
    /// Pre-computed waveform samples for visualization (RMS values)
    waveform: Vec<f32>,
}

impl SharedPlaybackState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(PlaybackStateInner {
                samples: Vec::new(),
                sample_rate: 48000,
                position: 0,
                duration: 0.0,
                is_playing: false,
                waveform: Vec::new(),
            })),
        }
    }

    /// Load audio samples for playback
    pub fn load(&self, samples: Vec<f32>, sample_rate: u32) {
        let mut inner = self.inner.lock().unwrap();
        inner.duration = samples.len() as f64 / sample_rate as f64;

        // Pre-compute waveform visualization (96 bars like recording view)
        let num_bars = 96;
        let samples_per_bar = samples.len() / num_bars;
        let mut waveform = Vec::with_capacity(num_bars);

        for i in 0..num_bars {
            let start = i * samples_per_bar;
            let end = ((i + 1) * samples_per_bar).min(samples.len());
            if start < end {
                // Calculate RMS for this segment
                let sum_squares: f32 = samples[start..end].iter().map(|s| s * s).sum();
                let rms = (sum_squares / (end - start) as f32).sqrt();
                waveform.push(rms);
            } else {
                waveform.push(0.0);
            }
        }

        inner.waveform = waveform;
        inner.samples = samples;
        inner.sample_rate = sample_rate;
        inner.position = 0;
    }

    /// Get current playback position in seconds
    pub fn current_time(&self) -> f64 {
        let inner = self.inner.lock().unwrap();
        inner.position as f64 / inner.sample_rate as f64
    }

    /// Get total duration in seconds
    pub fn duration(&self) -> f64 {
        self.inner.lock().unwrap().duration
    }

    /// Check if playback is active
    pub fn is_playing(&self) -> bool {
        self.inner.lock().unwrap().is_playing
    }

    /// Get pre-computed waveform samples
    pub fn waveform(&self) -> Vec<f32> {
        self.inner.lock().unwrap().waveform.clone()
    }

    /// Get playback progress as fraction (0.0 - 1.0)
    pub fn progress(&self) -> f32 {
        let inner = self.inner.lock().unwrap();
        if inner.samples.is_empty() {
            0.0
        } else {
            inner.position as f32 / inner.samples.len() as f32
        }
    }

    /// Set playing state
    fn set_playing(&self, playing: bool) {
        self.inner.lock().unwrap().is_playing = playing;
    }

    /// Reset playback position to start
    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.position = 0;
        inner.is_playing = false;
    }

    /// Seek to a position (fraction 0.0 - 1.0)
    pub fn seek(&self, fraction: f32) {
        let mut inner = self.inner.lock().unwrap();
        let target = (fraction * inner.samples.len() as f32) as usize;
        inner.position = target.min(inner.samples.len());
    }

    /// Get samples for playback (advances position)
    fn get_samples(&self, count: usize) -> Option<Vec<f32>> {
        let mut inner = self.inner.lock().unwrap();
        if inner.position >= inner.samples.len() {
            inner.is_playing = false;
            return None;
        }

        let end = (inner.position + count).min(inner.samples.len());
        let samples = inner.samples[inner.position..end].to_vec();
        inner.position = end;

        if inner.position >= inner.samples.len() {
            inner.is_playing = false;
        }

        Some(samples)
    }
}

impl Default for SharedPlaybackState {
    fn default() -> Self {
        Self::new()
    }
}

/// Audio player using PipeWire
pub struct AudioPlayer {
    state: SharedPlaybackState,
    is_running: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
    sender: Option<pw::channel::Sender<PlaybackCommand>>,
}

enum PlaybackCommand {
    Stop,
}

impl AudioPlayer {
    /// Create a new audio player
    pub fn new() -> Self {
        Self {
            state: SharedPlaybackState::new(),
            is_running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
            sender: None,
        }
    }

    /// Get shared playback state for UI updates
    pub fn shared_state(&self) -> SharedPlaybackState {
        self.state.clone()
    }

    /// Check if playback is running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Load audio for playback
    pub fn load(&self, samples: Vec<f32>, sample_rate: u32) {
        self.state.load(samples, sample_rate);
    }

    /// Start playback
    pub fn play(&mut self) -> Result<(), String> {
        if self.is_running.load(Ordering::SeqCst) {
            return Err("Playback already running".to_string());
        }

        // Reset position to start if we've finished
        if self.state.progress() >= 0.99 {
            self.state.reset();
        }

        self.state.set_playing(true);
        self.is_running.store(true, Ordering::SeqCst);

        let state = self.state.clone();
        let is_running = self.is_running.clone();

        // Create channel for stopping the loop
        let (sender, receiver) = pw::channel::channel::<PlaybackCommand>();
        self.sender = Some(sender);

        let handle = thread::spawn(move || {
            if let Err(e) = run_playback_loop(state.clone(), is_running.clone(), receiver) {
                eprintln!("Playback error: {}", e);
            }
            state.set_playing(false);
            is_running.store(false, Ordering::SeqCst);
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    /// Stop playback
    pub fn stop(&mut self) {
        if !self.is_running.load(Ordering::SeqCst) {
            return;
        }

        // Send stop command
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(PlaybackCommand::Stop);
        }

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }

        self.is_running.store(false, Ordering::SeqCst);
        self.state.set_playing(false);
    }

    /// Toggle play/pause
    pub fn toggle(&mut self) -> Result<(), String> {
        if self.is_running.load(Ordering::SeqCst) {
            self.stop();
            Ok(())
        } else {
            self.play()
        }
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Run the PipeWire playback loop in a background thread
fn run_playback_loop(
    state: SharedPlaybackState,
    _is_running: Arc<AtomicBool>,
    receiver: pw::channel::Receiver<PlaybackCommand>,
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
    let _receiver = receiver.attach(mainloop.loop_(), move |cmd| match cmd {
        PlaybackCommand::Stop => {
            if let Some(mainloop) = mainloop_weak.upgrade() {
                mainloop.quit();
            }
        }
    });

    // User data for the stream callbacks
    struct UserData {
        format: spa::param::audio::AudioInfoRaw,
        state: SharedPlaybackState,
        mainloop_weak: pw::main_loop::MainLoopWeak,
    }

    let user_data = UserData {
        format: Default::default(),
        state: state.clone(),
        mainloop_weak: mainloop.downgrade(),
    };

    // Create playback stream
    let props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::APP_NAME => "Adlib Voice Recorder",
    };

    let stream = pw::stream::StreamBox::new(&core, "adlib-playback", props)
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
            let n_channels = user_data.format.channels().max(1) as usize;
            let stride = std::mem::size_of::<f32>() * n_channels;

            let Some(slice) = data.data() else {
                return;
            };

            let n_frames = slice.len() / stride;

            // Get samples from our buffer
            let samples = user_data.state.get_samples(n_frames);

            match samples {
                Some(samples) => {
                    // Write samples to output buffer
                    for (i, &sample) in samples.iter().enumerate() {
                        let offset = i * stride;
                        if offset + std::mem::size_of::<f32>() <= slice.len() {
                            let bytes = sample.to_le_bytes();
                            slice[offset..offset + 4].copy_from_slice(&bytes);
                            // If stereo, duplicate to second channel
                            if n_channels > 1 && offset + 8 <= slice.len() {
                                slice[offset + 4..offset + 8].copy_from_slice(&bytes);
                            }
                        }
                    }
                    // Fill remainder with silence
                    let written = samples.len() * stride;
                    if written < slice.len() {
                        slice[written..].fill(0);
                    }

                    let chunk = data.chunk_mut();
                    *chunk.offset_mut() = 0;
                    *chunk.stride_mut() = stride as i32;
                    *chunk.size_mut() = (samples.len() * stride) as u32;
                }
                None => {
                    // No more samples - stop playback
                    if let Some(mainloop) = user_data.mainloop_weak.upgrade() {
                        mainloop.quit();
                    }
                }
            }
        })
        .register()
        .map_err(|e| format!("Failed to register stream listener: {}", e))?;

    // Set up audio format - request F32LE stereo at native rate
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

    // Connect the stream (Output direction for playback)
    stream
        .connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| format!("Failed to connect stream: {}", e))?;

    // Run until stopped or playback ends
    mainloop.run();

    Ok(())
}
