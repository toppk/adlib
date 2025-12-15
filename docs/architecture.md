# Adlib Architecture

This document describes the current architecture and implementation of Adlib.

## Overview

Adlib is a single-binary desktop application that provides voice recording and transcription capabilities. It uses a GPU-accelerated UI framework (GPUI) and integrates with the Linux audio stack via PipeWire.

## Module Structure

```
src/
├── main.rs              # Entry point, GPUI app initialization
├── app.rs               # Main application component (2800+ lines)
│                        # Contains all views and business logic
├── models.rs            # Domain models and data structures
├── audio/
│   ├── mod.rs           # Module exports
│   ├── capture.rs       # PipeWire audio capture
│   ├── playback.rs      # PipeWire audio playback
│   └── recorder.rs      # WAV file recording (hound)
├── state/
│   ├── mod.rs           # Module exports
│   ├── app_state.rs     # Application state types
│   └── database.rs      # JSON persistence for recordings
├── transcription/
│   └── mod.rs           # Whisper integration
│                        # - TranscriptionEngine (file-based)
│                        # - LiveTranscriber (real-time streaming)
└── whisper/
    └── manager.rs       # Model download and management
```

## Core Components

### Application State (`app.rs`)

The `Adlib` struct is the root GPUI component containing:

```rust
struct Adlib {
    // View state
    state: AppState,

    // Audio components
    audio_capture: Option<AudioCapture>,
    capture_state: Option<SharedCaptureState>,
    audio_player: Option<AudioPlayer>,
    playback_state: Option<SharedPlaybackState>,

    // Recording state
    is_recording: bool,
    recording_start: Option<Instant>,
    recorded_samples: Vec<f32>,

    // Live transcription
    live_transcriber: Option<Arc<Mutex<LiveTranscriber>>>,
    live_transcript: String,
    live_is_running: bool,
    live_capture_state: Option<SharedCaptureState>,

    // Model management
    model_manager: Option<Arc<ModelManager>>,
    selected_model: WhisperModel,
    model_download_queue: Vec<WhisperModel>,

    // Persistence
    recordings_db: RecordingsDatabase,
}
```

### Views

The application has five main views, all rendered in `app.rs`:

| View | Description |
|------|-------------|
| `Live` | Real-time transcription without saving audio |
| `Record` | Voice recording with waveform visualization |
| `RecordingList` | Browse and manage saved recordings |
| `RecordingDetails` | Playback, view transcript, edit title |
| `Settings` | Model selection, app preferences |

Navigation is via a sidebar with keyboard shortcuts (Ctrl+1-5).

### Audio Pipeline

```
                    ┌─────────────────┐
                    │    PipeWire     │
                    │   (48kHz cap)   │
                    └────────┬────────┘
                             │
                    ┌────────▼────────┐
                    │  AudioCapture   │
                    │ (background th) │
                    └────────┬────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
     ┌────────▼────────┐ ┌───▼───┐ ┌────────▼────────┐
     │ SharedCapture   │ │ Save  │ │   Resample      │
     │     State       │ │ (WAV) │ │  48kHz → 16kHz  │
     │ (waveform/RMS)  │ │       │ │                 │
     └────────┬────────┘ └───────┘ └────────┬────────┘
              │                             │
              ▼                    ┌────────▼────────┐
         UI Waveform               │ LiveTranscriber │
                                   │   (Whisper)     │
                                   └─────────────────┘
```

**Key details:**
- PipeWire captures at native sample rate (typically 48kHz)
- Audio is resampled to 16kHz mono for Whisper
- `SharedCaptureState` provides thread-safe access to waveform data for UI
- Recording saves raw samples; resampling happens at transcription time

### Live Transcription

The `LiveTranscriber` provides real-time speech-to-text:

```rust
struct LiveTranscriber {
    ctx: WhisperContext,           // Whisper model context
    buffer: Vec<f32>,              // Accumulated audio samples
    committed_text: String,        // Finalized transcription
    current_text: String,          // Live/tentative transcription
    vad_threshold: f32,            // Voice activity detection threshold
    calibrated: bool,              // Whether VAD is calibrated
}
```

**Processing flow:**
1. Audio samples are added via `add_samples()` (already resampled to 16kHz)
2. First second calibrates VAD threshold from ambient noise
3. Every 500ms, if speech detected, transcribe entire buffer
4. After 1.5s of silence, commit current text and start fresh
5. Hallucination filtering removes common Whisper artifacts

### Model Management

Whisper models are downloaded from Hugging Face:

```
~/.cache/huggingface/hub/models--ggerganov--whisper.cpp/
└── snapshots/<hash>/
    └── ggml-base.en.bin
```

Supported models: tiny, base, small, medium, large-v1/v2/v3 (with .en variants)

### Data Persistence

All data stored in `~/.local/share/adlib/`:

| File | Purpose |
|------|---------|
| `recordings.json` | Recording metadata (title, duration, transcript) |
| `settings.json` | User preferences |
| `*.wav` | Audio files (16kHz mono) |

## Threading Model

```
┌─────────────────────────────────────────────────────────┐
│                     Main Thread                          │
│  - GPUI event loop                                       │
│  - UI rendering                                          │
│  - State management                                      │
└─────────────────────────────────────────────────────────┘
                           │
         ┌─────────────────┼─────────────────┐
         ▼                 ▼                 ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│ Audio Capture   │ │ Audio Playback  │ │ Whisper Decode  │
│    Thread       │ │    Thread       │ │ (background_    │
│  (PipeWire)     │ │  (PipeWire)     │ │  executor)      │
└─────────────────┘ └─────────────────┘ └─────────────────┘
```

- Audio threads run PipeWire main loops
- Whisper transcription uses GPUI's background executor to avoid blocking UI
- Shared state uses `Arc<Mutex<T>>` or `Arc<AtomicT>` for thread safety

## UI Framework (GPUI)

GPUI uses a builder pattern for UI elements:

```rust
div()
    .id("element-id")           // Required for stateful elements
    .flex()                     // Flexbox layout
    .flex_col()                 // Column direction
    .gap_4()                    // 16px gap
    .p_4()                      // 16px padding
    .bg(rgb(0x1e1e2e))         // Background color
    .rounded_lg()               // Border radius
    .child(child_element)       // Add children
    .on_click(handler)          // Event handling
```

The UI refreshes at 60fps during recording/live transcription via a recurring timer task.

## Error Handling

- Audio errors surface as user-visible error messages
- Transcription errors are logged and shown in UI
- File I/O errors are logged with `eprintln!`
- No panic-based error handling in production paths

## Performance Considerations

- Waveform rendering uses fixed 48 bars (not dynamic sizing)
- Live transcription processes in 500ms chunks
- VAD prevents unnecessary Whisper calls on silence
- Hallucination filtering reduces false positives
- Background threading keeps UI responsive during transcription
