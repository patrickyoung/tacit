//! Voice-first workspace; the original renderer remains available for legacy replay.
use super::*;
use tacit::camera::Camera;

impl State {
    pub(super) fn voice_in_progress(&self) -> bool {
        matches!(
            self.voice_state,
            VoiceState::Starting
                | VoiceState::Recording
                | VoiceState::Finishing
                | VoiceState::Cancelling
        )
    }

    fn input_mode_locked(&self) -> bool {
        self.read_only
            || self.replay.is_some()
            || self.voice_in_progress()
            || self.directing
            || self.queued_direction.is_some()
    }

    fn can_submit_draft(&self) -> bool {
        !self.read_only
            && self.replay.is_none()
            && if self.direction_mode {
                self.director.is_some()
                    && !self.direction_draft.text.trim().is_empty()
                    && !self.input_mode_locked()
            } else {
                !self.draft.text.trim().is_empty()
            }
    }

    pub(super) fn camera(&self) -> Camera {
        Camera {
            zoom: self.camera_zoom.get(),
            pan: self.camera_pan.get(),
        }
    }
    fn set_camera(&mut self, camera: Camera) {
        self.camera_zoom.set(camera.zoom);
        self.camera_pan.set(camera.pan);
        self.camera_ready = true;
    }
    pub(super) fn zoom_center(&mut self, factor: f32) {
        if self.replay.is_some() {
            return;
        }
        let mut camera = self.camera();
        camera.zoom_at(
            factor,
            [self.map_viewport[0] / 2.0, self.map_viewport[1] / 2.0],
        );
        self.set_camera(camera);
    }
    pub(super) fn fit_map(&mut self) {
        if self.replay.is_some() {
            return;
        }
        let mut camera = self.camera();
        camera.fit(self.map_size, self.map_viewport);
        self.set_camera(camera);
    }
}

pub(super) fn canvas(s: &State, sheet: El, size: [f32; 2]) -> El {
    el().key("map")
        .help("Drag or scroll to pan. Pinch to zoom. Use Fit map to see every idea.")
        .w(0.0)
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .clip_rounded(16.0)
        .on_layout_tx(move |s: &mut State, rect, tx| {
            if s.replay.is_none() {
                s.map_viewport = [rect[2], rect[3]];
                s.map_size = size;
                if !s.camera_ready {
                    s.fit_map();
                    // Use a little more of a compact canvas without hiding its outer nodes.
                    if s.camera_zoom.get() < 0.65 {
                        s.zoom_center(1.1);
                    }
                }
                tx.instant();
            }
        })
        .on_wheel(|s: &mut State, e, tx| {
            if s.replay.is_some() {
                return true;
            }
            if e.mods.cmd || e.mods.ctrl {
                let mut camera = s.camera();
                camera.zoom_at((e.dy * 0.006).exp(), [e.x, e.y]);
                s.set_camera(camera);
                tx.live_only();
            } else {
                let p = s.camera_pan.get();
                s.camera_pan.set([p[0] + e.dx, p[1] + e.dy]);
                tx.live_only();
            }
            true
        })
        .on_magnify(|s: &mut State, e, tx| {
            if s.replay.is_none() {
                let mut camera = s.camera();
                camera.zoom_at(1.0 + e.delta, [e.x, e.y]);
                s.set_camera(camera);
                tx.live_only();
            }
            true
        })
        .on_smart_magnify(|s: &mut State, _, _, tx| {
            s.fit_map();
            tx.live_only();
            true
        })
        .on_drag(|s: &mut State, d, tx| {
            if s.replay.is_some() {
                return;
            }
            match d.phase {
                rune::Phase::Down => s.drag_pan = s.camera_pan.get(),
                rune::Phase::Move => {
                    s.camera_pan
                        .set([s.drag_pan[0] + d.dx, s.drag_pan[1] + d.dy]);
                }
                rune::Phase::Up if d.cancelled() => {
                    s.camera_pan.set(s.drag_pan);
                }
                _ => {}
            }
            tx.live_only();
        })
        .child(
            sheet
                .absolute(0.0, 0.0)
                .origin(0.0, 0.0)
                .live_scale(&s.camera_zoom)
                .live_offset(&s.camera_pan),
        )
}

