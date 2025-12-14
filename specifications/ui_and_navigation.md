# UI, Navigation, and Feature Flows

This document translates the SwiftUI + TCA presentation layer into platform-neutral behavior suitable for a desktop UI toolkit. State flows should map cleanly to view-models or Redux-style stores.

## Root Shell
- Root state hosts:
  - `recordScreen`: recording UI and live transcription switch.
  - `recordingListScreen`: library of recordings with playback and transcription controls.
  - `settingsScreen`: configuration, model management, storage tools.
  - `transcriptionWorker`: background queue driver.
  - `path`: navigation stack entries (`list`, `settings`, `details(recording)`).
- Idle timer is disabled while recording or transcribing.
- On launch, any in-progress transcriptions are marked failed and removed from the worker queue.

```python
class RootState:
    record_screen: RecordScreenState
    recording_list: RecordingListState
    settings: SettingsState
    transcription_worker: WorkerState
    nav_stack: list[Union["list","settings","details"]]
```

## Record Screen
- Components:
  - **Mic Selector**: choose input device from available microphones; persists selection.
  - **Recording Controls**: record/pause/resume/save/cancel.
  - **Live Transcription Model Selector**: visible when no active recording; gated behind premium purchase; toggles live mode and model choice.
- Flow:
  1. On record tap, request mic permission. If allowed, create `Recording` state with a new `RecordingInfo`.
  2. `Recording` reducer starts audio capture; if live mode is enabled, loads model and starts live transcription loop concurrently.
  3. Pause toggles audio engine pause; resume restarts; cancel deletes the partially recorded file.
  4. Save stops capture, marks any live transcription status as done, and emits delegate event to insert into the recording list.

## Recording List Screen
- Displays cards for each `RecordingInfo` (sorted by date).
- Each card shows title/date/duration, transcription snippet/status, playback controls, and buttons to start/cancel/resume transcription.
- Swipe-to-delete and an action button to import files (opens file picker).
- Background sync:
  - On appear and on app-foreground events, calls `Storage.sync` to reconcile disk files and persisted metadata.
  - When import completes, triggers optional iCloud upload.

```python
class RecordingCardState:
    recording: RecordingInfo
    player_controls: PlayerControlsState

    def transcribe_button_tapped(): enqueue_task(recording.id)
    def cancel_transcription(): cancel_task(recording.id)
    def resume_transcription(task): resume_task(task)
```

## Recording Details Screen
- Entry from a recording card tap.
- Modes:
  - **Text** view: concatenated transcription text (or empty/placeholder if not transcribed).
  - **Timeline** view: list of segments with `[start - end]` timestamps and text.
- Shared components:
  - Header with editable title and transcription status.
  - Waveform progress bar and play/pause control.
  - Bottom action sheet:
    - Restart transcription (re-enqueue job)
    - Delete recording
    - Copy/share audio (share sheet)
- Keeps a shared `displayMode` so toggles persist.

## Settings Screen
- Sections:
  - **Model Selector**: lists available/remote/local models; load/delete; shows sizes and properties; triggers reload.
  - **Premium Features**: manage live transcription purchase (StoreKit) and show status; on desktop, replace with licensing gate.
  - **Transcription Options**: language (auto or explicit), initial prompt, translation toggle, VAD toggle, compute preferences (GPU/Neural Engine flags), live transcription enable switch.
  - **Storage & Sync**: show free/used space, delete recordings, delete all models, toggle iCloud sync (copy WAVs to iCloud Drive).
  - **About/Links**: GitHub, website, rate app, bug/feature links.
- Derived fields: app version/build number, storage percentages.

## Share Extension (Import UX)
- Separate target on iOS that ingests shared audio, converts to WAV, and writes to the shared app-group folder.
- Main app pulls these files into Documents on next sync via `Storage.moveSharedFiles`.
- Desktop analog: support drag/drop or OS-level “Share to app” to drop files into a watched import directory.

## Visual/Interaction Notes
- Design system includes gradients, stylized typography, and animated buttons. Not essential to functionality but helpful for polish.
- Navigation stack should allow going from home to list/settings/details and back; consider standard desktop navigation with sidebars or tabs.
- Alerts/sheets used for confirmation (deletes) and contextual actions (recording actions sheet).
