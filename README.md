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

Type a thought and watch its tentative placement update immediately. Press **Enter** to keep it; **Shift+Enter** adds a newline. Select a thought to inspect its original words, move it to the next branch, park it beside the map, or remove it. **Undo** reverses the last capture, move, parking, or removal. **Read the thread** switches to a narrative view. Tab navigates controls. Cmd+S (Ctrl+S on Linux) retries saving, and Escape clears selection.

The map grows tapered curved branches from the central idea, with short thought labels to either side; a single ink mural can sit beneath its title once you ask for one, and the branches then bud from either side of the picture. Select a thought to read its complete original wording. With OpenRouter configured, the **map agent** files each thought as you speak, grows three to seven named main branches around one ordering idea, combines repeats, and parks what is off topic or not yet ready beside the map. Offline, four narrative lenses provide a starting point: observations, questions, tensions, and possibilities. Moving a thought pins it so the agent cannot move it again. Long regions scroll; a selected thought's details open in the dock beneath the map. Colored branches indicate classification, not causal claims.

## Artwork when you ask for it

Tacit can draw the conversation as **one mural** in the center of the canvas, and only when you ask. **Create artwork** in the header composes the first scene whenever you choose: after a handful of ideas, or once the conversation is finished. Until then no artist or image model is called. `openai/gpt-image-2.5-flare` runs at **high** quality for both the first image and subsequent edits, producing 1536 × 1024 transparent PNGs. The title sits above the landscape artwork; there is no circular frame, central opening, or per-topic illustration.