fn pill(title: &str, key: &str, active: bool, action: impl Fn(&mut State) + 'static) -> El {
    button(title, key, action)
        .py(10.0)
        .radius(12.0)
        .fill(if active { INK } else { WHITE })
        .color(if active { WHITE } else { INK })
        .hover(if active { INK } else { LINE })
        .press_scale(0.97)
}

fn mode_controls(s: &State) -> El {
    let mut modes = row()
        .key("input-modes")
        .role("tablist")
        .label("Input mode")
        .focus_group()
        .gap(2.0)
        .padding(3.0)
        .radius(10.0)
        .fill(PAPER);
    for (title, key, direction) in [
        ("Brainstorm", "mode-brainstorm", false),
        ("Direct the map", "mode-direct", true),
    ] {
        let active = s.direction_mode == direction;
        modes = modes.child(enabled(
            button(title, key, move |s| {
                if s.direction_mode != direction && !s.input_mode_locked() {
                    s.toggle_direction();
                }
            })
            .role("tab")
            .focus_selected(active)
            .fill(if active { WHITE } else { Color::TRANSPARENT })
            .color(if active { INK } else { MUTED })
            .hover(WHITE)
            .help(if s.input_mode_locked() {
                "Finish the current recording or guidance before changing modes."
            } else if direction {
                "Give instructions to organize your existing ideas."
            } else {
                "Capture ideas and let the map take shape."
            }),
            !s.input_mode_locked(),
        ));
    }
    modes
}

fn voice_controls(s: &State) -> El {
    // A running microphone always keeps its stop control, including after a save error.
    if !s.voice_in_progress() {
        let idle = if s.read_only {
            Some("Read-only session")
        } else if s.directing {
            Some("Applying guidance…")
        } else if s.queued_direction.is_some() {
            Some("Finishing captured thoughts…")
        } else if s.direction_mode && s.director.is_none() {
            Some("Map guidance unavailable")
        } else if s.voice_worker.is_none() {
            Some("Voice unavailable")
        } else {
            None
        };
        if let Some(message) = idle {
            return label(message, 13.0, MUTED)
                .key("voice-progress")
                .px(12.0)
                .py(12.0);
        }
    }
    let action = match s.voice_state {
        VoiceState::Ready => pill(
            if s.direction_mode {
                "●  Speak guidance"
            } else {
                "●  Start recording"
            },
            "voice-start",
            true,
            |s| s.voice_command(VoiceCommand::Start),
        )
        .fill(INK)
        .font(TextStyle::new(14.0, 500.0))
        .px(18.0)
        .py(12.0)
        .help("Record your voice. Your typed draft is kept separately."),
        VoiceState::Recording => pill(
            if s.direction_mode {
                "■  Stop & direct"
            } else {
                "■  Finish recording"
            },
            "voice-stop",
            true,
            |s| s.voice_command(VoiceCommand::Finish),
        )
        .fill(RED)
        .font(TextStyle::new(14.0, 500.0))
        .hover(RED)
        .px(18.0)
        .py(12.0),
        VoiceState::Unavailable => pill("Reconnect Hex", "voice-check", true, |s| {
            s.voice_command(VoiceCommand::Check)
        })
        .font(TextStyle::new(14.0, 500.0))
        .py(12.0)
        .help("Open Hex with a local model loaded, then retry the connection."),
        state => label(
            match state {
                VoiceState::Checking => "Connecting to Hex…",
                VoiceState::Starting => "Opening microphone…",
                VoiceState::Finishing => "Finishing transcript…",
                VoiceState::Cancelling => "Discarding guidance…",
                _ => unreachable!(),
            },
            13.0,
            MUTED,
        )
        .key("voice-progress")
        .px(12.0)
        .py(12.0),
    };
    let mut controls = row().gap(10.0).center_y();
    if s.voice_state == VoiceState::Recording {
        controls = controls.child(
            label(format!("●  {}", time(s.voice_seconds * 1000)), 13.0, RED)
                .key("recording-timer")
                .label(format!(
                    "Recording, {} elapsed",
                    time(s.voice_seconds * 1000)
                )),
        );
    }
    controls = controls.child(action);
    if s.voice_state == VoiceState::Recording && s.direction_mode {
        controls = controls.child(button("Cancel", "voice-cancel", |s| {
            s.voice_command(VoiceCommand::Cancel)
        }));
    }
    controls
}

