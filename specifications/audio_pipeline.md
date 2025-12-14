# Audio Capture, Processing, and Playback

This document outlines how WhisperBoard handles microphone input, waveform generation, live transcription feeds, and playback so it can be rebuilt with Linux audio primitives (e.g., PulseAudio/ALSA + a DSP library).

## Capture Pipeline
- Uses `AVAudioEngine` input node with a tap to receive PCM buffers.
- Input buffers are resampled to **mono 16kHz float32** (WhisperKit requirement) via `AVAudioConverter`.
- Buffers are converted to `List[float]` samples, appended to a rolling `audioSamples` array, and relative energy is tracked for waveform and VAD.
- Each raw buffer is also written to an `AVAudioFile` at the converter’s input format, producing a WAV file on disk.

```python
class AudioProcessor:
    min_buffer_length_frames = 1600  # ~100ms at 16kHz
    audio_samples: list[float] = []
    relative_energy: list[float] = []
    audio_engine = None

    async def start_recording(raw_cb=None) -> Converter:
        reset_buffers()
        engine = AudioEngine()
        input_node = engine.input
        converter = AudioConverter(from_format=input_node.format, to_format=mono16k_float32)
        input_node.install_tap(buffer_size=min_buffer_length_frames, format=input_node.format,
            callback=lambda buf: handle_buffer(buf, converter, raw_cb))
        engine.start()
        self.audio_engine = engine
        return converter
```

## Recording Session Coordination
- `RecordingStream` actor owns recording state and writes audio to disk while exposing progress to UI.
- Uses `AudioSessionClient` to configure the session (`record` category, preferred mic).
- Tracks:
  - `isRecording`, `isPaused`
  - `fileURL`
  - `waveSamples` (relative energy slice for waveform)
  - `duration` (computed from audio file frames / sample rate)
- Public controls: `startRecording(file_url) -> AsyncStream[state]`, `pause`, `resume`, `stop`.

```python
class RecordingStream:
    state = RecordingState()
    audio_processor = AudioProcessor()
    audio_file = None

    async def start_recording(path, on_state):
        ensure_mic_permission()
        converter = await audio_processor.start_recording(raw_cb=self.on_buffer)
        audio_file = WavFileWriter(path, settings=converter.input_format)
        state.isRecording = True
        while state.isRecording:
            await sleep(0.3)  # UI poll interval
            on_state(state)

    def on_buffer(buf):
        state.waveSamples = audio_processor.relative_energy
        audio_file.write(buf)
        state.duration = audio_file.frames / audio_file.sample_rate

    def pause(): audio_processor.pause(); state.isPaused = True
    def resume(): audio_processor.resume(); state.isPaused = False
    def stop(): audio_processor.stop(); audio_file.close(); state = RecordingState()
```

## Live Transcription Feed
- Shares the same `AudioProcessor` buffers with `TranscriptionStream`.
- VAD uses the rolling `relative_energy` to gate transcription when silence exceeds a threshold.
- Loop pulls new samples every 100ms and triggers Whisper transcription when at least one second of new audio is available.

## Microphone Selection
- `AudioSessionClient` lists available microphones, remembers the selected one in preferences, and sets it as preferred input when enabling the session.
- Session categories allow Bluetooth and default-to-speaker; an option controls whether to mix with other audio.

## Playback
- `AudioPlayerClient` wraps `AVAudioPlayer`:
  - `play(url) -> AsyncStream[PlaybackState]` emits `playing/pause/stop/finish/error` with periodic `PlaybackPosition` updates (every 0.5s).
  - `seekProgress(fraction)`, `pause`, `resume`, `stop`, and `speed(rate)` controls.
- Playback activates the audio session for `.playback` and tears it down when finished.

```python
async def play_file(url, on_state):
    player = AudioPlayer(url)
    player.play()
    while player.is_active():
        on_state(("playing", PlaybackPosition(player.current_time, player.duration)))
        await sleep(0.5)
    on_state(("finish", True))
```

## File Import and Format Normalization
- Importing external audio (from file picker or share extension) converts to **16kHz mono WAV** using `FormatConverter` (AudioKit). On Linux, use an equivalent transcoder (e.g., ffmpeg/libsox) to ensure all pipeline inputs share the same sample rate and channel layout.
- Imported files are saved into the app’s recordings directory with generated IDs and metadata rows in `RecordingInfo`.

## Live vs Offline Recording Modes
- Offline mode: capture to WAV only; transcription happens later via the background worker queue.
- Live mode (premium + toggle): concurrently loads the selected model, starts live transcription loop, and updates the in-memory `RecordingInfo.transcription` segments/timings as buffers arrive.
