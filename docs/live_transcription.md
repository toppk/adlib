# Live Transcription Architecture

This document describes the live transcription system in Adlib, including the evolution from the old approach to the new approach, the problems encountered, and future improvements.

## Overview

Live transcription converts speech to text in real-time as the user speaks. The system uses:
- **whisper.cpp** via `whisper-rs` bindings for speech recognition
- **RMS-based Voice Activity Detection (VAD)** to detect speech vs silence
- **Segment-based commits** to preserve transcribed text

## Architecture

### Audio Pipeline

```
┌─────────────┐     ┌──────────────┐     ┌────────────────┐     ┌─────────┐
│ PipeWire    │────>│ Capture      │────>│ LiveTranscriber│────>│ UI      │
│ (48kHz)     │     │ (resample    │     │ (16kHz buffer) │     │ Display │
│             │     │  to 16kHz)   │     │                │     │         │
└─────────────┘     └──────────────┘     └────────────────┘     └─────────┘
```

1. **Audio Capture**: PipeWire captures at 48kHz, stored in `CaptureState`
2. **Resampling**: Downsampled to 16kHz (Whisper's required sample rate)
3. **Buffering**: Samples accumulate in `LiveTranscriber.buffer`
4. **Processing**: Every ~500ms, Whisper transcribes the buffer
5. **Display**: Transcript updates in real-time

### Key Data Structures

```rust
struct LiveTranscriber {
    buffer: Vec<f32>,           // Accumulated audio samples (16kHz)
    current_text: String,       // Text from current speech segment
    committed_text: String,     // Finalized text from previous segments
    silence_count: usize,       // Consecutive silence detections
    vad_threshold: f32,         // Calibrated noise threshold
}
```

### Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `SAMPLE_RATE` | 16000 | Whisper's required sample rate |
| `STEP_SAMPLES` | 8000 | ~500ms of audio per processing step |
| `MAX_BUFFER_SAMPLES` | 480000 | 30 seconds max buffer (memory limit) |
| `SILENCE_COMMIT_THRESHOLD` | 3 | ~1.5s of silence triggers commit |
| `VAD_MULTIPLIER` | 3.0 | Threshold = ambient_noise × 3 |
| `MIN_VAD_THRESHOLD` | 0.02 | Floor for very quiet environments |

## Old Approach (Problematic)

The original implementation had several issues that caused text fragmentation and duplication.

### How It Worked

1. **Continuous buffering**: Audio samples accumulated in buffer
2. **Live transcription**: Whisper transcribed full buffer each cycle
3. **RESET detection**: If Whisper's output changed significantly (didn't start with first 20 chars of previous), commit old text immediately
4. **Silence detection**: After 1.5s silence, commit segment

### The RESET Logic (Old)

```rust
// Old problematic code
if !self.current_text.is_empty()
    && !full_text.starts_with(&self.current_text[..self.current_text.len().min(20)])
    && !is_suffix
{
    // Whisper "changed direction" - commit previous text
    self.commit_segment();  // Clears buffer!
    self.current_text = full_text;  // Accept new text
}
```

### Problems Encountered

#### 1. False RESET Triggers

When speaking "Okay, here we go. This is good.", the system would produce:

```
[LIVE] 'Okay.'
[RESET] Whisper changed direction, committing previous: 'Okay.'
[COMMIT] 'Okay.'
[LIVE] 'Okay, here we go.'
[RESET] Whisper changed direction, committing previous: 'Okay, here we go.'
[COMMIT] 'Okay, here we go.'
[LIVE] 'Go, this is good.'
...
```

**Result**: "Okay" appeared twice, text was fragmented.

**Root cause**: RESET triggered because "Okay, here we go." doesn't start with "Okay." (period vs comma). Punctuation changes during progressive transcription falsely triggered the reset.

#### 2. Buffer Clear After RESET

After `commit_segment()` cleared the buffer, Whisper only saw a tiny fragment of new audio. This caused a cascade:
- Small buffer → partial transcription → triggers RESET → clear buffer → repeat

#### 3. Accepting Stale Transcription

After clearing buffer in RESET, the code still set `current_text = full_text` where `full_text` came from the pre-clear buffer, causing duplicate content.

### Attempted Fixes (Also Problematic)

1. **Return early after RESET**: Prevented stale assignment but still caused fragmentation
2. **Suffix detection**: Tried to skip if new text was suffix of old - didn't address root cause
3. **Reduced silence threshold**: From 1.5s to 1s - just made fragmentation worse

## New Approach (Current)

The new approach is simpler and more robust: **trust Whisper's progressive updates, only commit on silence**.

### Key Changes

1. **Removed RESET logic entirely**: No mid-speech commits
2. **Final transcription on PAUSE**: Trim silence, re-transcribe speech portion
3. **Force commit at 30s**: Prevents unbounded memory growth
4. **Verbose logging**: Track VAD state for debugging