fn voice_feedback(s: &State) -> &str {
    // Connection failures must stay visible in both input modes.
    if s.voice_in_progress() || s.voice_state == VoiceState::Checking {
        if s.voice_state == VoiceState::Recording && s.voice_status == "Listening with Hex" {
            return if s.direction_mode {
                "Describe the change. Your guidance is applied when you finish."
            } else {
                "Listening. Your words appear below as the map takes shape."
            };
        }
        return &s.voice_status;
    }
    if s.voice_state == VoiceState::Unavailable && s.voice_worker.is_some() {
        return &s.voice_status;
    }
    if s.direction_mode {
        return if s.director.is_none() {
            "Map guidance needs OpenRouter. You can still capture ideas in Brainstorm."
        } else {
            &s.direction_status
        };
    }
    if s.voice_worker.is_none() {
        return "Voice is available in the desktop app. You can type a thought below.";
    }
    &s.voice_status
}
fn drawer(s: &State) -> El {
    let can_switch = !s.voice_in_progress()
        && !s.directing
        && s.queued_direction.is_none()
        && s.pending_recordings.is_empty()
        && !s.video_busy();
    let mut list = col().gap(6.0).grow(1.0).min_h(0.0).scroll(ScrollSpec {
        stick_bottom: false,
    });
    for (index, entry) in s.session_entries.iter().enumerate() {
        let path = entry.path.clone();
        let current = s.path.as_ref() == Some(&path);
        list = list.child(enabled(
            pill(
                &entry.name,
                &format!("session-{index}"),
                current,
                move |s| s.switch_session(path.clone()),
            )
            .w_full()
            .wrap_anywhere(),
            can_switch || current,
        ));
    }
    col()
        .key("session-drawer")
        .w(250.0)
        .min_w(250.0)
        .h_full()
        .padding(16.0)
        .gap(14.0)
        .fill(WHITE)
        .radius(16.0)
        .transition(rune::Transition::offset(-250.0, 0.0))
        .child(
            row()
                .center_y()
                .child(label("Your sessions", 18.0, INK).grow(1.0))
                .child(button("Close", "sessions-back", |s| {
                    s.sessions_open = false
                })),
        )
        .child(enabled(
            pill("+  New session", "session-create", false, |s| {
                s.session_name_edit = TextEdit::new("");
                s.create_session();
            })
            .fill(PAPER),
            can_switch,
        ))
        .child(list)
        .child(
            text_input(&s.session_name_edit, TextStyle::new(14.0, 400.0))
                .key("session-name")
                .label("Rename this session")
                .help("Choose a name to keep instead of the automatic conversation title.")
                .h(40.0)
                .color(INK)
                .placeholder("Session name", MUTED)
                .on_edit(|s: &mut State| &mut s.session_name_edit),
        )
        .child(enabled(
            button("Rename current", "session-rename", State::rename_session),
            !s.read_only
                && !s.session_name_edit.text.trim().is_empty()
                && s.session_name_edit.text.trim() != s.session_name,
        ))
        .child(label(
            if can_switch {
                "Each session keeps its audio, transcript and map history."
            } else {
                "Finish recording, transcription, guidance or export before switching sessions."
            },
            11.0,
            MUTED,
        ))
}
fn time(ms: u64) -> String {
    format!("{:02}:{:02}", ms / 60_000, ms / 1000 % 60)
}
fn transcript(s: &State) -> El {
    let mut lines = col().gap(12.0).w_full();
    let mut count = 0;
    // Finished transcripts supersede live windows; never display both copies.
    for audio in &s.projection.audio {
        let instruction = s.projection.direction_recordings.contains(&audio.id);
        lines = lines.child(label(
            format!(
                "{}  ·  {}",
                time(audio.start_ms),
                if instruction {
                    "Map direction"
                } else {
                    "Conversation"
                }
            ),
            10.0,
            GREEN,
        ));
        if let Some((_, cues, full)) = s
            .projection
            .transcripts
            .iter()
            .find(|(id, _, _)| id == &audio.id)
        {
            if cues.is_empty() {
                lines = lines.child(
                    label(full.clone(), 15.0, INK)
                        .wrap_anywhere()
                        .selectable(true),
                );
                count += 1;
            } else {
                for (index, cue) in cues.iter().enumerate() {
                    lines = lines.child(
                        row()
                            .gap(14.0)
                            .child(label(time(audio.start_ms + cue.start_ms), 11.0, MUTED).w(48.0))
                            .child(
                                label(cue.text.clone(), 15.0, INK)
                                    .key(format!("transcript-{}-{index}", audio.id))
                                    .grow(1.0)
                                    .min_w(0.0)
                                    .wrap_anywhere()
                                    .selectable(true),
                            ),
                    );
                    count += 1;
                }
            }
        } else if let Some(segments) = s.projection.speech_segments.get(&audio.id) {
            for (index, segment) in segments {
                lines = lines.child(
                    row()
                        .gap(14.0)
                        .child(
                            label(time(audio.start_ms + segment.cue.start_ms), 11.0, MUTED).w(48.0),
                        )
                        .child(
                            label(segment.cue.text.clone(), 15.0, INK)
                                .key(format!("live-segment-{}-{index}", audio.id))
                                .grow(1.0)
                                .min_w(0.0)
                                .wrap_anywhere()
                                .selectable(true),
                        ),
                );
                count += 1;
            }
        }
    }
    for (id, segments) in &s.projection.speech_segments {
        if s.projection.audio.iter().any(|a| &a.id == id) {
            continue;
        }
        lines = lines.child(label("CURRENT RECORDING · elapsed time", 10.0, GREEN));
        for (index, segment) in segments {
            lines = lines.child(
                row()
                    .gap(14.0)
                    .child(label(time(segment.cue.start_ms), 11.0, MUTED).w(48.0))
                    .child(
                        label(segment.cue.text.clone(), 15.0, INK)
                            .key(format!("live-segment-{id}-{index}"))
                            .grow(1.0)
                            .min_w(0.0)
                            .wrap_anywhere()
                            .selectable(true),
                    ),
            );
            count += 1;
        }
    }
    if s.projection.audio.is_empty() && s.projection.speech_segments.is_empty() {
        let evidence = s.projection.evidence();
        let mut originals = evidence
            .iter()
            .filter_map(|(id, words)| {
                id.strip_prefix("thought-")
                    .and_then(|n| n.parse::<u64>().ok())
                    .map(|n| (n, id, words))
            })
            .collect::<Vec<_>>();
        originals.sort_by_key(|(n, _, _)| *n);
        for (_, id, words) in originals {
            lines = lines.child(
                label(words.clone(), 15.0, INK)
                    .key(format!("source-{id}"))
                    .wrap_anywhere()
                    .selectable(true),
            );
            count += 1;
        }
    }
    if !s.live_text.is_empty() {
        lines = lines
            .child(label("LIVE · still being transcribed", 10.0, GREEN))
            .child(
                label(s.live_text.clone(), 15.0, INK)
                    .key("live-transcript")
                    .wrap_anywhere()
                    .selectable(true),
            );
    } else if count == 0 {
        lines = lines.child(label(
            "Speak freely. Your words appear here while the map takes shape above.",
            16.0,
            MUTED,
        ));
    }
    lines
}
fn concept_label(s: &State, id: u64) -> &str {
    s.projection
        .session
        .thoughts
        .iter()
        .chain(s.projection.session.retired.iter().map(|r| &r.thought))
        .find(|t| t.id == id)
        .map_or("Related thought", |t| t.map_label())
}

