# Adlib Development Guide

Adlib is a Linux desktop voice recorder and transcription application built with Rust and GPUI.

## Project Overview

- **Purpose**: Self-contained voice recorder and transcription tool using Whisper
- **Target Platform**: Linux desktop
- **License**: MIT/Apache-2.0 dual license
- **GUI Framework**: GPUI (Zed's GUI toolkit)

## Development Rules

### Golden Rules

- **NEVER commit without explicit user approval** - Always wait for the user to say they're ready to commit. Run tests and let the user do their own QA first.

### Cargo and Dependencies

- **NEVER** manually edit `Cargo.toml` for adding dependencies
- Use `cargo add <package>` to add new dependencies
- Use `cargo add <package> --features <feature>` for feature flags
- Use `cargo remove <package>` to remove dependencies

### Code Style

- Follow Rust 2021 edition idioms
- Use `rustfmt` for formatting (run `cargo fmt`)
- Use `clippy` for linting (run `cargo clippy`)
- Prefer descriptive variable and function names
- Document public APIs with rustdoc comments

### Architecture

- **Unidirectional data flow**: Use a Redux-style state management pattern
- **Separation of concerns**: Keep UI, state, and business logic separate
- **Module structure**:
  - `src/app.rs` - Application entry point and root state
  - `src/views/` - UI views (record, list, details, settings)
  - `src/state/` - State management and reducers
  - `src/audio/` - Audio capture and playback
  - `src/transcription/` - Whisper integration
  - `src/storage/` - Persistence and file management

### File Organization

```
adlib/
├── CLAUDE.md           # This file
├── Cargo.toml          # Managed by cargo
├── LICENSE-MIT         # MIT license
├── LICENSE-APACHE      # Apache 2.0 license
├── docs/               # Documentation
│   ├── architecture.md       # Technical overview
│   ├── development.md        # Development guide
│   ├── future.md             # Roadmap and ideas
│   ├── live_transcription.md # Live transcription docs
│   └── ui-waveform.md        # Waveform UI implementation
├── specifications/     # Project specifications
├── src/
│   ├── main.rs         # Entry point
│   ├── app.rs          # Application setup
│   ├── views/          # UI components
│   ├── state/          # State management
│   ├── audio/          # Audio handling
│   ├── transcription/  # Whisper integration
│   └── storage/        # File persistence
└── assets/             # Icons, fonts, etc.
```

### Data Storage

- **Recordings**: `~/.local/share/adlib/recordings.json`
- **Settings**: `~/.local/share/adlib/settings.json`
- **Audio Files**: `~/.local/share/adlib/*.wav`
- **Models**: `~/.local/share/adlib/models/`

### Audio Requirements

- Record to **mono 16kHz WAV** format (Whisper requirement)
- Support pause/resume during recording
- Track waveform energy for UI visualization

### GPUI Patterns

- Use `gpui::div()` for layout containers
- Use `gpui::Model<T>` for shared state
- Implement `Render` trait for view components
- Use actions for user interactions
- Keep views focused and composable

### Testing

- Run tests with `cargo test`
- Add unit tests in the same file as the code (`#[cfg(test)]` module)
- Add integration tests in `tests/` directory

### Common Commands

```bash
# Build the project
cargo build

# Run the application
cargo run

# Run in release mode
cargo run --release

# Format code
cargo fmt

# Check for issues
cargo clippy

# Run tests
cargo test
```

## Key Dependencies

- `gpui` - GUI framework from Zed
- `serde` / `serde_json` - Serialization
- `uuid` - Unique identifiers
- `chrono` - Date/time handling
- `cpal` - Cross-platform audio (recording/playback)
- `hound` - WAV file reading/writing
- `whisper-rs` - Whisper bindings for transcription
- `dirs` - XDG directory paths

## Error Handling

- Use `anyhow` for application errors
- Use `thiserror` for library-style errors with specific types
- Propagate errors with `?` operator
- Log errors appropriately before presenting to user

## Security Considerations

- Validate all file paths before operations
- Sanitize user input for filenames
- Don't store sensitive data in plain text
- Follow principle of least privilege for file access
