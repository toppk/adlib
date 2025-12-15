# Adlib Development Guide

## Prerequisites

### System Dependencies

Adlib uses GPUI (Zed's GPU-accelerated UI framework) and PipeWire for audio capture, which require the following system libraries:

**Fedora/RHEL:**
```bash
sudo dnf install -y \
    freetype-devel \
    libxcb-devel \
    libxkbcommon-devel \
    libxkbcommon-x11-devel \
    pipewire-devel
```

**Ubuntu/Debian:**
```bash
sudo apt install -y \
    libfreetype-dev \
    libxcb1-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev \
    libpipewire-0.3-dev
```

### Rust Toolchain

Install Rust via rustup:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Building

```bash
# Development build
cargo build

# Release build (optimized)
cargo build --release

# Run the application
cargo run
```

## Project Structure

```
adlib/
├── src/
│   ├── main.rs            # Application entry point
│   ├── app.rs             # Main GPUI application component with all views
│   ├── models.rs          # Domain models (Recording, Transcription, Settings)
│   ├── audio/
│   │   ├── mod.rs         # Audio module exports
│   │   ├── capture.rs     # PipeWire audio capture with volume metering
│   │   ├── playback.rs    # PipeWire audio playback
│   │   └── recorder.rs    # WAV file recording via hound
│   ├── state/
│   │   ├── mod.rs         # State module exports
│   │   ├── app_state.rs   # Application state management
│   │   └── database.rs    # JSON persistence for recordings
│   ├── transcription/
│   │   └── mod.rs         # Whisper transcription (file + live)
│   └── whisper/
│       └── manager.rs     # Model download and management
├── docs/
│   ├── DEVELOPMENT.md     # This file
│   ├── ARCHITECTURE.md    # Technical architecture
│   └── FUTURE.md          # Roadmap and ideas
├── icons/                 # Application icons (hicolor theme)
├── specifications/        # Feature specifications
├── com.adlib.VoiceRecorder.desktop  # Desktop entry file
├── Cargo.toml             # Rust dependencies
├── CLAUDE.md              # AI development guidelines
├── LICENSE-MIT            # MIT License
└── LICENSE-APACHE         # Apache 2.0 License
```

## Architecture

### GPUI Framework

Adlib uses GPUI, the GPU-accelerated UI framework from the Zed editor. Key concepts:

- **Elements**: UI components built using a fluent builder pattern (`div().flex().p_4()...`)
- **Styled trait**: Common styling methods like `flex()`, `bg()`, `p_4()`, etc.
- **InteractiveElement trait**: Event handlers like `on_click()`, `on_hover()`
- **StatefulInteractiveElement trait**: Scroll handling requires `.id("...")` to create a stateful element

### Audio Pipeline

Audio capture uses PipeWire for low-latency microphone access:

- `AudioCapture`: Manages PipeWire stream in a background thread
- `SharedCaptureState`: Thread-safe state with volume levels and waveform data
- `WavRecorder`: Saves captured audio to 16kHz mono WAV files (Whisper-compatible)

### State Management

The application uses a unidirectional data flow pattern:

1. `AppState` holds all application state (current view, recordings, settings)
2. User actions trigger state mutations
3. GPUI re-renders the view when `cx.notify()` is called

### Views

The application has five main views:

1. **Live Transcription** (`ActiveView::Live`): Real-time speech-to-text without saving
2. **Record Screen** (`ActiveView::Record`): Voice recording interface with waveform visualization
3. **Recording List** (`ActiveView::RecordingList`): Browse and manage recordings
4. **Recording Details** (`ActiveView::RecordingDetails`): Playback and transcription view
5. **Settings** (`ActiveView::Settings`): Model selection and preferences

### Navigation

- Sidebar navigation for switching between views
- Keyboard shortcuts: F1 (help), Space (record), Ctrl+1/2/3 (views)
- Help overlay accessible via F1

## Desktop Integration

### Installing the Desktop Entry

```bash
# Install the desktop file
cp com.adlib.VoiceRecorder.desktop ~/.local/share/applications/

# Install icons
cp -r icons/hicolor/* ~/.local/share/icons/hicolor/

# Update icon cache
gtk-update-icon-cache ~/.local/share/icons/hicolor
```

### App ID for Wayland

The application uses `com.adlib.VoiceRecorder` as its app ID for Wayland/GNOME integration. This matches the `.desktop` file name for proper taskbar icon association.

## Common Tasks

### Adding a New View

1. Add variant to `ActiveView` enum in `src/state/app_state.rs`
2. Create render method in `src/app.rs`: `fn render_<view_name>(...)`
3. Add match arm in `render()` method
4. Add navigation option in sidebar if needed

### Adding a New Setting

1. Add field to `Settings` struct in `src/models.rs`
2. Update `Settings::default()` with default value
3. Add UI controls in `render_settings()` in `src/app.rs`

### Working with GPUI Styling

Common patterns:
```rust
div()
    .flex()                     // Enable flexbox
    .flex_col()                 // Column direction
    .gap_4()                    // Gap between children (16px)
    .p_4()                      // Padding (16px)
    .bg(rgb(0x1e1e2e))         // Background color
    .rounded_lg()               // Border radius
    .text_color(rgb(0xffffff)) // Text color
    .child(...)                // Add child element
```

For scrollable containers, add an ID first:
```rust
div()
    .id("my-scroll-container")  // Required for StatefulInteractiveElement
    .overflow_y_scroll()
    .child(...)
```

## Testing

```bash
# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run
```

## Debugging

The Vulkan warning `"radv is not a conformant Vulkan implementation"` is expected in VMs or with non-conformant GPU drivers and does not affect functionality.

For debugging GPUI layouts, you can use the built-in inspector (if enabled).

## Key Dependencies

- **gpui**: GPU-accelerated UI framework from Zed
- **pipewire**: Low-latency audio capture
- **hound**: WAV file reading/writing
- **hf-hub**: Hugging Face model downloading
- **tokio**: Async runtime for background tasks
- **serde/serde_json**: Serialization for settings and recordings
- **chrono**: Date/time handling
- **uuid**: Unique identifiers for recordings

## Related Documentation

- [Architecture](ARCHITECTURE.md) - Technical overview and design decisions
- [Future Roadmap](FUTURE.md) - Planned features and ideas