fn selected(s: &State) -> Option<El> {
    let t = s
        .projection
        .session
        .thoughts
        .iter()
        .chain(s.projection.session.retired.iter().map(|r| &r.thought))
        .find(|t| Some(t.id) == s.selected)?;
    let retired = s
        .projection
        .session
        .retired
        .iter()
        .find(|r| r.thought.id == t.id);
    let mut controls = row().gap(6.0).center_y().child(
        label(s.projection.session.placement_path(t), 11.0, color(t.role))
            .help(s.projection.session.placement_path(t))
            .grow(1.0)
            .min_w(0.0)
            .truncate(),
    );
    if s.replay.is_none() && !s.read_only && retired.is_none() {
        controls = controls
            .child(button("Move to next branch", "move", State::move_selected))
            .child(button("Remove", "remove", State::remove_selected));
        if t.pinned {
            controls = controls.child(button("Allow AI to revise", "unpin", State::unpin_selected));
        }
    }
    let mut detail = col()
        .gap(8.0)
        .child(controls)
        .child(
            label(
                t.concept.as_deref().unwrap_or("Original thought"),
                20.0,
                INK,
            )
            .key("selected-concept")
            .selectable(true),
        )
        .child(
            label(t.text.clone(), 15.0, INK)
                .key("selected-original")
                .wrap_anywhere()
                .selectable(true),
        );
    if let Some(r) = retired {
        detail = detail.child(label(r.reason.clone(), 13.0, MUTED));
        for id in &r.replaced_by {
            let id = *id;
            detail = detail.child(button(
                &format!("Now: {}", short_label(concept_label(s, id))),
                &format!("replacement-{id}"),
                move |s| s.selected = Some(id),
            ));
        }
    }
    if retired.is_some() {
        detail = detail.child(label("MAP HISTORY", 10.0, MUTED));
        for r in s.projection.session.retired.iter().rev() {
            let id = r.thought.id;
            detail = detail.child(button(
                r.thought.map_label(),
                &format!("retired-{id}"),
                move |s| s.selected = Some(id),
            ));
        }
    }
    // Keep semantic cross-links discoverable after removing the right inspector.
    if let Some(i) = &s.projection.session.interpretation {
        for r in i
            .relationships
            .iter()
            .filter(|r| r.from == t.id || r.to == t.id)
        {
            let id = if r.from == t.id { r.to } else { r.from };
            detail = detail.child(button(
                &format!("{} → {}", r.label, short_label(concept_label(s, id))),
                &format!("connection-{id}"),
                move |s| s.selected = Some(id),
            ));
        }
    }
    Some(detail)
}
fn has_transcript(s: &State) -> bool {
    !s.projection.audio.is_empty()
        || !s.projection.speech_segments.is_empty()
        || !s.projection.session.thoughts.is_empty()
        || !s.projection.session.retired.is_empty()
        || !s.live_text.is_empty()
}

