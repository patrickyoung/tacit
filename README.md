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

The visual vocabulary is no longer limited to the twelve fallback icons: generated scenes can combine concepts from the thoughts. Raster artwork reveals as a soft wash; the fallback vectors retain their stroke-by-stroke animation. Style selection is currently per app session.

To verify the complete image-generation path with one fictional demo theme (a paid request unless cached):

```sh
cargo run -- --check-image
cargo run -- --scene generated --snap /tmp/tacit-generated.png --size 1280x860
```

Normal headless snapshots never contact the network; the generated demo scene loads only existing cache entries. The explicit `--check-image` diagnostic validates the PNG by decoding it before caching.

## Voice with Hex

Open your installed Hex desktop app with its microphone permission granted and a model ready. Tacit connects automatically on launch; no microphone is opened until you click **Listen**.

1. Click **Listen** and speak. The red recording indicator and elapsed timer stay visible.
2. Click **Stop & transcribe**. Hex returns the raw transcript directly to Tacit without changing the clipboard or pasting elsewhere.
3. The transcript is saved immediately. With OpenRouter configured, Tacit separates it into coherent thoughts, then categorizes them using the surrounding discussion. Your typed draft stays untouched.

Splitting follows ideas and topic changes, keeping explanations and dependent references together. The model selects boundaries in the original words; it cannot rewrite, omit or reorder them. The fast classifier receives the discussion context, and the stronger review reconciles the whole map afterward. **Undo** can restore the unsplit recording; undoing while a split is pending cancels its application. If splitting fails or OpenRouter is unavailable, the full transcript remains on the map. Transcription stays local in Hex; semantic splitting and classification send text to OpenRouter. Splitting happens after Stop, not while the microphone is recording.

**Cancel** or **Escape** while recording discards that recording and preserves your draft. While Hex is transcribing, Tacit remains usable. A capture is owned by Tacit; it refuses to start while Hex is recording elsewhere. Authentication and ownership tokens stay in memory and are never written into the notebook or logs. The background worker sends lease heartbeats and attempts to cancel an unfinished capture on shutdown; Hex expires abandoned recordings after ten seconds without renewal.

Use **Reconnect Hex** after starting/restarting Hex or resolving an error. A read-only diagnostic is available:

```sh
cargo run -- --check-hex
# Optional live check with a fictional multi-topic transcript (uses OpenRouter):
cargo run -- --check-recording
```

This first integration uses Hex's desktop capture API (macOS and Linux), which returns the transcript after Stop. It does **not** stream partial words yet. Windows desktop capture is currently unsupported by Hex and is reported as unavailable. Headless snapshots and automated tests never connect to your installed Hex or start its microphone.

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
- `src/storage.rs`: session loading, atomic saves, exclusive session lock.
- `src/organizer.rs`: independent OpenRouter fast/review workers, Ollama fallback, and response validation.
- `src/voice.rs`: authenticated Hex discovery, owned recording lifecycle, and lease management.
- `src/ui.rs`: Rune canvas, vector illustrations, input, inspector, and narrative view.
- `design/living-map.html`: the earlier conceptual exploration, independent of the Rust app.

This is a first working vertical slice. The tree uses up to eight thematic branches (four narrative lenses offline) and scrolls as it grows; it is not yet an infinite spatial graph. Nested subthemes, drag positioning, relationship rejection, session browsing, and SVG/PDF export remain future work. The local-model integration is tested with fixtures; model quality and live inference still need validation with an installed model.
