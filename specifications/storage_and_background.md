# Storage, Persistence, Background Work, and Sync

This document captures how data is persisted, migrated, queued for background processing, and synced to external locations. Use equivalent filesystem and background primitives on Linux.

## Persistence Layout
- **Recordings**: `Documents/recordings.json` (array of `RecordingInfo`).
- **Settings**: `Documents/settings.json`.
- **Premium Features**: `Documents/premiumFeatures.json`.
- **Audio Files**: `Documents/*.wav` (one per recording) and optional waveform PNGs.
- **Models**: `Documents/huggingface/models/argmaxinc/whisperkit-coreml/<model_name>/`.
- **Temporary Import (Share Extension)**: `AppGroup/shared/share/*.wav` moved into `Documents` on next sync.
- **Transcription Tasks**: in-memory queue (`Shared(.transcriptionTasks)`).

## Storage Sync
- `Storage.sync` reconciles filesystem WAVs with persisted metadata:
  - Updates missing durations.
  - Moves files from the shared container into `Documents` with new UUID filenames.
  - Regenerates `RecordingInfo` rows for WAVs that lack metadata.
  - Deletes files shorter than 1s.
  - Returns sorted `RecordingInfo` list (newest first).
- `Storage.setAsCurrentlyRecording(url)` prevents active files from being picked up mid-record.

```python
async def sync_recordings(current: list[RecordingInfo]) -> list[RecordingInfo]:
    current = await update_missing_durations(current)
    current += move_shared_files_to_documents()
    files = list_documents_wav_files(excluding=current_recording_file)
    recordings = []
    for file in files:
        match = find(current, file_name=file)
        recordings.append(match or await create_info(file))
    recordings = [r for r in recordings if r.duration_seconds >= 1]
    return sorted(recordings, key=lambda r: r.date, reverse=True)
```

## Data Migration
- `DataMigrator` runs versioned migrations:
  - v1: converts legacy `RecordingInfo` schema (with plain text) into the new structure containing `Transcription`.
  - v2: migrates legacy `voiceLanguage` settings into `Settings.parameters.language`.

## Background Transcription Worker
- `TranscriptionWorker` manages a queue of `TranscriptionTask` objects.
- Behavior:
  - `enqueue(recording_id, settings)` adds a task and starts processing if idle.
  - Processes tasks FIFO; loads the model, updates status, calls WhisperKit on the WAV file, writes `Transcription` back into the associated `RecordingInfo`.
  - Supports `cancel(recording_id)` (removes queued task and cancels current if matching) and `resume(task)` (requeues paused tasks).
  - Uses UI background tasks (iOS) and BGProcessingTask scheduling to keep running while the app is backgrounded; on Linux, map to a worker thread/service.
  - Automatically marks dangling “in-progress” transcriptions as failed on app launch.

```python
async def process_queue():
    while queue and not cancel_requested:
        task = queue.pop(0)
        recording = lookup_recording(task.recording_info_id)
        update_transcription(task.id, status=("loading", None))
        try:
            await model_manager.load(task.settings.selected_model_name)
            result = await transcribe_file(recording.file_url, task.settings, progress_cb)
            update_transcription(task.id, status=("done", now), text=result.text, segments=result.segments)
        except Exception as e:
            update_transcription(task.id, status=("error", str(e)))
```

## Premium Features and Licensing
- Live transcription is gated by a StoreKit product (`me.igortarasenko.Whisperboard.LiveTranscription`).
- Purchase status is persisted to `premiumFeatures.json`.
- On desktop, replace with your licensing mechanism; ensure the flag `premiumFeatures.liveTranscriptionIsPurchased` controls access to live transcription UI/actions.

## iCloud Sync (Optional)
- If enabled in settings, recorded WAVs are copied to the user’s iCloud Drive container (`FileManager.default.url(forUbiquityContainerIdentifier: nil)`).
- Tracks uploaded filenames in `UserDefaults uploadedFiles`; can reset this cache.
- Runs after imports and new recordings.
- Desktop analog: copy WAVs to a configurable sync folder or invoke a cloud API.

```python
async def upload_recordings_to_cloud(reset: bool, recordings: list[RecordingInfo]):
    uploaded = [] if reset else load_uploaded_cache()
    for r in recordings:
        if r.file_name in uploaded: continue
        copy_file(r.file_url, cloud_folder / readable_name(r))
        uploaded.append(r.file_name)
    save_uploaded_cache(uploaded)
```

## File Import/Share Pipeline
- File picker import converts arbitrary audio to 16kHz mono WAV and inserts new `RecordingInfo`.
- Share extension processes incoming URLs, converts to WAV into shared container, and exposes “Open App” to continue in the main app.
- During the next `Storage.sync`, shared files are moved into `Documents`, assigned new IDs, and added to metadata.

## Safety and Cleanup
- Deleting recordings removes WAV files and metadata.
- “Delete storage” wipes all WAVs and resets recording metadata/settings to defaults.
- “Delete all models” removes all downloaded Whisper models and resets selected model to the default recommended model.
