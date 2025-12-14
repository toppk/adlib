# WhisperBoard Architectural Overview

This document reverse-engineers the current iOS Swift/SwiftUI implementation into a platform-agnostic description aimed at reimplementing the product on a Linux desktop in another language. The app is a self-contained voice recorder and transcription tool that wraps OpenAI Whisper via the WhisperKit library, maintains a queue of transcription jobs, and exposes the functionality through a small set of screens.

## High-Level Architecture
- **Modules**
  - `AppKit` – SwiftUI/TCA presentation layer (screens, view models, design system).
  - `AudioProcessing` – recording, live transcription stream, WhisperKit model orchestration.
  - `Common` – shared entities, helpers, persistence keys, logging.
  - `App` – platform entry point and share extension glue.
  - `Support/Resources` – assets, fonts, app icons, and StoreKit stubs.
- **State Management**
  - The code uses The Composable Architecture (TCA): each feature has `State`, `Action`, and `Reducer`. Replace with an equivalent unidirectional data flow (e.g., Redux-style store) on desktop.
- **Primary Data Stores**
  - `recordings.json` (Documents): array of `RecordingInfo`.
  - `settings.json`: persisted `Settings`.
  - `premiumFeatures.json`: purchase flags for live transcription.
  - In-memory `transcriptionTasks`: queue of pending transcriptions.
- **Navigation Shell**
  - `Root` reducer hosts three tabs/flows: Record, Recording List, Settings. Navigation stack also opens a Recording Details screen.

## Core Domain Models (Python-style pseudocode)

```python
class TranscriptionParameters:
    initial_prompt: Optional[str] = None
    language: Optional[str] = None
    offset_ms: int = 0
    should_translate: bool = False

class TranscriptionTimings:
    tokens_per_second: float = 0.0
    full_pipeline_seconds: float = 0.0

class Token:
    id: int
    index: int
    log_probability: float
    speaker: Optional[str] = None

class WordData:
    word: str
    start_ms: int
    end_ms: int
    probability: float

class Segment:
    start_ms: int
    end_ms: int
    text: str
    tokens: list[Token]
    speaker: Optional[str] = None
    words: list[WordData] = []

class Transcription:
    id: UUID
    file_name: str
    start_date: datetime
    parameters: TranscriptionParameters
    model_name: str
    status: Literal["not_started","loading","uploading","progress","done","canceled","error","paused"]
    text: str
    segments: list[Segment] = []
    timings: TranscriptionTimings
    progress: float  # derived from status

class RecordingInfo:
    file_name: str            # wav on disk
    title: str                # default to date string
    date: datetime
    duration_seconds: float
    edited_text: Optional[str]
    transcription: Optional[Transcription]
    @property
    def id(self) -> str: return file_name
    @property
    def text(self) -> str: return edited_text or (transcription.text if transcription else "")

class Settings:
    selected_model_name: str = "tiny"
    parameters: TranscriptionParameters
    is_icloud_sync_enabled: bool = False
    should_mix_with_other_audio: bool = False
    is_using_gpu: bool = False
    is_using_neural_engine: bool = True
    is_vad_enabled: bool = False
    is_live_transcription_enabled: bool = False

class TranscriptionTask:
    id: UUID
    recording_info_id: str
    settings: Settings

class ModelInfo:
    name: str
    is_local: bool
    is_default: bool
    is_disabled: bool
```

## Functional Pillars
- **Audio capture and playback**
  - Records microphone input to mono 16kHz WAV while tracking waveform energy for UI.
  - Supports pause/resume, cancel, and completion actions.
  - Playback uses AVAudioPlayer with play/pause/seek/speed controls.
- **Transcription**
  - Two modes: queued offline transcription of saved files, and optional live transcription during recording (premium feature).
  - Whisper models are downloaded and loaded via WhisperKit; decode options are configured per Settings.
  - Progress updates drive UI states (loading, progress %, pause/resume, errors).
- **Model lifecycle**
  - Lists local and remote models, downloads to `Documents/huggingface/models/...`, prewarms, loads, and can delete individually or en masse.
- **File import/share**
  - Import arbitrary audio files; converts to app-native WAV (16kHz mono).
  - Share extension writes incoming audio to an app group folder that is ingested on next launch.
- **Persistence and migration**
  - JSON-backed persistence for recordings/settings/premium status.
  - Migration helpers upgrade legacy payloads to current schema.
- **Background/queue processing**
  - Transcription worker processes a FIFO queue; keeps work alive in background tasks and supports cancel/resume per recording.
- **Sync**
  - Optional iCloud Drive copy of recorded WAV files.

## Typical Runtime Flow
1. On launch, migrate data, load persisted recordings/settings, and clean up any in-progress transcriptions by marking them failed.
2. Root screen presents Record, List, and Settings flows. Idle timer disables while recording/transcribing.
3. Recording flow:
   - Request mic permission; start recording to a unique WAV file.
   - Optionally load the selected model and start live transcription loop, updating segments as buffers arrive.
   - On save, stop recording and persist `RecordingInfo`; enqueue transcription task if needed.
4. Recording list:
   - Displays persisted recordings as cards with playback controls and transcription status.
   - Allows deletion and file import; taps open Recording Details.
5. Recording details:
   - Shows text view or time-aligned timeline of segments plus waveform/progress and playback controls.
   - Action sheet offers restart transcription, delete, or share audio.
6. Settings:
   - Adjust model selection, language/prompt/translation flags, VAD/compute options, live transcription toggle, storage cleanup, iCloud sync, and app links.
7. Background worker:
   - Processes queued transcriptions, loads models, runs WhisperKit on WAV files, updates recordings, and handles cancel/resume.
8. Share extension:
   - Converts incoming audio to WAV in shared container; main app moves these into Documents on next sync.

Reimplementing on Linux can mirror these concepts with equivalent libraries: a unidirectional state store, local file-based persistence, an audio engine capable of 16kHz mono capture, and a Whisper-compatible inference backend.