fn dock(s: &State) -> El {
    let selected = selected(s);
    let inspecting = selected.is_some();
    let mut dock = col()
        .key("composer")
        .gap(12.0)
        .padding(16.0)
        .fill(WHITE)
        .radius(16.0)
        .border(1.0, LINE.alpha(0.6));

    if s.replay.is_none() {
        dock = dock.child(
            row()
                .gap(12.0)
                .center_y()
                .child(if s.read_only { el() } else { mode_controls(s) })
                .child(el().grow(1.0))
                .child(voice_controls(s)),
        );
        if !s.read_only {
            dock = dock.child(
                label(voice_feedback(s), 12.0, MUTED)
                    .key("voice-feedback")
                    .wrap_anywhere(),
            );
        }
    }

    if inspecting || has_transcript(s) || s.voice_in_progress() {
        let mut tabs = row()
            .gap(6.0)
            .center_y()
            .child(if inspecting {
                button("← Transcript", "show-transcript", |s| s.selected = None)
            } else {
                label("Transcript", 12.0, MUTED).px(6.0)
            })
            .child(el().grow(1.0));
        if let Some(r) = s.projection.session.retired.last() {
            let id = r.thought.id;
            tabs = tabs.child(button("Prior concepts", "concept-history", move |s| {
                s.selected = Some(id)
            }));
        }
        if !inspecting && s.replay.is_none() {
            if !s.transcript_following {
                tabs = tabs.child(
                    label(
                        if s.voice_state == VoiceState::Recording {
                            "Jump to live ↓"
                        } else {
                            "Jump to latest ↓"
                        },
                        12.0,
                        GREEN,
                    )
                    .key("transcript-follow")
                    .role("button")
                    .focusable(true)
                    .px(12.0)
                    .py(8.0)
                    .radius(7.0)
                    .hover(PAPER)
                    .on_press_tx(|s: &mut State, tx| {
                        s.transcript_following = true;
                        tx.scroll_to_instant("transcript-scroll", 0.0, 1_000_000_000.0);
                    }),
                );
            } else if s.voice_state == VoiceState::Recording {
                tabs = tabs.child(label("Following live", 11.0, GREEN).px(8.0));
            }
        }
        if s.replay.is_none() {
            tabs = tabs.child(
                button(
                    if s.transcript_expanded {
                        "Collapse"
                    } else {
                        "Expand"
                    },
                    "transcript-expand",
                    |s| s.transcript_expanded = !s.transcript_expanded,
                )
                .expanded(s.transcript_expanded)
                .help("Make more room to read the transcript or selected idea."),
            );
        }
        let content = col()
            .key("transcript-scroll")
            .h(if s.transcript_expanded {
                (s.viewport[1] * 0.30).clamp(150.0, 320.0)
            } else if inspecting {
                104.0
            } else {
                64.0
            })
            .min_h(0.0)
            .px(6.0)
            .on_scroll_offset({
                let offset = s.transcript_scroll.clone();
                move |p| {
                    offset.set(p.offset);
                }
            })
            .on_wheel(|s: &mut State, e, _| {
                if s.replay.is_none() && s.selected.is_none() && e.dy > 0.0 {
                    s.transcript_following = false;
                }
                false
            })
            .child(selected.unwrap_or_else(|| transcript(s)))
            .scroll(ScrollSpec {
                stick_bottom: !inspecting && s.transcript_following,
            });
        dock = dock.child(col().gap(4.0).child(tabs).child(content));
    }

    if s.replay.is_none() && !s.read_only {
        let draft = if s.direction_mode {
            &s.direction_draft
        } else {
            &s.draft
        };
        let mut input = row()
            .gap(10.0)
            .center_y()
            .px(12.0)
            .py(4.0)
            .radius(9.0)
            .fill(PAPER)
            .child(
                text_input(draft, TextStyle::new(14.0, 400.0))
                    .key("thought-input")
                    .label(if s.direction_mode {
                        "Type map guidance"
                    } else {
                        "Type a thought"
                    })
                    .grow(1.0)
                    .min_w(0.0)
                    .h(32.0)
                    .color(INK)
                    .placeholder(
                        if s.direction_mode {
                            "Or type map guidance…"
                        } else {
                            "Or type a thought…"
                        },
                        MUTED,
                    )
                    .help("Enter to submit. Shift+Enter for a new line.")
                    .on_edit(|s: &mut State| {
                        if s.direction_mode {
                            &mut s.direction_draft
                        } else {
                            &mut s.draft
                        }
                    })
                    .on_submit(|s: &mut State, _| {
                        if s.can_submit_draft() {
                            s.capture();
                        }
                    }),
            );
        if !draft.text.trim().is_empty() {
            input = input.child(enabled(
                button(
                    if s.direction_mode {
                        "Apply guidance ↵"
                    } else {
                        "Add thought ↵"
                    },
                    "capture",
                    |s| {
                        if s.can_submit_draft() {
                            s.capture();
                        }
                    },
                ),
                s.can_submit_draft(),
            ));
        }
        dock = dock.child(input);
    }
    dock
}