A dedicated **artist** pass (`openai/gpt-6.1-terra`) reads the committed messages, source evidence, concepts, map narrative and tentative connections, alongside the previous scene and an image preview composited onto the light app canvas. Hidden RGB beneath transparent pixels is excluded from visual review; editing and archives still use the original RGBA pixels. It states the story the viewer should recognize and connects 1–4 visible actions or relationships to validated thought IDs. The subject determines the setting and whether people are useful; a system, product or process need not become another group around a desk. Proposed solutions remain exploratory. The artist chooses **edit** for a sound composition or **redraw** when the old scene is generic, repetitive or cluttered. Edits use [OpenRouter's reference-image input](https://openrouter.ai/docs/guides/overview/multimodal/image-generation); redraws start fresh while retaining the archive's revision history. This process cannot alter the messages or map.

Every session has an independent, stable visual identity with varied composition and fictional casting cues. These are illustration choices, not inferred attributes of the speakers. Explicit source descriptions take precedence, and useful characters stay consistent within their own session. Sparse editorial brush-pen contours, clear silhouettes, selective detail and a small indigo or terracotta accent keep the scene readable at canvas scale. Unmarked areas stay transparent. **Hide artwork** puts the mural out of sight and **Show artwork** brings it back.

Once there is a mural, **Update artwork** appears whenever the conversation holds something it does not show: new, corrected or removed ideas, a changed narrative or changed relationships. Cosmetic changes such as a title, branch name, ordering or pin leave the mural current, so no update is offered. A separate rendering-policy fingerprint marks murals drawn before a prompt or quality improvement as behind too, so the next update brings them forward. Historical replay retains its exact original images.

Each press composes one scene from the conversation as it stands. Nothing is composed in the background, on a timer, or as you speak. The header shows **Composing artwork…** while the request is out; network time varies, and a press made while the map is still being organized waits for it to settle. Completed scenes crossfade over six seconds; reduced motion shows them immediately. The artist receives up to 128 KiB of context; larger conversations retain their previous artwork. You can keep talking while a scene is composed: new ideas leave the finished scene one update behind, while a correction or deletion discards the scene under way and composes it again from the corrected conversation. No requests run during replay. Without cloud image generation nothing offers to create artwork: the canvas retains its text and branches, archived murals still show, and there are no separate fallback illustrations.

The session archive saves each image, its story, source connections, scene description, continuity anchors, visual identity, content and rendering-policy fingerprints, and parent revision. Reopening resumes from these archived pixels even if the image cache was cleared. Older mural archives still load; the first update you ask for revisits the scene using the current narrative direction. Image cache identities include the session identity, conversation content, parent pixels, prompt, model and output settings; private PNGs live under `$XDG_CACHE_HOME/tacit/art` (default `~/.cache/tacit/art`), bounded to 128 PNGs / 256 MiB. Private prompts are stored only in the session archive, not the image cache. The diagnostic's single `demo-mural.json` manifest contains only the built-in fictional example.

Generation and art direction use your existing OpenRouter key and credits, and only when you press **Create artwork**, **Update artwork** or **Retry art**. `TACIT_IMAGE_ART=off` removes the option altogether. `TACIT_IMAGE_MODEL` overrides the single image model (which must support high-quality landscape transparent PNGs and image references); `TACIT_ARTIST_MODEL` overrides the artist (which must support vision and structured output). `TACIT_CENTRAL_IMAGE_MODEL` is no longer used. **Hide artwork** also cancels a scene that is being composed. An in-flight call may finish into the cache, but cannot replace the current mural after cancellation. Failures preserve existing artwork and are reported in a notice above the status line, where **Retry art** asks again without changing your thoughts and **Dismiss** puts the message away. There is no automatic retry loop.

A live diagnostic sends only the fictional demo through the artist and image model and saves a preview for `--scene generated`:

```sh
cargo run -- --check-image
cargo run -- --scene generated --snap /tmp/tacit-generated.png --size 1280x860 --at 1
```

Normal headless snapshots never contact the network; the generated demo scene loads only existing cache entries. The explicit `--check-image` diagnostic validates the PNG by decoding it before caching.

## Voice with Hex

Open Hex with a local model loaded and timed transcription streams enabled. Tacit records **one microphone** through Rune into a 16 kHz mono PCM WAV, streams those same samples to Hex locally while recording. Recording currently requires macOS because Rune's native recorder backend is macOS-only. Hex's stream endpoint is cross-platform; a native recorder backend is still needed for other platforms.

1. Click **Start recording** in the voice dock. macOS may ask for microphone permission for Tacit or its launching terminal. The recording indicator and timer stay visible.
2. Speak naturally. Provisional words appear as a live preview. Each finalized speech window becomes a source thought immediately. The listener cuts it into the separate ideas it holds, the placer names each one and files or parks it within about a second, and the steward steps back to refine the whole map when the talk pauses. Your typed draft stays untouched.
3. Click **Finish recording** to drain the stream and save the complete transcript and WebVTT. Hex supplies audio-window timestamps, not word timings. Finalization happens at pauses or window boundaries (up to roughly 25 seconds), so concepts do not appear on every spoken word; partial previews depend on the Hex model/device.

In Brainstorm mode, **Escape** finishes and keeps the recording because live concepts may already have been saved. In **Direct the map** mode, speech remains a preview until **Stop & direct**; **Cancel** or **Escape** discards that pending instruction. Finalized speech windows and their source links are durable events, so reconnecting does not duplicate thoughts and replay preserves incremental map changes.

The original WAV is retained even if live transcription fails. Tacit falls back to transcribing saved audio at Stop. If that transcript differs from already committed live windows, Tacit retains the live concepts and archives the complete transcript without blindly duplicating it; a status message identifies the discrepancy. **Retry transcript** retries retained audio without opening the microphone. **Reconnect Hex** checks the endpoint. The WAV owned by Tacit remains authoritative; this integration does not use Hex's potentially lossy dictation audio mirror.

Transcription stays local in Hex. Semantic splitting, the map agent, and illustration still use OpenRouter when configured. Headless snapshots never open a microphone or automatically call either service. Diagnostic commands:

```sh
cargo run -- --check-hex
cargo run -- --check-audio /path/to/16000hz-mono-pcm16.wav
cargo run -- --check-recording # fictional example: split, mapped and parked; uses OpenRouter
```

## Dropping files onto the map

Drag files from Finder onto the map. Each one is kept whole in the session's archive, read into text, and then treated like anything you say: captured as one thought, cut into separate thoughts by the listener, and placed by the map agent. Several files are read one after another; the header shows which one is being read, and sessions cannot be switched until they are done.

| Dropped | Read by | Leaves this Mac |
| --- | --- | --- |
| Text, Markdown | read as written | no |
| Word, RTF, OpenDocument, HTML, web archives | `textutil` | no |
| PDF with a text layer | PDFKit | no |
| Scanned PDF (up to 20 MB) | the reader model | the PDF |
| Images (PNG, JPEG, HEIC, GIF, WebP, TIFF, BMP) | the reader model | a JPEG no larger than 2048 px |
| Audio (MP3, M4A, WAV, AAC, FLAC, Ogg, Opus, AIFF, CAF) | FFmpeg, then Hex in five-minute parts | no |
| Video (MP4, MOV, M4V, WebM, MKV, AVI) | its speech as audio; with no speech, six frames by the reader model | only the frames of a video without speech |

Text of up to 3,000 words goes onto the map as written: the listener hears it in pieces of about 500 words and cuts each into thoughts, keeping every word, as it does with speech. A longer text is first told in brief by the reader model, in up to 40 stand-alone statements, so that one long document does not fill the map; the full text stays in the journal and its opening is shown in the transcript under the file's name. The reader is told that a file is material to read and never an instruction.

Without an OpenRouter key, the kinds read on this Mac still work for texts of up to 3,000 words, each arriving as one thought; images, scans, videos without speech and longer texts say that they need OpenRouter. A file that could not be read stays in the archive, and **Read again** retries it from there, so the original can be moved or deleted as soon as it is dropped. The words of a long recording are saved before they are told in brief, so a failed request after an hour of transcription costs only the request. Files larger than 2 GB, and kinds Tacit does not know, are refused by name. Audio and video need FFmpeg (`brew install ffmpeg`, or set `TACIT_FFMPEG`) and a running Hex; documents, PDFs and images use tools that ship with macOS.

One file at a time can be checked from the command line. It prints what would go onto the map and changes no session:

```sh
cargo run -- --check-file /path/to/file          # uses OpenRouter where the table says so
cargo run -- --check-file /path/to/file --local  # never leaves this Mac
```

## The knowledge base

A dragged file has a second place to land. While files are over the window, the map shows two targets: **Drop to add to this map**, as above, and **Drop to file in the knowledge base**. A file dropped on the second is not turned into thoughts. It is kept, read the same way (the table above applies), and filed by the librarian as a zettelkasten: small notes of one idea each, fitted into what the knowledge base already holds. Nothing is added to the map. The header shows which file is being filed and how far along it is, and afterwards a button with the number of notes, which opens them to read (**Reading the knowledge base**, below).

### How the librarian works

The librarian works the way someone practised at a zettelkasten does, in three roles. Each is a turn of the librarian model with its own instructions, and code checks what comes back.

1. **The reader** goes through the whole file first (a long one by how each part opens) and writes a note on the source: what the file is, how it runs, and the topic it belongs to. Every part is then filed knowing where it sits.
2. **The writer** takes the file a part at a time, about 1,300 words each. It lists the ideas in the part and decides for each what the knowledge base already holds:
   - **new**: no note holds it, so a note is written;
   - **same**: a note already says it, so none is written, and the source note records that this file says it too;
   - **adds**: a note holds it and the file brings a reason, a case or a number, so a note of only what is added is written and linked to it;
   - **disputes**: the file contradicts a note, so a note states the file's position and is linked as a contradiction. Disagreements are kept, never smoothed over.

   When a one-line card does not settle it, the writer opens the notes in full before it decides. A note holds one idea, has a title that states the idea as a claim, can be read without the file, and gives a reason for every link. Code checks each answer against that (a title that only names a subject, a text that says "the author", a text too long for one idea or too thin to be one, a link with no reason) and sends it back once, saying what fell short.
3. **The curator** then goes over the topics where the new notes met notes held before, or that have grown to four notes. It merges notes that hold the same idea, rewrites a note that does not stand alone, moves notes to the topic they belong in, links notes across topics, and writes the topic's **overview**: a short way in that says what the rest stands on, which note is the evidence for which, and where two notes disagree.

Three things keep your notes safe from it:

- A note you wrote or edited is yours. The librarian links to it and never rewrites, moves or removes it. It tells the two apart by the `seal` line it puts in the pages it writes: change such a page, or delete the line, and the page is yours from then on.
- Every page the librarian replaces or takes out is kept in `history/`.
- `changes.md` says in plain lines what it did: what each file was filed as, every merge, rewrite, move, link and overview, and what the filing cost.

There is one knowledge base for the whole library of sessions. It is a folder beside the notebook, `session.knowledge/` next to `session.json`:

- `notes/`: one Markdown file per note, with a short front matter (`id`, `title`, `topic`, `keywords`, `source`, `kind` for a note on a source or an overview, `seal`), the gist as a quoted line under the title, the text, and a `See also:` list of `[[links]]`, each with its reason. Markdown notes you put there yourself are read as notes when the app starts.
- `sources/`: each filed file, kept whole, and beside it the text read from it, so a filing that failed is not read or transcribed twice.
- `history/`: the pages the librarian replaced or took out, named by the note and the time.
- `changes.md`: what the librarian did, newest last.

`TACIT_KNOWLEDGE=/path/to/folder` keeps it somewhere else, and `TACIT_KNOWLEDGE=off` removes the second target and everything below.

### How the map agent reads it

The knowledge base informs how a map is organised. It is never content: no note is put on a map, and the agents are told that a note is something the user knows, not something they said in this session. It reaches them a level at a time, so that a large knowledge base costs a turn little:

1. **The index**: the topics, how many notes each holds, whether it has an overview, and the keywords its notes share. The steward and a directed turn always see it.
2. **Overviews**: for up to two of the topics the map falls in, the overview the curator wrote, so that a turn sees how a subject is built before it reads single notes. The steward and a directed turn see them.
3. **Cards**: one line of a note (id, title, topic, gist, and the ids it links to) for the notes nearest the map. The placer sees up to five, chosen by what the thoughts it is filing say, and nothing else, so it stays as quick as before. The steward and a directed turn see up to ten, chosen by the whole map and the instruction, and the notes those link to. Only notes of an idea have a card here; a note on a source or an overview is reached through them.
4. **Whole notes**: the steward and a directed turn may answer by naming notes or topics to open. Nothing in that answer is applied; they are asked again with the notes in full, each with the notes it is linked with and why (or a topic's overview and cards), at most twice in a turn.

Nearness is worked out on this Mac from the words a note is called by, with no model call. With no knowledge base, or an empty one, every prompt and brief is exactly what it was.

Filing needs OpenRouter. What a filed file says goes to the librarian model: how each part opens to the reader, then each part to the writer together with the index and the cards of notes near it, and the cards of a tended topic to the curator, with any notes they open. A short file takes three to five model calls. Without a key the file is kept in `sources/` and nothing is written; drop it again once a key is configured. The overviews, cards and opened notes shown to the map agents go to OpenRouter with the rest of a turn. A file that is already in the knowledge base is refused by name.

A knowledge base can be filled and looked at from the command line, without opening a session: it prints the index, every card, the overviews and `changes.md`. With `--say`, the thoughts are then mapped with the notes in hand, by the placer and the steward, and with `--ask` a directed turn carries out an instruction on that map. The notes a turn opened are listed under it:

```sh
cargo run -- --check-knowledge /path/to/folder file.pdf notes.md
cargo run -- --check-knowledge /path/to/folder --say "Agendas arrive late." --say "No one keeps time." \
  --ask "Arrange this the way my handbook says meetings should run."
```

### Reading the knowledge base

The button with the number of notes opens the knowledge room in the map's place; **Back to map** returns. The room is laid out for the way someone practised at a zettelkasten reads one, which is a handful of moves:

- **Come in from the top.** The way in lists the topics, each with its overview in a sentence, the files the notes were taken from, and what was filed last. A topic opens on its overview, and lists its notes in the order the overview takes them.
- **Read one note whole.** Its claim, its text, its keywords and the day it was filed. A page you wrote or changed yourself is marked **Yours**.
- **Follow a link for its reason.** Under the note are the notes it leads to and the notes that lead to it, each with why it is linked. A note that contradicts it is marked **Disagrees**, in red, in whichever direction the link runs.
- **Go back to the source.** A note names the file it was taken from, and the file's own note lists every idea taken from it.
- **Look for a word.** Type in the field and press Enter (or **Find**): the notes nearest those words are listed, nearest first.
- **Retrace your steps.** **Back**, or Escape, goes to where you were reading, and from the way in back to the map. The room is as you left it when you open it again.

**Ask** hands the question in the field to the **researcher**, an agent that goes through the notes with the same moves. It starts from the question, comes in through the overviews of the topics the question falls in, searches in the notes' words, opens notes in full, follows links by their reasons, and answers only from what it read. Code checks the answer against the reading:

- every note the answer rests on was opened in full; one it had only seen the card of is opened and the answer asked for again, once, and after that the note is left out;
- a note the answer rests on that another note contradicts is never taken alone: the other is opened too, and the disagreement is shown whether or not the answer mentions it.

The answer page shows the reply with numbered references, the notes it **rests on**, where **the notes disagree**, what is **not in the notes**, what to **read next**, **how it was found** (each search and each note read, which you can follow), and what it cost. The researcher is told never to fill a gap from what it knows itself; asked about something the notes do not hold, it says so and lists what it searched for.

Looking, reading and following links happen on this Mac with no model call. Only **Ask** uses OpenRouter: the question goes to the researcher model with the index, the overviews and cards near the question, and every note it opens on the way. A question takes two to seven model calls; the two tried on a small synthetic knowledge base took two and three calls, 7 to 15 seconds, and about half a cent or less. The researcher uses the librarian's model and effort. Without a key, **Ask** says so and the rest of the room works. What you read in the room is no part of the session: nothing there reaches a map, and the room is left out of the session's recording, as the sessions drawer is. A session that is open read-only can still read.

A question can be asked from the command line. It prints the answer, what it rests on, disagreements, gaps, the way it was found and the cost:

```sh
cargo run -- --ask-knowledge /path/to/folder "How many people should be in a meeting?"
```

## Saved sessions and replay

Beside `session.json`, Tacit creates a private `session.tacit/` archive:

- `events.jsonl`: versioned, ordered events with millisecond session timestamps, sequence numbers, and a BLAKE3 integrity chain. Content events capture thoughts, corrections, deletions, semantic splits, the map agent's edits (placements, restructuring, combined repeats, narrative and links), parking decisions, map guidance, undo, transcript links, dropped files (each one's name, kind and asset, the text read from it, and the thought it became), and available artwork. A separate, versioned presentation track stores Rune draft, selection, art style, vector drawing state, image transitions, scrolling, and viewport metadata. Recording, transcript, and failure events retain their original order.
- `recordings/`: original captured WAVs, including stopped recordings whose transcription is pending or failed.
- `media/`: content-addressed, durable audio, generated PNG assets and dropped files, plus per-recording `.vtt` files. These assets are independent of the bounded image-generation cache.
- `transcript.vtt`: captions on the session timeline, including gaps between recordings. Per-recording VTT uses time relative to its WAV. Caption boundaries are Hex's actual transcription windows, not inferred boundaries for the semantic thought chunks.

The event log is authoritative; `session.json` remains a compatibility snapshot. Existing notebooks are imported as the initial event without inventing past history. Live content follows **action → event → durable append → reducer projection → renderer**. Captures, corrections, deletion, splitting, model results, and undo cannot publish content before their event is saved; failed writes keep the previous projection. Replay uses the same pure reducer. Rune reads the shared projection for content and manages its own presentation controls and animations. Model results and images are replayed from saved data; no model calls occur during playback. Stroke reveals, image crossfades, and classification fades use recorded time; hidden Rune layout springs have been removed so seeking does not depend on prior animation frames.

### Original pixels and exact replay

Persistent sessions now capture the actual Tacit render texture before presenting it to the window. Each frame is stored as a lossless, content-addressed RGBA PNG with its timestamp, device-pixel dimensions, display scale, and content revision. This records Tacit's application pixels, including its controls and draft; it does not record other applications or the OS desktop. Headless demos only capture when explicitly launched with `--capture-demo NEW_PATH`.

**Play video**, beside the session status, starts the original recording with synchronized audio. Playback also offers **Scene replay**, which uses the recorded presentation's content revision and sampled animation clock; it does not show later content before the frame that displayed it. Related transcript/capture/link events and artwork/presentation events commit atomically. Recorded map dimensions prevent replay controls or a different window width from rearranging branches. Unsupported or missing presentation tracks fail strict replay instead of silently generating a new scene. Older archives without original pixels offer **Play session** for scene playback; their missing video cannot be recovered retroactively.

**Save video** is available beside **Play video** and in playback. It opens a native save dialog with a session-based filename and writes an `.mkv` with lossless FFV1 video and the original PCM audio, aligned on the same timeline. Cancelling the dialog leaves the live session running. Choosing a destination pauses live capture and opens playback until the export finishes; **Back to live** then resumes the session. The saved file can be revealed with **Show in Finder** (or **Show in folder**). Existing files are never overwritten. Video starts at its first captured frame; the `tacit_session_start_ms` container tag records its offset into the session/WebVTT timeline. Window resizing adds black padding without scaling or changing the original frame pixels. Capture stops/restarts are explicit; intervals when original capture is paused are black. An interrupted capture is reported, and export refuses to claim completeness for an archive with recorded capture failures. The PNG masters and event log remain authoritative. FFmpeg is required for export (`brew install ffmpeg` on macOS, or set `TACIT_FFMPEG`); recording and in-app original-pixel playback do not require it.

```sh
cargo run -- --replay /path/to/session.tacit --replay-pixels
cargo run -- --export-video /path/to/session.tacit /path/to/original.mkv
```

Original capture uses synchronous GPU readback and durable writes, prioritizing retaining every application frame over throughput. Each frame's presentation and pixels commit together in one journal transaction; capture avoids an additional presentation save during UI polling. PNG compression and checksums, and Tacit's pixel conversion, are optimized in developer builds too. Capture still adds rendering latency and retains potentially substantial media; no automatic pruning is performed. It records the frames Tacit presents, not a promised fixed frame rate. Frame timestamps have millisecond resolution. Scene replay remains renderer/version dependent; original-pixel playback and lossless video preserve the visual record independently of future renderer changes.

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

Click **Play video** (or **Play session** in an older archive) for play/pause, ±10-second seeking, and **Next event** to skip quiet periods, with synchronized original audio. Finish recording, transcription or map guidance before opening playback or saving a video. **Back to live** restores the latest scene. To open an archive read-only, including for a headless snapshot:

```sh
cargo run -- --replay /path/to/session.tacit
cargo run -- --replay /path/to/session.tacit --replay-at 12500 --snap /tmp/replay.png --size 1280x860
```

Scene replay reproduces the saved scene and its timed effects with this renderer version. The separate original-pixels track preserves the application image; OS cursor/hover, window chrome, and font/GPU differences across machines or future renderer versions are not pixel-perfect replay guarantees. Viewport metadata is retained; use the original window size for matching layout. The scene's map scroll is recorded, while playback controls are separate from recorded content.

Each committed event is flushed before becoming authoritative. A final partial line from an interrupted write is preserved separately and recovered on the next writable open; corruption in committed events, missing assets, or asset checksum failures are reported instead of silently substituted. Valid WAVs left by an interrupted recording are recovered on reopen and can be transcribed using **Retry transcript**. The log and media contain private thoughts and audio and are retained without automatic pruning; Unix archive folders are mode 700 and journal/exported asset files mode 600. Copy the entire `.tacit` directory to keep a replay self-contained. A session with no recordings still gets a valid empty WebVTT file.

## The map agent

Tacit uses the community [openrouter-rs](https://github.com/realmorrisliu/openrouter-rs) SDK and OpenRouter's strict JSON-schema output. Configure `OPENROUTER_API_KEY`, or store the key in a private file at `$XDG_CONFIG_HOME/tacit/openrouter.key` (default `~/.config/tacit/openrouter.key`, mode `600`). `TACIT_OPENROUTER_KEY_FILE` overrides the file location. Keys are never stored in notebook JSON or logged.

Run `cargo run` normally after configuring a key. OpenRouter takes priority over Ollama. Set `TACIT_ORGANIZER=offline` to disable AI, or `TACIT_ORGANIZER=ollama` to explicitly select the local model.

With a key, one **map agent** is the only editor of the live map besides you. It works the way a facilitator works a wall of notes, in three kinds of turn that share one vocabulary of edits:

- **Placer:** the moment a thought waits, a small fast model gives it a first home: an existing branch, a new branch when the map has none for it, or the parking lot. It distills each thought into a concept phrase and takes up to twelve new thoughts a turn. It may grow the map; it does not restructure it.
- **Steward:** when the talk pauses or the map strains, a stronger model steps back and reads the whole map against the words behind every label. It regroups, renames, merges and reorders branches, corrects labels, combines thoughts that say the same thing, revisits the parking lot, draws tentative links, and rewrites the focus, ordering idea and narrative.
- **Directed turn:** what you ask of the map in **Direct the map** is carried out with your authority (see **Brainstorm and Direct the map**).

A turn returns edits, never a whole map: grow, rename, move, merge, remove or reorder branches; place, pin or park a thought; combine repeats; set the links, title, narrative, focus and ordering idea. Edits are tried one at a time against the map as it is when the answer arrives, so a turn that began before you said more is rebased instead of thrown away, and one bad edit costs only itself. Before answering for good, the model is shown what its own edits left undone (a thought still without a branch, an edit the map refused, a strain that remains) and may correct it: two calls at most for the placer, three for the steward and a directed turn. What survives is saved as one `Mapped` event (`Guided` for a directed turn) holding the ordered edits, and replay applies them without calling a model.

### What the agent knows about mind maps

The prompts, and the rules every edit is checked against, hold the agent to what a seasoned mapper does by habit:

- **One central idea, one ordering idea.** The steward states what the whole map is about and the single principle that divides it (aspects, stages, stakeholders, causes, options…). It finds that principle bottom-up, as in affinity mapping: group the thoughts by what they are about, then name the groups. When the principle is a sequence, the main branches read in that order.
- **Three to seven main branches.** The agent never leaves a map wider than seven; an eighth main branch exists only on your word. Sibling branches are the same kind of thing at the same level, do not overlap, and between them cover every thought.
- **Chunking.** When a subject has more parts of one kind than a map has room for, the agent gathers them into two to four families, each a main branch named for what its members share. It counts the parts a speaker has announced and not yet reached (“there are seven basic plots…”) and chunks in one move, as soon as the count is clear. A main branch for every part is yours to ask for.
- **General to specific, at most three levels.** A branch reads at a glance with up to seven items beneath it; past that it is divided along a real distinction. A sub-branch earns its place with two or more thoughts. No branch exists only to wrap another, and no main branch holds most of the map.
- **Keywords.** Branch names are noun phrases of one to three words in sentence case, parallel with their siblings. Thought labels are two to six words that name the idea instead of clipping the sentence, keep negation and uncertainty, and mark a suggestion as *Proposed* or *Possible* so it never reads as settled.
- **No catch-alls.** No Other, Misc or General, and no branch per kind of thought such as Questions or Ideas.
- **Say it once.** Thoughts that state the same idea are combined, and the survivor keeps everyone's words. Related ideas stay separate, and no two thoughts carry the same label.
- **Tentative links and a reading.** Up to twelve cross-links with short verb-phrase labels, only where the words support them, and a narrative of two to four sentences saying what the map now shows.
- **Restraint.** You are watching the map while you talk, and every rename or move costs you your bearings. Branches keep their names and places wherever they still fit; structure changes when the map strains, and then decisively, once. A steward that finds the map sound changes nothing.

A rule-based health check (`src/agent/health.rs`) measures the same things without a model: breadth, balance between main branches, crowded and thin branches, empty or wrapper branches, repeated labels, and parked thoughts waiting for a home. Each finding is either a strain, a fault of shape to mend, or an observation to weigh. The steward's brief marks the strains, the steward is shown any that remain after its edits, and a new strain calls it in without waiting for the talk to pause.

The check counts; it cannot know why a map has the shape it has. So for as long as a session is open, the agent carries two things from one turn to the next. A strain the steward was sent back to once and still left is one it judged true to the thinking: it goes on seeing the strain but is not sent back to it, and one that is mended and later returns is new. And a branch one of your instructions arranged is yours. If it strains as you left it (seven plot types gathered under one branch at your request make that branch heavy), the steward is told that it stands on your word and to leave it, and the placer is not told of the strain at all. That lasts until you undo the instruction or the branch carries more thoughts than you left in it; from then on it is the map's to answer for.

### The parking lot

Not everything said belongs on the map. Instead of forcing a thought onto a branch, the agent can park it beside the map, as one of three kinds:

- **Not yet:** it belongs to this subject but cannot be mapped yet, because you put it off ("pricing is a separate conversation") or it needs what comes next to make sense. The agent brings it back once the map has a home for it.
- **Off topic:** it is about something else: a tangent, another project, an errand, small talk. It stays parked unless the subject grows to include it.
- **Aside:** it says nothing that could be mapped: filler, a false start, a word to the room.

When in doubt the agent places a thought; a question, a decision or a next step about the subject is content, not something to park. A parked thought keeps its words, a short topic and the reason it was set aside, and nothing is deleted. The header counts what is parked, and the dock's **Parked · N** tab lists the lot under those three headings.

Your decisions outrank the agent's. **Park** sets a selected thought aside, and the agent may not bring it back. **Bring back** returns a parked thought to the map; the agent finds it a branch and may not park it again. **Keep parked** keeps a thought the agent parked off the map until you bring it back yourself. In **Direct the map** you can say the same in your own words: "park the pricing thoughts for later", "bring the sales call back", "the match and the board deck are off topic for good". Each is an ordinary event (`Parked`, `Revived`, `Held`, or the `park` and `place` edits of a turn), so the parking lot replays with the map.

### Rhythm

- The placer runs as soon as a thought waits; a recording still being cut into thoughts is left alone until the listener is done with it. Typical turns take about a second.
- The steward runs three seconds after the talk pauses once three new thoughts have arrived or the placer asked for it, after a ten-second lull for even one, and without waiting for a pause once eight new thoughts have arrived or the map newly strains (then at most every twenty seconds while the talk runs on). A steward turn that changes the map is one step for **Undo**.
- Neither runs while a directed turn is out. When you move, park, bring back or remove a thought, or undo, turns already in flight are dropped instead of being applied over your edit.
- A failed call leaves the map as it was and backs off: two seconds doubling to thirty for the placer, eight doubling to two minutes for the steward. A placer call is given 12 seconds, a steward or directed call 90.

Thought labels spring into their new positions, colored branches fade back in, and a soft pulse marks a turn in progress. Rune's reduced-motion preference suppresses the fade and pulse.

### Models

| Role | Default | Reasoning | Typical turn | Override |
| --- | --- | --- | --- | --- |
| Listener: cuts a finished recording into thoughts | `openai/gpt-6-luna` | off | about 0.6 s | `TACIT_ASSIST_MODEL`, `TACIT_ASSIST_EFFORT` |
| Placer | `openai/gpt-6-luna` | off | about 1 s | `TACIT_PLACE_MODEL`, `TACIT_PLACE_EFFORT` |
| Steward | `openai/gpt-6.1-terra` | low | 3–7 s | `TACIT_STEWARD_MODEL`, `TACIT_STEWARD_EFFORT` |
| Directed turn | `openai/gpt-6.1-terra` | low | 2–5 s, 5–9 s to reorganise a whole map | `TACIT_DIRECT_MODEL`, `TACIT_DIRECT_EFFORT` |
| Reader: says what a dropped image, scan or long text holds | `openai/gpt-6.1-terra` | low | 2–5 s | `TACIT_READ_MODEL`, `TACIT_READ_EFFORT` |
| Librarian: reads a filed file, writes its notes, tends the topics | `openai/gpt-6.1-terra` | low | not yet measured | `TACIT_LIBRARY_MODEL`, `TACIT_LIBRARY_EFFORT` |
| Researcher: answers a question from the notes | the librarian's | the librarian's | 7–15 s on two questions tried | `TACIT_LIBRARY_MODEL`, `TACIT_LIBRARY_EFFORT` |

The defaults were chosen by measurement with the evaluations under **Development**, on synthetic talks scored against what a facilitator would have left on the wall. The placer is the quickest of the small models tried and the one with the tidiest labels. The steward at low effort kept every map sound; more reasoning and dearer models took two to five times as long without leaving a better map. The directed turn needs the larger model: the small ones answered sooner and fell short on every run, reaching only some of the thoughts an instruction covered. The listener cuts a recording in about a third of the time of the larger model used before, at a thirtieth of the cost. The reader and the librarian are the defaults not yet measured: both use the steward's model, the reader because that one reads images and PDFs, and each has only been tried on a few synthetic files. A 36-capture talk mapped end to end (nine placer turns, three to five steward turns) cost between one and two cents. On a third talk, which announces seven kinds of plot and then describes each, the map ends with the seven in families beside the framework that introduces them, each steward turn taking one call. Asked there for a branch per plot, a directed turn of three to seven seconds leaves eight main branches; asked to put all seven under one branch, it takes about five. The steward lets either stand, and is not sent back to them.

An effort of `off` disables reasoning; anything else is passed on as OpenRouter's reasoning effort (`low`, `medium`, `high`). A replacement model must support strict structured output. Provider routing favors latency for the listener and the placer, whose answers are short, and throughput for the steward and directed turns.

Kept thoughts, their labels and branches, the parking lot, and the guidance you submitted go to OpenRouter and its model providers; unsent drafts and Hex audio stay out of these requests. A dropped image, scan or long text goes to the reader as **Dropping files onto the map** sets out, and what a filed file says to the librarian, as do the notes near a map to the map agents and, when you press **Ask**, your question and the notes read for it to the researcher, as **The knowledge base** sets out. The placer sees the map as an outline with the latest words; the steward and directed turns also read the original words behind the labels, newest first, up to about 24,000 characters. Cloud calls use your OpenRouter credits. The agent keeps a map of up to 400 thoughts, counting parked ones; a larger map remains editable by hand. Provider error text is never shown or logged, since it can echo a private prompt. Older notebooks load without migration.

A small live diagnostic sends only the built-in fictional demo: the placer files it, the steward reads the whole map, the placer files one more thought, and a directed turn renames the map. Each turn's edits are applied the way the journal replays them, and its time, calls and resulting shape are printed:

```sh
cargo run -- --check-openrouter
```

## Local semantic organization

Without configuration, Tacit works entirely offline using simple, explicitly labeled classification rules. This fallback is not semantic AI.

For model-generated classifications, a session title, a narrative, and suggested relationships, run an Ollama server with an installed **local** model, then launch with that model's name:

```sh
TACIT_MODEL=your-installed-model cargo run
```

Tacit sends committed thoughts to `http://127.0.0.1:11434/api/generate`, after a 180 ms quiet period. Draft text is only previewed with local rules. The worker stays off the UI thread, coalesces pending edits, rejects stale results, validates references, and respects pinned classifications. Model-generated links appear beneath their source thoughts and in the selected concept’s review dock; select a link to inspect its target. All model interpretations are suggestions.

A missing server, unknown model, timeout, or invalid response leaves thought capture usable and reports the problem in the status line. Another edit requests organization again. No model is downloaded or server started automatically. The Ollama backend only calls loopback; use an actual local model if you want all inference to stay on your machine.

## Storage

The default session is `$XDG_DATA_HOME/tacit/session.json`, or `~/.local/share/tacit/session.json` when XDG_DATA_HOME is unset. Override it with `TACIT_SESSION=/path/to/session.json` to keep separate notebooks. Captures and corrections save automatically using a synced temporary file and atomic rename. A file lock prevents two app instances from overwriting the same session. Errors remain visible, and malformed sessions are not silently overwritten.

Original text, stable IDs, concept phrases, branches, pinned placements, parked thoughts, your guidance, and the map's focus, narrative and links are persisted. Unsubmitted drafts and undo history are currently in memory only. Sessions are plain JSON, not encrypted.

## Development

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo test --locked --doc
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
# Headless performance probe: synthetic map, zoom, art planning, frame archiving and replay.
# Uses temporary storage; no network, microphone or real notebook access.
cargo test --bin tacit performance_report -- --ignored --nocapture --test-threads=1
# Synthetic archive for offline playback QA; output directory must be new:
cargo run --example replay_fixture -- /tmp/tacit-replay-fixture
```

The tests cover the agent's mechanics with scripted answers: what each edit does, what each tier may do, replay, pacing, and the self-check. Its judgment is measured instead, by two opt-in evaluations that call the configured OpenRouter models. They send only invented talks, never your notebook, and they spend credits (a full run costs one to two cents), so keep runs few:

```sh
# A synthetic talk with tangents, deferrals and filler: placed slice by slice, stewarded
# every third slice (or sooner when the map strains), then scored against what a
# facilitator would have left on the wall. Exit status 1 means the run fell short.
cargo run --release --example agent_eval -- talk=onboarding save=/tmp/tacit-eval.json
cargo run --release --example agent_eval -- talk=machines
cargo run --release --example agent_eval -- talk=plots save=/tmp/tacit-plots.json
# One tier at a time. The steward and the directed turn start from a map kept by save=.
cargo run --release --example agent_eval -- mode=place
cargo run --release --example agent_eval -- mode=steward load=/tmp/tacit-eval.json
cargo run --release --example agent_eval -- mode=direct load=/tmp/tacit-eval.json
# One instruction of your own on a saved map, then two looks by the steward.
cargo run --release --example agent_eval -- mode=direct load=/tmp/tacit-plots.json \
  ask="Give each plot type its own branch." trace=1
# Before changing a default, try the candidate on the same talk:
cargo run --release --example agent_eval -- steward=MODEL steward_effort=low trace=1
# The listener: where a model cuts finished recordings into thoughts, and how fast.
cargo run --release --example split_eval -- model=openai/gpt-6-luna effort=off trace=1
```

Each run prints the finished map with what was parked and why, how the parking compares with a facilitator's (caught, missed, wrongly parked, right kind), the shape of the map, a verdict, and each tier's median and slowest turn with tokens and cost. `mode=direct` gives the saved map the instructions a user might (bring back, park, keep parked, a new main branch, a standing preference, a rename, a reorganisation of the whole map, and one request no map can hold, which must change nothing), one directed turn each, then lets the steward loose to check that it leaves what was decided alone. With `ask=`, it carries out that one instruction instead and gives the steward two looks, which must keep the branches the instruction made, leave what hangs from them as it was, and not be sent back a second time to a strain already left. The three talks are a messy working session (`onboarding`), an explanation that names its own two families (`machines`), and a walkthrough of seven parts of one kind (`plots`). The evaluations are regression checks on synthetic talks, not a guarantee of expert judgment on every discussion.

Rune is pinned to a specific Git revision, and Cargo.lock is included with the project sources. CI uses Rust 1.99.1. Install FFmpeg and FFprobe to run the video integration tests; without them, those tests can be skipped locally. Linux uses Rune's Wayland/X11 backend. On Ubuntu 24.04, install its build dependencies with:

```sh
sudo apt-get install pkg-config libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libx11-dev libxcursor-dev libxi-dev libxrandr-dev libgl1-mesa-dev ffmpeg
```

On macOS use `brew install ffmpeg`; on Windows use `choco install ffmpeg` and the Rust MSVC toolchain with Visual Studio C++ build tools.

### GitHub Actions and downloads

The [CI workflow](https://github.com/patrickyoung/tacit/actions/workflows/ci.yml) runs formatting, Clippy with warnings treated as errors, unit/integration tests, documentation tests, and release builds. It runs on pull requests, pushes to `master`/`main`, `v*` tags, and **Run workflow** in GitHub Actions. Every platform installs and verifies FFmpeg and FFprobe so video tests cannot silently skip due to missing tools. Cloud-model evaluations and the ignored performance probe remain opt-in.

| Build | Rust target | Runner | Download |
| --- | --- | --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | Ubuntu 24.04 | `.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | macOS 15 | `.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | macOS 15 Intel | `.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | Windows Server 2025 | `.zip` |

Open a successful CI run and download the matching **tacit-&lt;target&gt;** artifact. Each contains an archive with the executable and this README, plus a `.sha256` checksum. CI artifacts are retained for 14 days. Extract the archive and run `./tacit --demo` (Windows: `.\tacit.exe --demo`). Unix tarballs preserve executable permissions. These are unsigned executables, not installers or notarized macOS applications. The Linux binary targets systems with glibc 2.39 or newer and the Wayland/X11 libraries above; a working Vulkan driver is required. Windows uses DirectX 12 or Vulkan. Microphone recording remains macOS-only, as described in **Voice with Hex**.

To prepare a versioned release, update `version` in Cargo.toml, refresh Cargo.lock with `cargo check`, commit those changes, then push the matching tag (for example, `git tag v0.1.0` followed by `git push origin v0.1.0`). The tag must exactly match `v` plus the package version. After all four builds pass, the workflow creates a **draft** [GitHub Release](https://github.com/patrickyoung/tacit/releases) containing all archives and checksums. Review the draft and publish it when ready. Builds need no repository secrets; the draft-release job uses GitHub's built-in token with write access limited to that job. Actions are pinned to commits and Dependabot checks for action updates weekly.

Rune's headless tools use the same view and input runtime as the desktop app:

```sh
cargo run -- --demo --snap /tmp/tacit.png --size 1280x860
cargo run -- --demo --snap /tmp/tacit-small.png --size 820x640
cargo run -- --scene themes --snap /tmp/tacit-themes.png --size 1280x860
cargo run -- --scene hierarchy --snap /tmp/tacit-hierarchy.png --size 1680x1000
cargo run -- --scene mapped --snap /tmp/tacit-mapped.png --size 1280x860
cargo run -- --scene parked --snap /tmp/tacit-parked.png --size 1280x860
cargo run -- --scene room --snap /tmp/tacit-room.png --size 1280x860
cargo run -- --scene answer --snap /tmp/tacit-answer.png --size 1280x860
```

`mapped` is a fictional map as the agent leaves one (keyword labels, a repeat combined, links, a thread to read, and a parking lot of each kind); `parked` is the same map with the parking lot open. `room` is one note open in the knowledge room, over a small fictional knowledge base, and `answer` is a question answered from it.

Headless runs do not read or write your real notebook or contact OpenRouter, Ollama, or Hex. Tests exercise real text entry, selection, reclassification, deletion, undo, storage, locking, organizer validation, and stale-result handling.

## Structure and current scope

- `src/art.rs`: offline motifs, hand-drawn strokes, coalesced illustration transitions.
- `src/generated_art.rs`: image model requests, stable scene briefs, bounded private cache, replacement transitions.
- `src/model.rs`: original thoughts, narrative roles, branches, the parking lot, the map's charter (focus, ordering idea, narrative, links), and validated model interpretations.
- `src/agent.rs`: the map agent's vocabulary of edits and the two ways they meet the map: strict replay from the journal, and lenient live application under each tier's licence.
- `src/agent/`: `prompt` (the mapper's method told to each tier, and the answer schemas), `brief` (the map as the outline a turn is shown), `wire` (answers read into edits), `step` (one edit against the map), `health` (the rule-based check of the map's shape), `pace` (when each tier is due), `turn` (one model turn, its self-check, and what is remembered from one turn to the next), `worker` (the three lanes, off the UI thread).
- `src/recording.rs`: the listener, which cuts a finished recording into thoughts without changing a word.
- `src/ingest.rs`: dropped files, kept in the archive and read into text on this Mac or by the reader model.
- `src/knowledge.rs`: the knowledge base: notes as Markdown files, their links, which notes are near a text, which pages are the user's own, and the levels an agent is shown them at. `src/knowledge/librarian.rs` is the librarian: the reader, the writer and the curator, and the checks on what they return. `src/knowledge/research.rs` is the researcher: one question answered by searching and opening notes, and the checks of the answer against what was read.
- `src/capture.rs`: original render-texture capture and exact presentation.
- `src/video_archive.rs`: lossless PNG masters, timestamped FFV1/PCM video export.
- `src/timeline.rs`: durable event log, shared projection, media integrity, WebVTT, and recovery.
- `src/session_event.rs`: portable content events and the pure reducer.
- `src/presentation.rs`: Rune presentation and version 1 scene compatibility adapter.
- `src/storage.rs`: session loading, atomic saves, exclusive session lock.
- `src/organizer.rs`: the OpenRouter key and model settings, and the local Ollama organizer with its response validation.
- `src/evolution.rs`: the source ledger (the words behind every thought) and retired thoughts; the whole-map revisions of older journals still replay from here.
- `src/direction.rs`: the facilitator commands of journals written before the map agent, kept so those journals still replay.
- `src/voice.rs`: Rune WAV capture, authenticated Hex transcription streams, and exact sample uploads.
- `src/ui.rs`: Rune canvas, vector illustrations, input, inspector, and narrative view. `src/ui/room.rs` is the knowledge room: the notes read by topic, link, source and search, and the researcher's answers.
- `design/living-map.html`: the earlier conceptual exploration, independent of the Rust app.

This is a first working vertical slice. With the map agent the tree keeps three to seven main branches (an eighth on your word; four narrative lenses offline) and nests at most three levels deep. Drag positioning, relationship rejection, and SVG/PDF export remain future work. The agent's judgment is measured on synthetic talks only; how it does on a real discussion depends on the configured models. The local-model integration is tested with fixtures; model quality and live inference still need validation with an installed model.

## Multiple sessions

Use **Sessions** in the header to open your session library. **New session** starts an independent mindmap immediately, with a distinct label such as **Conversation 1**. As you speak or type, its name follows the map's title, falling back to the first idea until a title is available. Existing generic session labels also display their conversation's topic. Enter a name and choose **Rename current** to keep a name of your own; automatic naming never replaces an explicit rename. Select a session to resume it. The original notebook stays in the library; names never become filesystem paths, and duplicate custom names are allowed. Naming uses existing content and makes no additional model requests.

Each session has its own event log, audio, WebVTT, artwork, original frames, draft, and replay history. Switching saves the draft and closes the current capture cleanly. Recording, transcription/chunking, map guidance, and video export must finish before switching. Late organization and image results stay isolated in the old workers. A session open in another process cannot be opened for editing.

The library lives beside the existing notebook in `session.sessions/`, with one directory per session. `TACIT_SESSION` selects the original notebook and therefore its adjacent library. Startup opens that original notebook; use the picker to reopen another session. Library browsing pauses original-frame capture, with explicit capture boundaries; the library itself is outside the recorded mindmap scene.

## Brainstorm and Direct the map

The composer has two explicit modes, each with its own draft. **Brainstorm** captures ideas as thoughts. **Direct the map** treats typed text or speech as an instruction to the map agent. Select the mode, press **Speak guidance**, and say what you want of the map; **Stop & direct** transcribes the instruction and hands it over. The mode cannot change during a recording. If earlier brainstorm recordings are still being cut into thoughts, the instruction waits and is carried out on the completed map; the status shows the wait and then the application.

A directed turn is one turn of the agent made on your word and with your authority. It does all of what you asked and only that, and answers in one plain sentence, shown in the status, saying what changed or why nothing did. Try:

- “Reorganise the whole map around the customer's journey, from signing up to the first report.”
- “Make a new main branch called Quick wins and put the checklist and the signup experiment in it.”
- “Rename Costs to Infrastructure costs.” “Put Decisions before Costs.” “Call this map Onboarding fix.”
- “Park everything about the sales call for another day.” “Bring the pricing thought back.” “The match and the board deck are off topic for good.”
- “From now on, keep every branch name to one or two words.”

What an instruction decides outranks the agent's own judgment:

- A thought you name and send to a branch is pinned there. Thoughts that move as part of a wider change stay open to the steward.
- What you park stays parked until you bring it back, and what you bring back is not parked again.
- An eighth main branch exists only because you asked for it. When all eight are taken, the agent makes room by nesting or merging branches and says which went where.
- A branch holding a thought you pinned keeps its name and stays where it hangs: the map refuses the agent's rename, move or merge. A branch you only named or renamed, and an order you gave the main branches, are kept on instruction: the steward reads your latest instructions on every turn and is told to let them stand, but the map does not enforce it.
- A branch your instruction made, moved or filled is not the agent's to repair, however it strains by the rules of thumb. Until you undo the instruction or the branch carries more thoughts than you left in it, the steward is told that it stands on your word and the placer is told nothing of the strain (see **What the agent knows about mind maps**). This too is instruction to the models, which the map does not enforce.
- A standing preference (“from now on…”, “anything about pricing can wait”) is applied at once and remembered. The latest twenty instructions are given to the listener, the placer and the steward on every later turn, and one that changes nothing today is still saved. What an instruction arranged stands for as long as it is among them.
- Nothing is deleted on an instruction. A thought you want gone is parked with its words kept; **Remove** in the review dock is the way to delete one.

An instruction cannot rewrite original words or set positions, fonts, or styles. One that is not about this map, or asks for something a map cannot hold, changes nothing and says so. Where a request can be read two ways, the agent takes the reading that changes less and says which it took. A branch your instruction empties goes with it unless you asked for that branch by name, and a reorganisation of the whole map that leaves a branch with more than seven items is sent back once to divide it.

The turn's edits are tried on the map as it stands when the answer arrives, so whatever you did by hand in the meantime stands; an edit that no longer fits is left out and counted in the status. What held is saved as one `Guided` event with your instruction, and is one **Undo** step. **Undo** while an instruction is waiting or under way cancels it and keeps your words in the draft; a failed instruction stays in the draft for retry as well. After a directed turn the steward leaves the map as you set it until new thoughts arrive. Spoken instructions keep their original audio and WebVTT but never enter the thought list or the listener's queue. Direction mode requires OpenRouter; it does not silently fall back to brainstorming.

## Concept phrases and original words

Every thought carries a `concept` phrase beside its original words. The placer distills each new thought into a keyword label of two to six words (at most eight words and 58 characters) in sentence case. It names the idea instead of clipping the sentence, resolves pronouns from what was said before, and keeps negation and uncertainty. A suggestion begins with **Proposed** or **Possible** so that it never reads as settled: “Maybe a single owner could help us decide” becomes “Possible single-owner decisions”. The steward corrects labels that mislead or merely echo the words, and gives thoughts that ended up sharing a label one each. Otherwise a label stays as it is from turn to turn.

The map displays concept phrases. Select a node to see its concept and original words in the bottom review dock. Full text, WAV recordings, and WebVTT are unchanged. Until a concept is available, a node shows a shortened preview of its words. The local organizer can supply concepts too, and older or local responses without them remain compatible.

Concepts are interpretations, not replacement transcripts. They are validated, saved in session events, and included in replay and other renderers. Splitting a thought clears its concept so that each resulting idea is distilled on its own.

Branches nest: **main branch → sub-branch → group → thoughts**. A map holds at most three levels, 72 branches, and eight main branches, of which the agent uses seven (see **The map agent**). A sub-branch has to earn its place with two or more thoughts that share something their siblings do not. Existing flat maps load unchanged. Nested groups share their root branch color and extend outward through curved branches. Large maps scroll; selecting a thought shows its full classification path and original words. The mural draws on descendant concepts across the whole map, while text branches remain readable to either side.

In **Direct the map**, instructions such as “Put Decision owners under Governance” create or move whole subtrees. Merging branches retains their children. Cycles, unknown parents, and excessive depth are refused. A pinned thought fixes its place for the agent, which does not move, combine or park it, change its role, or rename, move or merge a branch on its path; your own instructions can. Parent relationships are saved in ordinary session events and replay through the same reducer.

The central mural is composed on request, as described above. Artwork and its art direction are archived alongside the map; old scene recordings retain their original layout and images without new model calls.

## A map that can change its mind

The map is a reading of the conversation, and the steward revises that reading as the conversation goes on. It sees the whole map with the words behind every label, the parking lot, and your guidance, and returns only the edits that make the map a truer picture: branches renamed, merged, nested or put in reading order, thoughts moved to a better home or relabelled, parked thoughts brought back, and repeats combined. Related problems, proposals, constraints, and competing options stay separate contributions; shared vocabulary alone is no reason to combine them. The agent refers to thoughts by number and never rewrites a word that was said.

When two thoughts state the same idea, they are combined into one. The survivor carries everyone's original words, and each absorbed thought is retired with a reason and a link to the thought that replaced it. The review dock's **Prior concepts** control opens those retired thoughts and their history. All evidence stays attached to an active, parked, or retired thought. What you pinned is protected; **Allow AI to revise** releases a selected pin. Steward turns use the existing layout transitions, and each one that changes the map is one **Undo** step.

Every turn of the agent is saved as an ordered list of edits: a `Mapped` event for the placer and the steward, a `Guided` event with your instruction for a directed turn. What you do to the parking lot by hand is saved as `Parked`, `Revived` and `Held`. Live, each edit is tried against the map as it stands and one that no longer fits is left out, so only the edits that held are saved. Replay applies them in order without calling a model. Saved session JSON also keeps the evidence dictionary, retired thoughts, the parking lot, and the map's focus, ordering idea, narrative and links.

Journals written before the map agent replay unchanged. Their `Organized`, `Directed` and `MapRevised` events (the earlier classifier, facilitator commands, and whole-map revisions) keep their reducers although nothing writes them any more, and legacy empty fields remain omitted for journal hash compatibility.

## Voice-first workspace and canvas navigation

The live workspace brings Brainstorm / Direct the map, recording, and transcript feedback together in one dock below the canvas. Recording stays the primary action; connection, microphone startup, transcription, and guidance each show their current state. Input modes are locked during recording and guidance. **Reconnect Hex** checks the local transcription service; map guidance explains when OpenRouter is unavailable. Sessions open in a collapsible left drawer; at narrow sizes the drawer overlays the workspace. Switching is still blocked while a recording is in progress.

Select a concept to read its original words, move, park or unpin it, or follow relationships and prior concepts. **Parked · N** opens the parking lot with the room to read it: the selected thought with why it was parked, then the whole lot under **Not yet**, **Off topic** and **Aside**, one line a kind, each item named by its subject wherever its label does not say it already. **Bring back** returns a parked thought to the map and **Keep parked** holds it there. Choose **Transcript** to return to the chronological recording text. Finalized live windows appear before recording finishes; the final transcript replaces those windows without duplicate display. Expand the dock for longer reading, scroll back to pause following, and use **Jump to live** (or **Jump to latest** after recording) to return to the latest words. Following live is a status, not a toggle. Typed input remains available below the transcript; its submit button appears when there is a draft. Empty sessions start with an uncluttered canvas and four ways to begin (**I keep noticing…**, **Why do we…**, **But what about…**, **What if…**); choosing one opens the typed thought on those words.

Secondary actions appear when useful: Undo after an edit, **Create artwork** or **Update artwork** when the conversation holds something the mural does not show, **Hide artwork** when there is a mural, and Retry save / Retry art when recovery is needed. Saving remains automatic. **Read the thread** opens the narrative view; canvas navigation is shown only on the live map. Replay, transcript recovery, prior concepts, and lossless video export remain available in their relevant contexts.

The map is now a camera-driven canvas: two-axis trackpad/wheel panning, background drag-to-pan, pinch zoom, and Command/Ctrl+wheel zoom around the pointer. **− / zoom level / + / Fit map** controls float in the canvas's lower-right corner; the zoom level says how large the map is drawn, and pressing it returns to actual size. Keyboard shortcuts follow [MindNode's zoom conventions](https://www.mindnode.com/support/guides/keyboard-shortcuts): Command +/−, Command 0 for actual size, and Command * to fit. Panning and zoom gestures use Rune live transforms, avoiding a full interface and map layout rebuild on every movement. Until you pan or zoom, the map keeps every idea in view as it grows, centered in the canvas and clear of those controls. After that the camera stays where you put it as new speech arrives; Fit map reframes the growing map and follows it again.

Camera position/zoom, dock expansion, follow mode and transcript scroll position are saved in presentation events. Older presentation events retain the original interface for scene replay. No network or microphone access is needed for headless UI checks.
