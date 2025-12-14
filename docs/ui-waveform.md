# Waveform Visualization

This document describes the live waveform visualization system used in Adlib's recording interface.

## Overview

The waveform display shows real-time audio levels as a series of vertical bars that scroll from right to left. New audio data appears on the right and scrolls left as time progresses, similar to a traditional oscilloscope or audio workstation display.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  PipeWire Audio Thread                                      │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Audio callbacks (~48/sec at 48kHz with 1024 buffer) │   │
│  │  ↓                                                   │   │
│  │  Calculate RMS (root mean square) for volume level   │   │
│  │  ↓                                                   │   │
│  │  Accumulate over DECIMATION callbacks (default: 4)   │   │
│  │  ↓                                                   │   │
│  │  Push averaged sample to waveform_samples buffer     │   │
│  │  (keeps last 96 samples)                             │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│  SharedCaptureState (thread-safe)                           │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  waveform_samples: Vec<f32>  (RMS values, 0.0-1.0)   │   │
│  │  last_waveform_time: Instant (for smooth scrolling)  │   │
│  │  waveform_interval_secs: f32 (actual sample period)  │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│  UI Render (60 fps)                                         │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  Get waveform_samples and scroll_phase               │   │
│  │  ↓                                                   │   │
│  │  For each bar (48 total):                            │   │
│  │    - Map bar index to sample index                   │   │
│  │    - Interpolate between current and next sample     │   │
│  │      using scroll_phase for smooth animation         │   │
│  │    - Scale to pixel height (5-60px)                  │   │
│  │    - Color based on level (green/orange/red)         │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## Key Parameters

| Parameter | Value | Location | Purpose |
|-----------|-------|----------|---------|
| `num_bars` | 48 | `app.rs` | Number of vertical bars displayed |
| `bar_width` | 4px | `app.rs` | Width of each bar |
| `bar_height_max` | 60px | `app.rs` | Maximum bar height |
| `bar_height_min` | 5px | `app.rs` | Minimum bar height (silence) |
| `WAVEFORM_DECIMATION` | 4 | `capture.rs` | Audio callbacks per waveform sample |
| `buffer_size` | 96 | `capture.rs` | Max samples kept in buffer |

## Timing Calculation

With typical PipeWire settings (48kHz sample rate, ~1024 sample buffer):
- Audio callbacks: ~47 per second
- With decimation of 4: ~12 waveform samples per second
- With 48 bars: ~4 seconds of audio history displayed

```
Display time = (num_bars × DECIMATION) / callbacks_per_second
             = (48 × 4) / 47
             ≈ 4 seconds
```

## Display Behavior

The waveform displays as a "cityscape" that pans left:
- Each bar has a fixed height based on its RMS sample value
- When a new sample arrives, all bars shift left by one position
- New audio appears on the right edge
- The leftmost sample scrolls off and is discarded

The update rate is controlled by the decimation factor - with DECIMATION=4 and ~48 audio callbacks/second, the display updates ~12 times per second.

### Future: Smooth Scrolling
True sub-pixel smooth scrolling would require better understanding of GPUI's rendering pipeline. The current discrete approach works well and shows ~4 seconds of audio history.

## Color Thresholds

Bars are colored based on their height to indicate volume level:

| Height Range | Color | Meaning |
|--------------|-------|---------|
| 5-35px | Green (#4CAF50) | Normal speaking level |
| 35-54px | Orange (#FF9800) | Loud |
| 54-60px | Red (#e94560) | Very loud / clipping risk |

## RMS Calculation

Volume is measured using Root Mean Square (RMS), which gives a better representation of perceived loudness than peak detection:

```rust
let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
let rms = (sum_squares / samples.len() as f32).sqrt();
```

The RMS is then smoothed for display:
```rust
volume_level = volume_level * 0.7 + rms * 0.3;
```

## Customization

To adjust the waveform behavior:

### Slower/Faster Scrolling
Change `WAVEFORM_DECIMATION` in `src/audio/capture.rs`:
- Higher value = slower scrolling, more time displayed
- Lower value = faster scrolling, less time displayed

### More/Fewer Bars
Change `num_bars` in `src/app.rs`:
- More bars = finer detail, more samples shown
- Fewer bars = coarser display, less clutter

### Adjust Color Sensitivity
Modify the height thresholds in `src/app.rs`:
```rust
if height > 54.0 {
    rgb(0xe94560) // Red
} else if height > 35.0 {
    rgb(0xFF9800) // Orange
} else {
    rgb(0x4CAF50) // Green
}
```

## Files

- `src/audio/capture.rs` - Audio capture and waveform sample generation
- `src/app.rs` - Waveform UI rendering (in `render_record_view`)