fn empty_map() -> El {
    col()
        .key("empty-map")
        .w_full()
        .h_full()
        .center()
        .gap(12.0)
        .padding(32.0)
        .child(
            label("A little space to think.", 30.0, INK)
                .font(TextStyle::new(30.0, 500.0).tracking(-0.7)),
        )
        .child(
            label(
                "Speak a thought. Follow a question. Let the connections take shape.",
                16.0,
                MUTED,
            )
            .max_w(400.0),
        )
}

fn navigation(s: &State) -> El {
    row()
        .key("map-navigation")
        .center_y()
        .gap(4.0)
        .child(
            label(
                format!("{} concepts", s.projection.session.thoughts.len()),
                11.0,
                MUTED,
            )
            .grow(1.0),
        )
        .child(
            button("−", "zoom-out", |s| s.zoom_center(1.0 / 1.2))
                .label("Zoom out")
                .help("Zoom out · Command/Ctrl −"),
        )
        .child(
            button("Actual size", "zoom-reset", |s| {
                s.zoom_center(1.0 / s.camera_zoom.get())
            })
            .help("Reset to 100% · Command/Ctrl 0"),
        )
        .child(
            button("+", "zoom-in", |s| s.zoom_center(1.2))
                .label("Zoom in")
                .help("Zoom in · Command/Ctrl +"),
        )
        .child(
            button("Fit map", "zoom-fit", State::fit_map).help("Show every idea · Command/Ctrl *"),
        )
}

