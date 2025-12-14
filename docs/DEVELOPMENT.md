# Adlib Development Guide

## Prerequisites

### System Dependencies

Adlib uses GPUI (Zed's GPU-accelerated UI framework) which requires the following system libraries:

**Fedora/RHEL:**
```bash
sudo dnf install -y \
    freetype-devel \
    libxcb-devel \
    libxkbcommon-devel \
    libxkbcommon-x11-devel
```

**Ubuntu/Debian:**
```bash
sudo apt install -y \
    libfreetype-dev \
    libxcb1-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev
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
│   ├── main.rs          # Application entry point
│   ├── app.rs           # Main GPUI application component with all views
│   ├── models.rs        # Domain models (Recording, Transcription, Settings)
│   └── state/
│       ├── mod.rs       # State module exports
│       └── app_state.rs # Application state management
├── docs/
│   └── DEVELOPMENT.md   # This file
├── specifications/      # Feature specifications
├── Cargo.toml           # Rust dependencies
├── CLAUDE.md            # AI development guidelines
├── LICENSE-MIT          # MIT License
└── LICENSE-APACHE       # Apache 2.0 License
```

## Architecture

### GPUI Framework

Adlib uses GPUI, the GPU-accelerated UI framework from the Zed editor. Key concepts:

- **Elements**: UI components built using a fluent builder pattern (`div().flex().p_4()...`)
- **Styled trait**: Common styling methods like `flex()`, `bg()`, `p_4()`, etc.
- **InteractiveElement trait**: Event handlers like `on_click()`, `on_hover()`
- **StatefulInteractiveElement trait**: Scroll handling requires `.id("...")` to create a stateful element

### State Management

The application uses a unidirectional data flow pattern:

1. `AppState` holds all application state (current view, recordings, settings)
2. User actions trigger state mutations
3. GPUI re-renders the view when `cx.notify()` is called

### Views

The application has four main views:

1. **Record Screen** (`ActiveView::Record`): Voice recording interface with waveform visualization
2. **Recording List** (`ActiveView::RecordingList`): Browse and manage recordings
3. **Recording Details** (`ActiveView::RecordingDetails`): Playback and transcription view
4. **Settings** (`ActiveView::Settings`): Model selection and preferences

### Navigation

- Sidebar navigation for switching between views
- Keyboard shortcuts: F1 (help), Space (record), Ctrl+1/2/3 (views)
- Help overlay accessible via F1

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

## Future Development

Areas for implementation:
- Audio capture using ALSA/PulseAudio/PipeWire
- Whisper model integration for transcription
- File persistence for recordings and settings
- Export functionality (text, audio formats)
- Cloud sync support