### How It Works

```rust
// New approach - simplified
pub fn process(&mut self) -> Result<bool, String> {
    // Force commit if buffer too long (30s)
    if self.should_force_commit() {
        self.transcribe_and_commit();
        return Ok(true);
    }

    // VAD check on recent audio
    let is_silence = calculate_rms(recent_audio) < self.vad_threshold;

    if is_silence {
        self.silence_count += 1;
        if self.silence_count >= SILENCE_COMMIT_THRESHOLD {
            // Trim silence, run final transcription, commit
            let speech_buffer = trim_trailing_silence(&self.buffer);
            self.current_text = transcribe(speech_buffer);
            self.commit_segment();
        }
        return Ok(false);  // Don't transcribe during silence
    }

    // Speech detected - transcribe full buffer for live feedback
    self.silence_count = 0;
    self.current_text = transcribe(&self.buffer);
    return Ok(true);
}
```

### Final Transcription on PAUSE

When silence is detected for 1.5 seconds:

1. Calculate how many samples are silence (~24000 samples = 1.5s)
2. Trim those from the buffer end
3. Run one final "authoritative" transcription on just the speech
4. Commit that result
5. Clear buffer for next segment

```rust
let silence_samples = SILENCE_COMMIT_THRESHOLD * STEP_SAMPLES;
let speech_end = self.buffer.len().saturating_sub(silence_samples);
let final_text = transcribe(&self.buffer[..speech_end]);
```

### Results

Speaking "Okay, here we go. Is this working? It looks okay.":

```
[LIVE] 'okay'
[LIVE] 'Okay, here we go.'
[LIVE] 'Okay, here we go. Is this working?'
[LIVE] 'Okay, here we go. Is this working? It looks okay.'
[SILENCE] count=1/3
[SILENCE] count=2/3
[SILENCE] count=3/3
[PAUSE] Running final transcription on 99299 samples (trimmed 24000 silence samples)
[COMMIT] 'Okay, here we go. Is this working? It looks okay.'
```

**Result**: Clean, accurate transcription with no duplication or fragmentation.

## Memory Management

### Buffer Limits

- **Pre-allocated**: `Vec::with_capacity(MAX_BUFFER_SAMPLES)` = 30 seconds
- **Force commit**: If buffer exceeds 30s, transcribe and commit immediately
- **Memory usage**: 30s × 16000 samples × 4 bytes = ~1.9 MB max

### Handling Long Speech

If user speaks continuously for >30 seconds without pausing:

```
[FORCE] Buffer at 480000 samples (30.0s), forcing commit
[COMMIT] '...'
```

The text is committed and buffer cleared, allowing continued transcription.

## Threading Model

```
┌─────────────────┐
│ Audio Capture   │ (callback thread - PipeWire)
│ └─> samples[]   │
└────────┬────────┘
         │
┌────────▼────────┐
│ Main Loop       │ (every 100ms)
│ - fetch samples │
│ - add to buffer │
│ - check ready   │
└────────┬────────┘
         │
┌────────▼────────┐
│ Background      │ (gpui background_executor)
│ - Whisper       │
│ - transcribe    │
└─────────────────┘
```

**Mutex contention**: During `process()`, the transcriber mutex is held. New samples queue up in `CaptureState` and are added on next iteration. No samples are lost, but there's latency.

## Known Limitations

1. **Whisper "forgetting"**: On very long utterances, Whisper may lose beginning context. The 30s force commit mitigates this.

2. **Latency**: ~500ms between speech and transcription update. Whisper processing time adds to this.

3. **RMS VAD limitations**: Simple energy-based detection can't distinguish speech from other sounds (typing, coughing).

## Future Improvements: Better VAD

### Current: RMS-based VAD

```rust
fn calculate_rms(samples: &[f32]) -> f32 {
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

let is_silence = rms < threshold;
```

**Pros**: Simple, fast, no dependencies
**Cons**: Can't distinguish speech from noise, sensitive to volume

### Alternative: Silero VAD

