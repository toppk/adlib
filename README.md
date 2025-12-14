# Adlib

A Linux desktop voice recorder and transcription application built with Rust.

Adlib captures audio via PipeWire, transcribes speech using Whisper (via whisper-rs), and provides both recording-based and live transcription modes.

## Features

- **Voice Recording**: Record audio with real-time waveform visualization
- **Live Transcription**: Real-time speech-to-text with instant feedback
- **Offline Transcription**: Transcribe saved recordings using local Whisper models
- **Model Management**: Download and manage Whisper models from Hugging Face
- **Recording Library**: Browse, playback, and manage your recordings

## Requirements

- Linux with PipeWire audio
- Rust toolchain (1.70+)
- System dependencies (see [Development Guide](docs/DEVELOPMENT.md))

## Quick Start

```bash
# Install system dependencies (Fedora)
sudo dnf install freetype-devel libxcb-devel libxkbcommon-devel \
    libxkbcommon-x11-devel pipewire-devel

# Build and run
cargo run --release
```

## Documentation

- [Development Guide](docs/DEVELOPMENT.md) - Building, project structure, contributing
- [Architecture](docs/ARCHITECTURE.md) - Technical overview and design decisions
- [Future Roadmap](docs/FUTURE.md) - Planned features and ideas

## Tech Stack

- **GUI**: [GPUI](https://gpui.rs) (Zed's GPU-accelerated UI framework)
- **Audio**: PipeWire for capture/playback, hound for WAV I/O
- **Transcription**: [whisper-rs](https://github.com/tazz4843/whisper-rs) (whisper.cpp bindings)
- **Models**: Hugging Face Hub for model downloads

## License

Dual-licensed under MIT or Apache-2.0.
