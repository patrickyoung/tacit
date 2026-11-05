# Tacit

A Rust desktop sketchbook that turns thoughts into an evolving visual narrative. Built with [Rune](https://github.com/anomalyco/rune), the graphical framework used by [Hokum](https://github.com/anomalyco/hokum).

## Run

With a current stable Rust toolchain:

```sh
cargo run -- --demo
```

The demo is an isolated, editable example; it never writes to your real session. Start a persistent session with:

```sh
cargo run
```

Type a thought and watch its tentative placement update immediately. Press **Enter** to keep it; **Shift+Enter** adds a newline. Select a thought to inspect its original words, move it to the next branch, or remove it. **Undo** reverses the last capture, move, or removal. **Read the thread** switches to a narrative view. Tab navigates controls. Cmd+S (Ctrl+S on Linux) retries saving, and Escape clears selection.

The map is a colorful radial tree: an illustrated central idea, tapered curved branches, and shorter thought labels. Select a thought to read its complete original wording. OpenRouter creates up to eight named thematic branches. Offline, four narrative lenses provide a starting point: observations, questions, tensions, and possibilities. Moving a thought pins its classification so the organizer cannot override it. Long regions scroll; smaller windows move the inspector controls beneath the map. Colored branches indicate classification, not causal claims.

## Generated thematic artwork

Tacit now uses OpenRouter image-generation models to paint original scenes for its themes. The default is `openai/gpt-image-2.5-flare`, with medium quality and transparent backgrounds. The **Art · pastel** control cycles through pastel, chalk, bold marker, and off. The image model receives a theme's visual brief and name, not the entire notebook. Older themes without a brief use up to three kept thoughts as context.

The semantic organizer writes a concrete scene brief for each category and preserves it verbatim until the meaning changes. Images are generated on an independent background worker after themes have settled for five seconds. Classification and typing stay responsive. Images arrive one at a time; a feathered 2.4-second reveal and crossfade preserve the previous artwork while replacements are generated. Existing vector motifs remain the immediate/offline fallback. Reduced-motion mode shows completed images without reveal effects.

Generation uses your existing OpenRouter key and credits. The cache is keyed by model, style and scene prompt, so reopening a map or returning to a style reuses its art. Cache files live under `$XDG_CACHE_HOME/tacit/art` (default `~/.cache/tacit/art`), with private permissions on Unix, bounded to 128 PNGs / 256 MiB. These images may depict private notebook concepts. No API keys or plaintext prompts are stored in the image cache.

Set `TACIT_IMAGE_ART=off` to disable cloud image generation while keeping local artwork. `TACIT_IMAGE_MODEL` overrides the default; the selected model must support the Images API, transparent PNG output, medium quality and square aspect ratio. **Art · off** also stops queued work. A request already running may finish into the cache, but obsolete results cannot replace current art. Failed images preserve existing artwork; **Retry art** retries without changing your thoughts. No automatic retry loop runs.

The visual vocabulary is no longer limited to the twelve fallback icons: generated scenes can combine concepts from the thoughts. Raster artwork reveals as a soft wash; the fallback vectors retain their stroke-by-stroke animation. Style selection is saved with the presentation track.

To verify the complete image-generation path with one fictional demo theme (a paid request unless cached):

```sh
cargo run -- --check-image
cargo run -- --scene generated --snap /tmp/tacit-generated.png --size 1280x860
```

Normal headless snapshots never contact the network; the generated demo scene loads only existing cache entries. The explicit `--check-image` diagnostic validates the PNG by decoding it before caching.

## Voice with Hex

Open Hex with a local model loaded and timed transcription streams enabled. Tacit records **one microphone** through Rune into a 16 kHz mono PCM WAV, then sends those exact samples to Hex locally. Recording currently requires macOS because Rune's native recorder backend is macOS-only. Hex's stream endpoint is cross-platform; a native recorder backend is still needed for other platforms.

1. Click **Listen**. macOS may ask for microphone permission for Tacit or its launching terminal. The recording indicator and timer stay visible.
2. Click **Stop & transcribe**. The WAV is retained before transcription starts. Hex supplies audio-window segment timestamps; Tacit exports them as WebVTT without inventing word timings.
3. In Brainstorm mode, the complete transcript becomes a thought, then the stronger OpenRouter review model splits it into coherent ideas and categorizes them using discussion context. Your typed draft stays untouched. **Undo** can restore the unsplit thought; it does not erase historical audio or events.

**Cancel** or **Escape** discards an active recording. Once stopped, recordings remain part of the session archive, including when transcription fails. **Retry transcript** retries a retained recording without using the microphone. **Reconnect Hex** checks that the local transcription endpoint is available. This integration does not use Hex's dictation audio mirror because that mirror may drop chunks; the WAV owned by Tacit is the authoritative audio.

Transcription stays local in Hex. Semantic splitting, classification, and illustration still use OpenRouter when configured. Headless snapshots never open a microphone or automatically call either service. Diagnostic commands:

```sh
cargo run -- --check-hex
cargo run -- --check-audio /path/to/16000hz-mono-pcm16.wav
cargo run -- --check-recording # fictional semantic splitting example; uses OpenRouter
```

## Saved sessions and replay

Beside `session.json`, Tacit creates a private `session.tacit/` archive:

- `events.jsonl`: versioned, ordered events with millisecond session timestamps, sequence numbers, and a BLAKE3 integrity chain. Content events capture thoughts, corrections, deletions, semantic splits, AI classifications/narrative/relationships, undo, transcript links, and available artwork. A separate, versioned presentation track stores Rune draft, selection, art style, vector drawing state, image transitions, scrolling, and viewport metadata. Recording, transcript, and failure events retain their original order.
- `recordings/`: original captured WAVs, including stopped recordings whose transcription is pending or failed.
- `media/`: content-addressed, durable audio and generated PNG assets plus per-recording `.vtt` files. These assets are independent of the bounded image-generation cache.
- `transcript.vtt`: captions on the session timeline, including gaps between recordings. Per-recording VTT uses time relative to its WAV. Caption boundaries are Hex's actual transcription windows, not inferred boundaries for the semantic thought chunks.

The event log is authoritative; `session.json` remains a compatibility snapshot. Existing notebooks are imported as the initial event without inventing past history. Live content follows **action → event → durable append → reducer projection → renderer**. Captures, corrections, deletion, splitting, model results, and undo cannot publish content before their event is saved; failed writes keep the previous projection. Replay uses the same pure reducer. Rune reads the shared projection for content and manages its own presentation controls and animations. Model results and images are replayed from saved data; no model calls occur during playback. Stroke reveals, image crossfades, and classification fades use recorded time; hidden Rune layout springs have been removed so seeking does not depend on prior animation frames.

### Original pixels and exact replay

Persistent sessions now capture the actual Tacit render texture before presenting it to the window. Each frame is stored as a lossless, content-addressed RGBA PNG with its timestamp, device-pixel dimensions, display scale, and content revision. This records Tacit's application pixels, including its controls and draft; it does not record other applications or the OS desktop. Headless demos only capture when explicitly launched with `--capture-demo NEW_PATH`.

Replay offers **Scene replay** and **Original pixels**. Scene replay uses the recorded presentation's content revision and sampled animation clock; it does not show later content before the frame that displayed it. Related transcript/capture/link events and artwork/presentation events commit atomically. Recorded map dimensions prevent replay controls or a different window width from rearranging branches. Unsupported or missing presentation tracks fail strict replay instead of silently generating a new scene. Older archives remain usable, but their missing original pixels cannot be recovered retroactively.

**Export lossless video** writes an `.mkv` with lossless FFV1 video and the original PCM audio, aligned on the same timeline. Video starts at its first captured frame; the `tacit_session_start_ms` container tag records its offset into the session/WebVTT timeline. Window resizing adds black padding without scaling or changing the original frame pixels. Capture stops/restarts are explicit; intervals when original capture is paused are black. An interrupted capture is reported, and export refuses to claim completeness for an archive with recorded capture failures. The PNG masters and event log remain authoritative. FFmpeg is required for export (`brew install ffmpeg` on macOS, or set `TACIT_FFMPEG`); recording and in-app original-pixel playback do not require it.

```sh
cargo run -- --replay /path/to/session.tacit --replay-pixels
cargo run -- --export-video /path/to/session.tacit /path/to/original.mkv
```

The first capture implementation uses synchronous GPU readback and durable writes, prioritizing retaining every application frame over throughput. It adds rendering latency and retains potentially substantial media; no automatic pruning is performed. It records the frames Tacit presents, not a promised fixed frame rate. Frame timestamps have millisecond resolution. Scene replay remains renderer/version dependent; original-pixel playback and lossless video preserve the visual record independently of future renderer changes.

The export uses the [Matroska FFV1 codec mapping](https://www.matroska.org/technical/codec_specs.html#v_ffv1) and [Matroska timestamp semantics](https://www.matroska.org/technical/notes.html).

### Renderer contract

`Journal::at(milliseconds)` returns the shared projection. `Projection::content()` exposes only renderer-independent data: the session (thoughts, categories, interpretations and relationships), semantic artwork references, audio, transcripts, and thought provenance. It contains no Rune drawing objects, layout positions, or animation state. Artwork assets are associated with theme IDs; each renderer decides their placement. Historical assets and provenance survive deletion or undo.

`SessionEvent::reduce` is the pure content reducer. New renderers submit content events through the journal and render the resulting projection. They can use their own versioned `Presentation` events without changing content or having to interpret another renderer's presentation payload. Header compatibility checks the **timeline schema**, not the original renderer version. Version 1 scene archives remain readable through a legacy adapter; new live edits emit version 2 content and presentation events. Existing archives are not rewritten.

A minimal second renderer demonstrates this contract without invoking Rune's UI or any AI services:

```sh
cargo run --example render_text -- /path/to/session.tacit 12500
cargo run --example render_text -- /path/to/session.tacit 12500 --json
```

The JSON form is a portable content view, not a Rune scene dump. In-memory checkpoints accelerate seeking; rebuilding them from the event log produces the same state. Rune presentation reproduction requires its matching presentation version; other renderers remain free to choose a different layout.

Click **Replay** for play/pause, ±10-second seeking, and **Next event** to skip quiet periods, with synchronized original audio. **Back to live** restores the latest scene. To open an archive read-only, including for a headless snapshot:

```sh
cargo run -- --replay /path/to/session.tacit
cargo run -- --replay /path/to/session.tacit --replay-at 12500 --snap /tmp/replay.png --size 1280x860
```

Scene replay reproduces the saved scene and its timed effects with this renderer version. The separate original-pixels track preserves the application image; OS cursor/hover, window chrome, and font/GPU differences across machines or future renderer versions are not pixel-perfect replay guarantees. Viewport metadata is retained; use the original window size for matching layout. The scene's map scroll is recorded, while playback controls are separate from recorded content.

Each committed event is flushed before becoming authoritative. A final partial line from an interrupted write is preserved separately and recovered on the next writable open; corruption in committed events, missing assets, or asset checksum failures are reported instead of silently substituted. Valid WAVs left by an interrupted recording are recovered on reopen and can be transcribed using **Retry transcript**. The log and media contain private thoughts and audio and are retained without automatic pruning; Unix archive folders are mode 700 and journal/exported asset files mode 600. Copy the entire `.tacit` directory to keep a replay self-contained. A session with no recordings still gets a valid empty WebVTT file.

## OpenRouter: quick themes, thoughtful review

Tacit uses the community [openrouter-rs](https://github.com/realmorrisliu/openrouter-rs) SDK and OpenRouter's strict JSON-schema output. Configure `OPENROUTER_API_KEY`, or store the key in a private file at `$XDG_CONFIG_HOME/tacit/openrouter.key` (default `~/.config/tacit/openrouter.key`, mode `600`). `TACIT_OPENROUTER_KEY_FILE` overrides the file location. Keys are never stored in notebook JSON or logged.

Run `cargo run` normally after configuring a key. OpenRouter takes priority over Ollama. Set `TACIT_ORGANIZER=offline` to disable AI, or `TACIT_ORGANIZER=ollama` to explicitly select the local model.

- **Immediate:** local draft placement; keeping a thought saves it immediately.
- **Fast pass:** after 180 ms without a committed edit, `google/gemini-3.1-flash-lite` creates themes and classifies thoughts. Once themes exist, only unclassified thoughts are sent with the theme list. Provider routing favors latency, with reasoning disabled and a 15-second deadline. Network feedback is not guaranteed to be instant.
- **Review:** after three seconds without a committed edit and completion of the fast pass, `anthropic/claude-sonnet-4.6` reviews the complete map, refines themes, and develops the narrative and tentative relationships. It has its own worker and a 90-second deadline.
- **Transitions:** thought labels spring into their new positions, colored branches fade back in, and a soft pulse marks ongoing organization. Rune's reduced-motion preference suppresses the fade and pulse.

Override models with `TACIT_FAST_MODEL` and `TACIT_REVIEW_MODEL`; choose models supporting structured output. Each pass validates IDs, theme references and complete assignment coverage before applying anything. Your wording and pinned placements remain intact, including pinned theme names. Theme IDs retain branch order. Old responses are discarded after capture, correction, removal or Undo. Each worker coalesces queued edits, and there are no automatic retry loops. Another committed edit retries after an error.

Only kept thoughts and theme metadata go to OpenRouter and its model providers; drafts and Hex audio stay out of these requests. Cloud calls use your OpenRouter credits. Automatic organization is bounded to 200 thoughts / 64 KiB of input per request; larger maps remain editable locally. Older notebooks load without migration.

A small live diagnostic sends only the built-in fictional demo, runs initial classification, one incremental classification, and the full review, then prints timing and validation counts:

```sh
cargo run -- --check-openrouter
```

## Local semantic organization

Without configuration, Tacit works entirely offline using simple, explicitly labeled classification rules. This fallback is not semantic AI.

For model-generated classifications, a session title, a narrative, and suggested relationships, run an Ollama server with an installed **local** model, then launch with that model's name:

```sh
TACIT_MODEL=your-installed-model cargo run
```

Tacit sends committed thoughts to `http://127.0.0.1:11434/api/generate`, after a 180 ms quiet period. Draft text is only previewed with local rules. The worker stays off the UI thread, coalesces pending edits, rejects stale results, validates references, and respects pinned classifications. Model-generated links appear beneath their source thoughts and in the margin; select a link to inspect its target. All model interpretations are suggestions.

A missing server, unknown model, timeout, or invalid response leaves thought capture usable and reports the problem in the status line. Another edit requests organization again. No model is downloaded or server started automatically. The Ollama backend only calls loopback; use an actual local model if you want all inference to stay on your machine.

## Storage

The default session is `$XDG_DATA_HOME/tacit/session.json`, or `~/.local/share/tacit/session.json` when XDG_DATA_HOME is unset. Override it with `TACIT_SESSION=/path/to/session.json` to keep separate notebooks. Captures and corrections save automatically using a synced temporary file and atomic rename. A file lock prevents two app instances from overwriting the same session. Errors remain visible, and malformed sessions are not silently overwritten.

Original text, stable IDs, classifications, pinned placements, and the latest model interpretation are persisted. Unsubmitted drafts and undo history are currently in memory only. Sessions are plain JSON, not encrypted.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
# Synthetic archive for offline playback QA; output directory must be new:
cargo run --example replay_fixture -- /tmp/tacit-replay-fixture
```

Rune is pinned to a specific Git revision, and Cargo.lock is included with the project sources. The build and headless renderer have been tested on macOS. Linux uses Rune's Wayland/X11 backend and needs the associated native development libraries (including Wayland and xkbcommon); Linux execution has not yet been verified here.

Rune's headless tools use the same view and input runtime as the desktop app:

```sh
cargo run -- --demo --snap /tmp/tacit.png --size 1280x860
cargo run -- --demo --snap /tmp/tacit-small.png --size 820x640
cargo run -- --scene themes --snap /tmp/tacit-themes.png --size 1280x860
```

Headless runs do not read or write your real notebook or contact OpenRouter, Ollama, or Hex. Tests exercise real text entry, selection, reclassification, deletion, undo, storage, locking, organizer validation, and stale-result handling.

## Structure and current scope

- `src/art.rs`: offline motifs, hand-drawn strokes, coalesced illustration transitions.
- `src/generated_art.rs`: image model requests, stable scene briefs, bounded private cache, replacement transitions.
- `src/model.rs`: original thoughts, narrative roles, validated model interpretations.
- `src/capture.rs`: original render-texture capture and exact presentation.
- `src/video_archive.rs`: lossless PNG masters, timestamped FFV1/PCM video export.
- `src/timeline.rs`: durable event log, shared projection, media integrity, WebVTT, and recovery.
- `src/session_event.rs`: portable content events and the pure reducer.
- `src/presentation.rs`: Rune presentation and version 1 scene compatibility adapter.
- `src/storage.rs`: session loading, atomic saves, exclusive session lock.
- `src/organizer.rs`: independent OpenRouter fast/review workers, Ollama fallback, and response validation.
- `src/voice.rs`: Rune WAV capture, authenticated Hex transcription streams, and exact sample uploads.
- `src/ui.rs`: Rune canvas, vector illustrations, input, inspector, and narrative view.
- `design/living-map.html`: the earlier conceptual exploration, independent of the Rust app.

This is a first working vertical slice. The tree uses up to eight thematic branches (four narrative lenses offline) and scrolls as it grows; it is not yet an infinite spatial graph. Nested subthemes, drag positioning, relationship rejection, and SVG/PDF export remain future work. The local-model integration is tested with fixtures; model quality and live inference still need validation with an installed model.

## Multiple sessions

Use **Sessions** in the header to open your session library. Enter a name and choose **New session** to start an independent mindmap, or **Rename current** to rename the open session. Select a session to resume it. The original notebook remains available as **Original session**; names never become filesystem paths, and duplicate names are allowed.

Each session has its own event log, audio, WebVTT, artwork, original frames, draft, and replay history. Switching saves the draft and closes the current capture cleanly. Recording, transcription/chunking, and video export must finish before switching. Late organization and image results stay isolated in the old workers. A session open in another process cannot be opened for editing.

The library lives beside the existing notebook in `session.sessions/`, with one directory per session. `TACIT_SESSION` selects the original notebook and therefore its adjacent library. Startup opens that original notebook; use the picker to reopen another session. Library browsing pauses original-frame capture, with explicit capture boundaries; the library itself is outside the recorded mindmap scene.

## Brainstorm and Direct the map

The composer has two explicit modes, each with its own draft. **Brainstorm** captures ideas as thoughts. **Direct the map** treats typed text or speech as an editing instruction. Select the mode before pressing Listen; it cannot change during a recording. In direction mode, **Stop & direct** transcribes the instruction and asks the stronger model for a bounded edit plan.

Try “Rename Costs to Infrastructure costs,” “Move the hosting thought into Costs,” “Merge Meeting overhead into Coordination,” or “Put Decisions before Costs.” Directions can create, rename, merge, and order branches, move or order existing thoughts, and rename the central theme. They cannot rewrite/delete original thoughts or set arbitrary pixel coordinates, fonts, or styles. Ambiguous and unsupported requests return an explanation without editing. Current tree slots determine the positions after branch reordering.

Directions are validated and applied atomically through the shared reducer, support Undo, and are captured in the event log. Explicit thought moves are pinned. Stale plans never apply to changed content. The latest twenty applied instructions inform subsequent cloud classification; existing branch order survives normal organization. Spoken instructions keep their original audio and WebVTT but never enter the thought list or semantic splitting queue. Failed instructions stay in the direction draft for retry. Direction mode requires OpenRouter; it does not silently fall back to brainstorming.

Segmentation now uses the stronger review model and a contextual editorial pass that distinguishes independent proposals, attaches examples/qualifications to their parent idea, follows topic switches and returns, and preserves every original word. The fast classifier still gives quick placement feedback; the whole-map reviewer now enables reasoning and audits conceptual coherence, competing options, unsupported relationships, and unnecessary movement. The full recording remains visible while stronger segmentation runs. Quality is model-dependent; the synthetic evaluation is a regression check, not a guarantee of expert judgment on every discussion.

Run the opt-in live evaluation with synthetic discussions (uses configured OpenRouter credits):

```sh
cargo run --example facilitation_eval
```