[Silero VAD](https://github.com/snakers4/silero-vad) is a neural network-based voice activity detector.

**Pros**:
- Specifically trained for speech detection
- Handles background noise better
- Works at various volume levels
- Available as ONNX model

**Cons**:
- Additional model download (~2MB)
- Requires ONNX runtime dependency
- More CPU usage per frame

**Integration approach**:
```rust
// Hypothetical Silero VAD integration
let vad_model = SileroVad::load("silero_vad.onnx")?;
let is_speech = vad_model.is_speech(&audio_chunk);
```

### Alternative: WebRTC VAD

Google's WebRTC includes a battle-tested VAD.

**Pros**:
- Well-tested in production (Chrome, Meet)
- Multiple aggressiveness levels
- Lightweight

**Cons**:
- C library, needs bindings
- Less accurate than neural approaches

### Alternative: whisper.cpp Built-in VAD

whisper.cpp has experimental VAD support using Silero:

```cpp
// whisper.cpp with VAD
params.vad = true;
params.vad_model_path = "silero_vad.onnx";
```

This would be the most integrated solution but requires whisper-rs to expose these parameters.

## Debugging

### Enable Verbose Logging

```bash
cargo run -- -vv
```

### Log Message Reference

| Tag | Meaning |
|-----|---------|
| `[SILENCE]` | VAD detected silence, shows count and RMS |
| `[SPEECH]` | VAD detected speech, resets silence count |
| `[LIVE]` | Updated live transcription text |
| `[PAUSE]` | Silence threshold reached, running final transcription |
| `[COMMIT]` | Segment committed to transcript |
| `[FORCE]` | Buffer exceeded 30s, forcing commit |

### Example Debug Session

```
[SILENCE] count=1/3, rms=0.0098, threshold=0.0300
[SPEECH] Detected, resetting silence count from 1
[LIVE] 'Hello world'
[SILENCE] count=1/3, rms=0.0095, threshold=0.0300
[SILENCE] count=2/3, rms=0.0092, threshold=0.0300
[SILENCE] count=3/3, rms=0.0098, threshold=0.0300
[PAUSE] Running final transcription on 48000 samples (trimmed 24000 silence samples)
[COMMIT] 'Hello, world.' (13 chars)
```

## Open Issues

### Whisper Context Loss During Long Utterances

**Problem**: During continuous speech without pauses, Whisper can suddenly "forget" the beginning of the utterance and output only the recent portion.

**Observed behavior**:
```
[06:27:18.768] [LIVE] 'And again, I'm saying something new now.'
[06:27:21.097] [LIVE] 'It's so.'
```

The text went from 40 characters to 8 characters - a complete loss of the beginning. The user said "And again, I'm saying something new now and it's so perfect" but only "It's so." was captured.

**Root cause analysis**:

The issue stems from the blocking architecture in the processing loop:

```
┌─────────────────────────────────────────────────────────────┐
│ Processing Loop (app.rs)                                    │
├─────────────────────────────────────────────────────────────┤
│ 1. Sleep 100ms                                              │
│ 2. Fetch ALL new samples from capture_state                 │
│ 3. Add samples to transcriber buffer                        │
│ 4. If ready: spawn Whisper and .await (BLOCKS HERE)         │
│    └── While blocked, audio accumulates in capture_state    │
│ 5. Loop back to step 1                                      │
└─────────────────────────────────────────────────────────────┘
```

When Whisper takes a long time to process (e.g., 2+ seconds for a long buffer):
1. The loop blocks at `.await`
2. PipeWire continues capturing audio into `capture_state`
3. When Whisper finishes, ALL accumulated audio is added at once
4. Buffer grows by 2+ seconds in a single spike
5. Next transcription has a much larger buffer
6. Whisper may lose context on the older audio

**Why the buffer matters**:

The buffer is always growing (never truncated during live transcription):
```rust
// add_samples() - always extends
self.buffer.extend_from_slice(samples);

// Only commit_segment() clears the buffer
fn commit_segment(&mut self) {
    ...
    self.buffer.clear();
}
```

So between the two log lines, the buffer contained the SAME audio plus MORE. Yet Whisper's output completely changed, suggesting Whisper's internal context handling couldn't cope with the longer audio.

**Potential causes**:
1. **Whisper context window overflow**: Whisper has internal limits on how much audio it can process coherently
2. **Spiky buffer growth**: Adding 2+ seconds of audio at once (vs. smooth 100ms increments) may affect Whisper's progressive processing
3. **Audio quality degradation**: Possibly unrelated audio artifacts in the accumulated samples

**Potential solutions** (not yet implemented):

1. **Non-blocking Whisper**: Run Whisper in a separate thread that doesn't block sample accumulation. Samples are added continuously while Whisper processes in parallel.

2. **Shorter force-commit threshold**: Reduce `MAX_BUFFER_SAMPLES` from 30s to 10-15s to commit before Whisper loses context.

3. **Sliding window transcription**: Only transcribe the last N seconds for live display, keeping full buffer for final PAUSE transcription.

4. **Content loss detection**: If new transcription is significantly shorter than previous (>50% loss), preserve the old text. (Attempted but reverted as it's a band-aid, not a fix.)

5. **Double-buffering**: Maintain two buffers - one for Whisper processing, one for accumulating new samples. Swap when processing completes.

**Status**: Unresolved. The issue occurs intermittently during long continuous speech without natural pauses. Adding pauses in speech allows PAUSE commits which reset the buffer, avoiding the problem.

## References

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) - C++ Whisper implementation
- [whisper-rs](https://github.com/tazz4843/whisper-rs) - Rust bindings
- [Silero VAD](https://github.com/snakers4/silero-vad) - Neural VAD
- [WebRTC VAD](https://webrtc.org/) - Google's VAD implementation
