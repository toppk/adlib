# Dependencies and External Components

This app leans on several key libraries and system frameworks. For a desktop/Linux port, each item should be mapped to an equivalent component or custom implementation. Below is a concise inventory with capabilities and the touchpoints in the app.

## WhisperKit (Whisper models)
- **Role**: Loads, downloads, prewarms, and runs Whisper models for offline and live transcription.
- **Capabilities**:
  - Fetch remote model list from Hugging Face (`argmaxinc/whisperkit-coreml`).
  - Download model weights to local cache.
  - Prewarm and load models with configurable compute units.
  - Transcribe WAV files and in-memory audio sample arrays; provides progress, segments, tokens, and timings.
  - Exposes recommended models (default and disabled sets).
- **Key interactions**:
  - `TranscriptionStream.fetchModels()` — discovers local/remote models.
  - `TranscriptionStream.loadModel()` — downloads (if needed), prewarms, loads; tracks progress and state.
  - `TranscriptionStream.transcribeAudioFile()` — offline transcription of saved WAVs.
  - `TranscriptionStream.transcribeAudioSamples()` — live transcription of streaming buffers with VAD gating.
  - `TranscriptionWorker.process(...)` — uses `RecordingTranscriptionStream` to load model and transcribe queued recordings.
  - Settings screens — model selector uses WhisperKit’s recommended/default info.

## AudioKit (FormatConverter)
- **Role**: Normalizes imported audio to the app’s expected format (16kHz mono WAV).
- **Capabilities**:
  - Convert arbitrary audio files to WAV with target sample rate, bit depth, channels.
- **Key interactions**:
  - `FileImportClient.importFile` and share extension `ShareViewModel.importFile` — convert picked/shared files into WAVs in the recordings directory.

## AVFoundation (system audio)
- **Role**: Mic capture, resampling, writing WAVs, playback.
- **Capabilities**:
  - Audio engine with input taps for live capture.
  - Resample to target format (`AVAudioConverter`).
  - Write WAVs (`AVAudioFile`).
  - Playback (`AVAudioPlayer`).
  - Session routing and microphone selection (`AVAudioSession`).
- **Key interactions**:
  - `AudioProcessor.startFileRecording()` — resample buffers to 16kHz mono float, track energy, hand off to recorder/transcriber.
  - `RecordingStream` — writes buffers to WAV, tracks duration and waveform slices.
  - `AudioSessionClient` — selects microphone, configures categories/options, requests permission.
  - `AudioPlayerClient` — plays back recordings with seek/pause/resume/speed.

## The Composable Architecture (TCA)
- **Role**: State management and navigation (reducers, actions, stores).
- **Capabilities**:
  - Unidirectional data flow, effect handling, shared state.
- **Key interactions**:
  - All feature reducers (`Root`, `RecordScreen`, `RecordingListScreen`, `RecordingDetails`, `SettingsScreen`, `TranscriptionWorker`, etc.).
  - Shared persisted state (`Shared` keys for recordings, settings, tasks, premium flags).

## StoreKit (IAP) / RevenueCat (optional)
- **Role**: Gating live transcription behind a purchasable product.
- **Capabilities**:
  - Check/purchase/restore entitlement; observe subscription status (App Store build).
- **Key interactions**:
  - `PremiumFeaturesSection` / `PurchaseLiveTranscriptionModal` — manage purchase status and UI gating.
  - `Settings` and `RecordScreen` — enable/disable live transcription toggle based on purchase flag.
  - Desktop port: replace with custom licensing or disable the gate.

## BackgroundTasks / UIApplication background APIs
- **Role**: Keep queued transcription running while app is backgrounded (iOS).
- **Key interactions**:
  - `TranscriptionWorker` registers/schedules `BGProcessingTask` and uses `UIApplication.beginBackgroundTask` to extend execution.
  - Desktop port: replace with worker threads/services; no BGTask needed.

## iCloud Drive (ubiquity container)
- **Role**: Optional sync of WAV files to user’s iCloud Drive.
- **Key interactions**:
  - `StorageClient.uploadRecordingsToICloud` — copies WAVs to the ubiquity container; tracks uploaded files via `UserDefaults`.
  - Desktop port: swap for preferred sync target or disable.

## Rollbar (optional)
- **Role**: Error reporting when Rollbar is available.
- **Key interactions**:
  - Initialized in `AppView.init()` when `RollbarNotifier` is present.
  - Desktop port: choose your logging/telemetry stack or omit.

## Suggested Linux/Rust Equivalents (and gaps)
- **WhisperKit replacement**: `whisper-rs` (Rust bindings over whisper.cpp) or `whisper-cpp-rs`. Capabilities: local Whisper inference, model download can be scripted (no built-in Hugging Face catalog). Lacks prewarm/stateful model management API; you’ll need to handle download/cache, model selection, and progress reporting yourself.
- **Model download/catalog**: Prefer the Rust `hf-hub` crate (built-in caching, pulls by repo/model id) or `huggingface-hub` CLI; fallback is `reqwest` + manual cache. You must replicate `recommendedModels()` logic and folder normalization.
- **Audio capture/playback**: `pipewire` crate for low-latency capture/playback; alternately `cpal` + `rodio` (simpler playback) with custom resampling (`rubato` or `speexdsp` bindings). Ensure mono 16kHz capture and tap-like callbacks; there’s no built-in AVAudioConverter, so you must resample manually.
- **Audio format conversion**: `ffmpeg-next` or `symphonia` for decoding/encoding; `hound` for WAV writing. Replace AudioKit’s `FormatConverter`.
- **UI toolkit**: `gtk4-rs`, `iced`, `egui`, or `tauri` (WebView) depending on needs. TCA replacement would be a Redux-style store (e.g., `dioxus` signals, `yewdux`, or bespoke state machine).
- **Background tasks**: `tokio` tasks or a background worker thread/service. No BGProcessingTask analog; build your own scheduling and cancellation.
- **Persistence**: JSON via `serde` (matches current schema). Watch filesystem for import directory (replace app-group share folder).
- **IAP/licensing**: No StoreKit; replace with your licensing check or remove the gate. RevenueCat isn’t applicable.
- **Telemetry**: Substitute `tracing` + `sentry` or similar; Rollbar not required.