fn status(s: &State) -> El {
    let organization = if s.busy {
        "Organizing…"
    } else if s.organizer_status.starts_with("✦ ") {
        "Map updated"
    } else {
        &s.organizer_status
    };
    let mut status = row().gap(8.0).center_y().child(
        label(format!("{} · {}", s.status, organization), 10.0, MUTED)
            .key("session-status")
            .help(&s.organizer_status)
            .grow(1.0)
            .min_w(0.0)
            .wrap_anywhere(),
    );
    if s.replay.is_none() && !s.read_only {
        if s.art_style != Style::Off && s.gallery.is_working() {
            status = status.child(label("Artwork updating…", 10.0, MUTED).key("art-progress"));
        }
        if s.path.is_some() && s.status.starts_with("Not saved:") {
            status = status.child(button("Retry save", "save", State::save));
        }
        if s.voice_worker.is_some()
            && s.journal.is_some()
            && matches!(s.voice_state, VoiceState::Ready | VoiceState::Unavailable)
            && s.projection.audio.iter().any(|a| {
                !s.projection
                    .transcripts
                    .iter()
                    .any(|(id, _, _)| id == &a.id)
            })
        {
            status = status.child(button(
                "Retry transcript",
                "retry-transcript",
                State::retry_transcript,
            ));
        }
    }
    status.child(session_video_controls(s))
}

pub(super) fn view(s: &State, cx: &mut Cx<State>) -> El {
    if s.replay.is_some() {
        cx.reduced_motion(s.reduced_motion);
    }
    let reduced_motion = cx.is_reduced_motion();
    let empty = s.projection.session.thoughts.is_empty()
        && s.projection.session.categories.is_empty()
        && s.draft.text.trim().is_empty();
    let mut header = row()
        .gap(12.0)
        .center_y()
        .child(label("tacit", 25.0, INK).font(TextStyle::new(25.0, 500.0).tracking(-1.0)))
        .child(sessions_button(s))
        .child(
            label(&s.session_name, 13.0, MUTED)
                .help(&s.session_name)
                .grow(1.0)
                .min_w(0.0)
                .truncate(),
        );
    if s.replay.is_none() {
        if !empty || s.story {
            header = header.child(button(
                if s.story {
                    "Back to map"
                } else {
                    "Read the thread"
                },
                "toggle-story",
                |s| s.story = !s.story,
            ));
        }
        if !s.story && (s.gallery.worker.is_some() || !s.gallery.layers.is_empty()) {
            header = header.child(button(
                if s.art_style == Style::Off {
                    "Show artwork"
                } else {
                    "Hide artwork"
                },
                "art-style",
                |s| s.art_style = s.art_style.toggle(),
            ));
        }
        if !s.read_only && (!s.history.is_empty() || s.queued_direction.is_some()) {
            header = header.child(button("Undo", "undo", State::undo));
        }
    }
    let main = if s.story {
        col()
            .key("story")
            .grow(1.0)
            .min_h(0.0)
            .padding(24.0)
            .gap(16.0)
            .child(label("THE THREAD SO FAR", 11.0, GREEN))
            .child(label(s.projection.session.narrative(), 23.0, INK).selectable(true))
            .on_scroll_offset({
                let offset = s.story_scroll.clone();
                move |p| {
                    offset.set(p.offset);
                }
            })
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
    } else if empty && s.replay.is_none() {
        empty_map()
    } else {
        tree(s, cx)
    };
    let mut center = col()
        .w(0.0)
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .gap(10.0)
        .child(header)
        .child(row().h(0.0).grow(1.0).min_h(0.0).child(main));
    if !s.story && !empty && s.replay.is_none() {
        center = center.child(navigation(s));
    }
    center = center.child(dock(s));
    if s.replay.is_some() {
        center = center.child(replay_controls(s));
    } else if s.art_style != Style::Off
        && s.gallery.worker.is_some()
        && !s.gallery.status.is_empty()
        && !s.gallery.status.starts_with("Art ·")
    {
        // Keep failures visible; Retry is useful only when the mural needs recovery.
        center = center.child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    label(&s.gallery.status, 12.0, MUTED)
                        .grow(1.0)
                        .min_w(0.0)
                        .wrap_anywhere(),
                )
                .child(enabled(
                    button("Retry art", "retry-art", |s| s.gallery.retry()),
                    !s.read_only,
                )),
        );
    }
    center = center.child(status(s));
    let mut root = row()
        .key("root")
        .w_full()
        .h_full()
        .padding(20.0)
        .gap(16.0)
        .fill(PAPER)
        .on_layout(move |s: &mut State, rect| {
            if s.replay.is_none() {
                s.viewport = [rect[2], rect[3]];
                s.reduced_motion = reduced_motion;
            }
        });
    if s.sessions_open && s.viewport[0] >= 1080.0 {
        root = root.child(drawer(s));
    }
    root = root.child(center);
    if s.sessions_open && s.viewport[0] < 1080.0 {
        root = root.child(drawer(s).absolute(20.0, 20.0));
    }
    root
}
