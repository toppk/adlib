# Adlib Future Roadmap

This document captures ideas and planned features for Adlib's future development.

## Background Service

**Goal**: Run Adlib as a system service that's always listening, rather than a foreground GUI application.

### Concepts

- **Daemon mode**: `adlib --daemon` runs headless, listening for activation
- **D-Bus interface**: Expose control API for start/stop/status
- **Socket activation**: systemd socket activation for on-demand startup
- **Tray icon**: Optional system tray presence for quick access

### Architecture Ideas

```
┌─────────────────────────────────────────────────────────┐
│                    adlib-daemon                          │
│  - Always running (systemd user service)                 │
│  - Listens for hotkey / D-Bus activation                 │
│  - Manages audio capture and transcription               │
└─────────────────────────────────────────────────────────┘
                           │
              D-Bus / Unix Socket
                           │
         ┌─────────────────┼─────────────────┐
         ▼                 ▼                 ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│   adlib-gui     │ │  adlib-cli      │ │  compositor     │
│  (full UI)      │ │ (terminal tool) │ │  integration    │
└─────────────────┘ └─────────────────┘ └─────────────────┘
```

### Implementation Notes

- Use `zbus` for D-Bus integration
- systemd user unit: `~/.config/systemd/user/adlib.service`
- Consider libnotify for transcription notifications
- Global hotkey registration (XDG portal or compositor-specific)

---

## Terminal Integration

**Goal**: Use voice input directly in terminal applications.

### Use Cases

1. **Voice-to-command**: Speak a command, have it typed into the terminal
2. **Voice dictation**: Dictate text for editors, REPLs, etc.
3. **Shell integration**: `$(adlib listen)` captures speech and returns text

### Approaches

#### PTY Injection
- Inject transcribed text directly into a pseudo-terminal
- Works with any terminal emulator
- Requires PTY access or terminal-specific integration

#### Clipboard-based
- Transcribe to clipboard, user pastes with Ctrl+Shift+V
- Simple, works everywhere
- Already supported (primary selection + clipboard)

#### Shell Function
```bash
# In .bashrc / .zshrc
voice() {
    local text=$(adlib listen --timeout 5)
    READLINE_LINE="${READLINE_LINE}${text}"
    READLINE_POINT=${#READLINE_LINE}
}
bind -x '"\C-v": voice'  # Ctrl+V to voice input
```

#### Terminal Emulator Plugin
- Integrate directly with specific terminals (kitty, wezterm, alacritty)
- Use their extension/scripting APIs
- Provides tightest integration

### CLI Interface Ideas

```bash
# Listen and output text
adlib listen [--timeout SECONDS] [--model MODEL]

# Continuous listening mode
adlib listen --continuous

# Pipe mode (for scripting)
echo "transcribe this" | adlib transcribe -

# Status/control when running as daemon
adlib status
adlib start
adlib stop
```

---

## Desktop Compositor Integration

**Goal**: Control the desktop environment using voice commands.

### Vision

Speak commands like:
- "Open Firefox"
- "Switch to workspace 2"
- "Tile window left"
- "Take a screenshot"
- "Volume up"

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  Live Transcription                      │
└─────────────────────────────────────────────────────────┘
                           │
                    Command Parser
                    (intent recognition)
                           │
         ┌─────────────────┼─────────────────┐
         ▼                 ▼                 ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│  App Launcher   │ │   Window Mgmt   │ │  System Control │
│  (xdg-open,     │ │ (wlr-randr,     │ │  (pactl,        │
│   gio launch)   │ │  swaymsg, etc)  │ │   brightnessctl)│
└─────────────────┘ └─────────────────┘ └─────────────────┘
```

### Compositor-Specific Integration

#### Sway / i3
- Use IPC protocol (`swaymsg`, `i3-msg`)
- Full window/workspace control
- Custom keybinding simulation

#### GNOME
- D-Bus interface for shell extensions
- `gdbus` commands or `gi` bindings
- XDG Desktop Portal for some actions

#### KDE Plasma
- D-Bus interfaces for KWin
- `qdbus` commands
- KRunner for app launching

#### Hyprland
- IPC socket protocol
- `hyprctl` commands
- Active window queries

### Command Recognition Approaches

#### Pattern Matching
Simple keyword-based matching:
```rust
match transcription.to_lowercase() {
    t if t.contains("open firefox") => launch("firefox"),
    t if t.contains("workspace") => parse_workspace_command(t),
    t if t.contains("volume") => parse_volume_command(t),
    _ => None,
}
```

#### Grammar-Based
Define a command grammar:
```
command := action target
action  := "open" | "close" | "switch to" | "move to"
target  := app_name | "workspace" number | direction
```

#### LLM-Assisted (Future)
- Send transcription to local LLM for intent parsing
- More flexible natural language understanding
- Could use llama.cpp or similar

### Wake Word Detection

For always-listening mode, need wake word detection:
- "Hey Adlib, open terminal"
- "Computer, switch workspace"

Options:
- Porcupine (proprietary but good)
- OpenWakeWord (open source)
- Simple keyword spotting in Whisper output

---

## Other Ideas

### Export & Sharing
- Export transcripts as Markdown, SRT subtitles, plain text
- Share audio + transcript bundles
- Integration with note-taking apps

### Multi-Language Support
- Language detection and switching
- Translation mode (transcribe + translate)
- Per-recording language settings

### Audio Improvements
- Noise cancellation / enhancement
- Speaker diarization (who said what)
- Audio bookmarks during recording

### Accessibility
- Screen reader compatibility
- High contrast themes
- Keyboard-only navigation (already partially supported)

### Cloud Sync (Optional)
- Sync recordings across devices
- Use Syncthing, Nextcloud, or cloud storage
- End-to-end encryption for privacy

### Developer & Debugging Tools
- JSON Lines logging mode (`--log-jsonl <path>`) for structured event output
- Easier log analysis and automated testing of transcription quality
- Event types: LIVE, COMMIT, PAUSE, SEGMENT, SILENCE, SPEECH
- Python script for parsing text logs: `scripts/parse_debug_log.py`

---

## Priority Roadmap

### Phase 1: Polish Current Features
- [x] Improve hallucination filtering
- [ ] Better error handling and recovery
- [ ] Settings persistence
- [ ] Keyboard shortcut customization
- [ ] JSON Lines logging mode for debugging

### Phase 2: Background Service
- [ ] Daemon mode with D-Bus API
- [ ] systemd user service
- [ ] Global hotkey activation
- [ ] Notification integration

### Phase 3: Terminal Integration
- [ ] CLI interface (`adlib listen`, `adlib transcribe`)
- [ ] Shell function for voice input
- [ ] Continuous listening mode

### Phase 4: Desktop Control
- [ ] Command parser framework
- [ ] Sway/i3 integration
- [ ] Basic app launching
- [ ] Window management commands

### Phase 5: Advanced Features
- [ ] Wake word detection
- [ ] LLM-assisted command parsing
- [ ] Multi-compositor support
