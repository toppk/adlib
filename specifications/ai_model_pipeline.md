# AI Model Usage & Retrieval

This document describes how WhisperBoard orchestrates Whisper models through WhisperKit. Replicate the lifecycle with whichever Whisper-compatible runtime you choose on Linux.

## Model Sources and Storage
- Remote models are fetched from Hugging Face repo `argmaxinc/whisperkit-coreml`.
- Local cache lives under `Documents/huggingface/models/argmaxinc/whisperkit-coreml`.
- `WhisperKit.recommendedModels()` provides a default model name and a set of disabled models (e.g., too heavy for device). The selected model is kept in `Settings.selected_model_name`.
- The app formats model filenames reported by WhisperKit so UI can show human-friendly variants.

## Compute and Decode Options
- Compute units are configurable:
  - Encoder: CPU+GPU, decoder: CPU+Neural Engine (default on iOS). On Linux, map to your available backends (CPU/GPU).
- Decoding options set per transcription:
  - Task: transcribe (no translate unless `parameters.should_translate` is true).
  - Language: explicit language code or auto (default).
  - Temperature schedule: start at 0, increment 0.2 up to `fallbackCount` on fallback.
  - Sample length: 224 frames.
  - Prefill prompt/cache toggles, skip special tokens, compression and log-prob thresholds, VAD/no-speech thresholds.
  - Word timestamps enabled for accurate segment timing.

## Model Lifecycle Pseudocode

```python
class ModelManager:
    repo = "argmaxinc/whisperkit-coreml"
    cache_dir = Path.home() / "Documents/huggingface/models/argmaxinc/whisperkit-coreml"
    available_models: list[str] = []
    local_models: list[str] = []
    remote_models: list[str] = []
    selected_model: str
    model_state: Literal["unloaded","downloading","downloaded","prewarming","loading","loaded"] = "unloaded"
    loading_progress: float = 0.0  # 0..1

    async def fetch_catalog(self):
        if cache_dir.exists():
            self.local_models = format_model_files(cache_dir.listdir())
            self.available_models += self.local_models
        self.remote_models = await list_repo_files(repo)
        self.available_models += [m for m in self.remote_models if m not in self.available_models]

    async def load(self, model_name: str, redownload: bool = False, progress_cb=None):
        self.model_state = "downloading"
        folder = cache_dir / model_name if model_name in self.local_models and not redownload else None
        if not folder:
            folder = await download_model(repo, model_name, progress_cb)
        self.model_state = "downloaded"
        self.loading_progress = 0.7
        self.model_state = "prewarming"
        await prewarm(folder, progress_cb=lambda p: self.loading_progress = 0.7 + 0.2 * p)
        self.model_state = "loading"
        await load_into_runtime(folder)
        self.model_state = "loaded"
        self.loading_progress = 1.0
        if model_name not in self.local_models:
            self.local_models.append(model_name)
```

## Offline Transcription Flow (Queued Jobs)
1. Load selected model via `load(model)`; progress events drive UI.
2. Call `transcribeAudioFile(file_url)` with decode options derived from Settings.
3. WhisperKit returns multiple `TranscriptionResult`s; merge into a single text and segment list.
4. Update `Transcription` status: `.loading -> .progress(fraction,text) -> .done(date)` or `.error(message)`.

```python
async def transcribe_file(path: Path, settings: Settings, progress_cb) -> Transcription:
    runtime = await model_manager.load(settings.selected_model_name)
    opts = build_decode_options(settings)
    transcription = Transcription(
        id=uuid4(), file_name=path.name,
        parameters=settings.parameters, model_name=settings.selected_model_name,
        status="loading"
    )
    for progress in runtime.transcribe_file(path, opts):
        transcription.status = ("progress", progress.fraction, progress.text)
        progress_cb(transcription)
    result = runtime.result()
    transcription.segments = merge_segments(result.segments)
    transcription.text = result.text
    transcription.timings.tokens_per_second = result.timings.tokens_per_second
    transcription.timings.full_pipeline_seconds = result.timings.full_pipeline
    transcription.status = ("done", datetime.utcnow())
    return transcription
```

## Live Transcription Loop
- Runs concurrently with recording when enabled and purchased.
- Pulls audio samples from the shared `AudioProcessor` buffer; waits until at least one second of new audio is available.
- Optional Voice Activity Detection (VAD) short-circuits processing when silence is detected based on relative energy and a silence threshold.
- Keeps track of `confirmedSegments` vs `unconfirmedSegments` to stabilize the last N segments; uses `lastConfirmedSegmentEndSeconds` to seed `clipTimestamps`.

```python
async def live_transcription_loop(audio_source, settings, callback):
    opts = build_decode_options(settings)
    state = LiveState(last_confirmed_end=0.0, unconfirmed=[], confirmed=[])
    while state.is_working:
        samples = audio_source.read_samples()
        if len(samples) - state.last_buffer_size < sample_rate:  # <1s new audio
            await sleep(0.1); continue
        if settings.is_vad_enabled and not voice_detected(audio_source.energy):
            await sleep(0.1); continue
        state.last_buffer_size = len(samples)
        result = await runtime.transcribe_array(samples, opts, start_from=state.last_confirmed_end)
        segments = result.segments
        if len(segments) > required_confirmation:
            confirm = segments[:-required_confirmation]
            state.confirmed += [s for s in confirm if s.end > state.last_confirmed_end]
            state.last_confirmed_end = state.confirmed[-1].end if state.confirmed else state.last_confirmed_end
            state.unconfirmed = segments[-required_confirmation:]
        else:
            state.unconfirmed = segments
        callback(state)  # provides confirmed+unconfirmed, tokens/sec, progress fraction
```

## Model Cleanup
- Individual deletion removes the folder for that model and unloads it if currently selected.
- “Delete all models” wipes the local model directory and resets selection to the default.

## Error and Fallback Handling
- Progress callbacks monitor WhisperKit fallbacks (temperature increases) and compression/log-prob heuristics to decide early stopping.
- On failed prewarm/load, the loader retries once with `redownload=True`; otherwise the model state resets to unloaded.
