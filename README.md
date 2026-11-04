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

Type a thought and watch its tentative placement update immediately. Press **Enter** to keep it; **Shift+Enter** adds a newline. Select a thought to inspect its original words, move it to the next lens, or remove it. **Undo** reverses the last capture, move, or removal. **Read the thread** switches to a narrative view. Tab navigates controls. Cmd+S (Ctrl+S on Linux) retries saving, and Escape clears selection.

The map uses four stable narrative lenses: observations, questions, tensions, and possibilities. Moving a thought pins its classification so the organizer cannot override it. Long regions scroll; smaller windows move the inspector controls beneath the map. The curved arrows between regions indicate reading order, not causal claims.

## Local semantic organization

Without configuration, Tacit works entirely offline using simple, explicitly labeled classification rules. This fallback is not semantic AI.

For model-generated classifications, a session title, a narrative, and suggested relationships, run an Ollama server with an installed **local** model, then launch with that model's name:

```sh
TACIT_MODEL=your-installed-model cargo run
```

Tacit sends committed thoughts to `http://127.0.0.1:11434/api/generate`, after an 800 ms quiet period. Draft text is only previewed with local rules. The worker stays off the UI thread, coalesces pending edits, rejects stale results, validates references, and respects pinned classifications. Model-generated links appear beneath their source thoughts and in the margin; select a link to inspect its target. All model interpretations are suggestions.

A missing server, unknown model, timeout, or invalid response leaves thought capture usable and reports the problem in the status line. Another edit requests organization again. No model is downloaded or server started automatically. Tacit only calls loopback; use an actual local model if you want all inference to stay on your machine.

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
```

Headless runs do not read or write your real notebook or contact Ollama. Tests exercise real text entry, selection, reclassification, deletion, undo, storage, locking, organizer validation, and stale-result handling.

## Structure and current scope

- `src/model.rs`: original thoughts, narrative roles, validated model interpretations.
- `src/storage.rs`: session loading, atomic saves, exclusive session lock.
- `src/organizer.rs`: background Ollama worker and response validation.
- `src/ui.rs`: Rune canvas, vector illustrations, input, inspector, and narrative view.
- `design/living-map.html`: the earlier conceptual exploration, independent of the Rust app.

This is a first working vertical slice. The canvas uses four fixed regions, not an infinite spatial graph. Freeform thematic clusters, custom illustrations per subject, drag positioning, relationship rejection, session browsing, and SVG/PDF export remain future work. The local-model integration is tested with fixtures; model quality and live inference still need validation with an installed model.
