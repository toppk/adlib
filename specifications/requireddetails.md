# Required Details for Adlib Implementation

This document lists open questions and decisions needed to complete the Adlib implementation. Please update this document with answers and the specifications will be updated accordingly.

## Audio Backend

### Question 1: Audio Capture Library
Which audio backend should be used for recording?

Options:
- **ALSA** - Low-level, direct hardware access, widely supported
- **PulseAudio** - Higher-level, session-aware, legacy but common
- **PipeWire** - Modern, low-latency, recommended for new apps
- **CPAL** (cross-platform audio library) - Rust-native, abstracts over backends

Recommendation: CPAL with PipeWire backend would provide the best balance of cross-platform potential and modern Linux support.

### Question 2: Audio Format
The specifications mention 16kHz mono WAV for Whisper compatibility. Should the app:
- Record directly at 16kHz? (Lower quality but smaller files)
- Record at higher quality (44.1kHz/48kHz) and downsample for transcription? (Better quality archive)

### Question 3: Input Device Selection
How should the user select their microphone?
- System default only?
- Dropdown selection from available devices?
- Settings page configuration?

## Whisper Integration

### Question 4: Whisper Implementation
Which Whisper implementation should be used?

Options:
- **whisper.cpp** via FFI - Fast, memory-efficient, C++ library
- **whisper-rs** - Rust bindings to whisper.cpp
- **candle-whisper** - Pure Rust, Hugging Face's ML framework
- **External API** - OpenAI Whisper API or similar

### Question 5: Model Storage
Where should Whisper models be stored?
- `~/.local/share/adlib/models/`
- `~/.cache/adlib/models/`
- User-configurable path?

### Question 6: Model Download
How should models be downloaded?
- On-demand when first selected?
- Background download manager?
- Manual download with user instructions?

## Storage

### Question 7: Recording Storage Format
What metadata format for recordings?
- Individual JSON sidecar files per recording?
- Single recordings.json database?
- SQLite database?

### Question 8: Recording File Location
Where should recordings be stored?
- `~/.local/share/adlib/recordings/`
- `~/Documents/Adlib/` or similar user-facing location?
- Configurable in settings?

## UI/UX

### Question 9: Waveform Visualization
What type of waveform display during recording?
- Simple amplitude bars (simpler to implement)
- Real-time scrolling waveform (more complex)
- Spectrogram view option?

### Question 10: Transcription Editing
How should the transcription editor work?
- Plain text editing only?
- Timestamp-synchronized editing (edit text, keep timestamps)?
- Segment-based editing with individual timestamps?

### Question 11: Export Formats
What export formats should be supported?
- Plain text (.txt)
- SubRip subtitles (.srt)
- WebVTT (.vtt)
- JSON with timestamps?
- Markdown?

### Question 12: Keyboard Shortcuts
Should keyboard shortcuts be configurable?
- Fixed shortcuts as documented?
- User-configurable in settings?
- Both (defaults with override capability)?

## Integration

### Question 13: System Tray
Should the app have a system tray icon for:
- Quick recording access?
- Background transcription status?
- Minimizing to tray?

### Question 14: Desktop Notifications
Should notifications be shown for:
- Recording started/stopped?
- Transcription completed?
- Errors?

### Question 15: File Import
What audio file formats should be supported for import?
- WAV only?
- MP3, FLAC, OGG, etc.?
- Any format supported by a decoder library?

## Performance

### Question 16: Transcription Queue
How should multiple transcriptions be handled?
- Serial (one at a time)?
- Parallel (configurable thread count)?
- Background with queue management?

### Question 17: Memory Management
For large Whisper models (large = 2.9GB):
- Load on startup?
- Load on demand (lazy loading)?
- Unload when not in use?

## Future Features (Priority Order)

Please rank these future features by priority:
- [ ] Cloud sync (which provider?)
- [ ] Multiple language support
- [ ] Speaker diarization (who said what)
- [ ] Voice activity detection (auto-pause on silence)
- [ ] Hotkey for global recording (record from any app)
- [ ] Audio enhancement/noise reduction
- [ ] Live transcription (real-time as you speak)
- [ ] Tags/folders for organization
- [ ] Search across transcriptions
- [ ] Collaborative features

---

## Answered Questions

*Move questions here once answered, with the decision made:*

(No questions answered yet)
