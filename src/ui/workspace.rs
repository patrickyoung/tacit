//! Voice-first workspace; the original renderer remains available for legacy replay.
use super::*;
use kit::Icon;
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
        self.framing.set(Framing::Held);
        self.read_out_zoom();
    }
    /// Tells the zoom control how large the map is drawn. Gestures zoom without rebuilding
    /// the view, so the control cannot read the camera for itself.
    fn read_out_zoom(&self) {
        self.zoom_readout
            .set(format!("{:.0}%", self.camera_zoom.get() * 100.0));
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
    /// The camera that shows all of `content` in a canvas of `viewport` points.
    fn frame(&self, content: [f32; 2], [w, h]: [f32; 2]) -> Camera {
        let camera = self.camera();
        if ![content[0], content[1], w, h]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
        {
            return camera;
        }
        // A compact canvas trades part of its margin for larger ideas.
        let slack = if w < 900.0 { 24.0 } else { 0.0 };
        let above = |bottom: f32| {
            let mut camera = camera;
            camera.fit(content, [w + slack, h + slack - bottom]);
            camera.pan = camera.pan.map(|p| p - slack / 2.0);
            camera
        };
        // The ideas stay centered. The floating zoom controls sit in the bottom right corner:
        // ideas with room to spare beside them take the whole canvas, and any others stop
        // above the controls.
        let whole = above(0.0);
        if whole.pan[0] >= 200.0 {
            whole
        } else {
            above((32.0 + slack / 2.0).min(h / 2.0))
        }
    }
    pub(super) fn fit_map(&mut self) {
        if self.replay.is_some() {
            return;
        }
        self.set_camera(self.frame(self.map_size.get(), self.map_viewport));
        self.framing.set(Framing::Follow);
    }
    /// Opens the composer on `stem`, ready for the rest of the thought.
    fn start_thought(&mut self, stem: &str) {
        if self.direction_mode && !self.input_mode_locked() {
            self.toggle_direction();
        }
        if self.read_only || self.replay.is_some() || self.direction_mode {
            return;
        }
        self.draft.replace_text(stem);
        self.draft.focused = true;
    }
    /// The drawer's name field holds a name this session does not have yet.
    fn can_rename(&self) -> bool {
        let name = self.session_name_edit.text.trim();
        !self.read_only && !name.is_empty() && name != self.session_name
    }
}

/// Who holds the map's camera.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Framing {
    /// Nobody: the map keeps every idea in view as it grows.
    Follow,
    /// Someone chose this view, so it stays put.
    Held,
    /// A recorded view, which follows again only if it already shows every idea.
    Restored,
}

pub(super) fn canvas(s: &State, sheet: El, size: [f32; 2]) -> El {
    // A recorded camera belongs to the recorded viewport: keep its view centered in this one.
    let shift = if s.replay.is_some() && s.map_viewport.iter().all(|v| *v > 0.0) {
        rune::memo::layout_size("map").map_or([0.0; 2], |live| {
            [
                (live[0] - s.map_viewport[0]) / 2.0,
                (live[1] - s.map_viewport[1]) / 2.0,
            ]
        })
    } else {
        [0.0; 2]
    };
    s.map_size.set(size);
    if s.replay.is_none() {
        if s.framing.get() == Framing::Restored {
            // A view that already shows every idea was never focused on a detail.
            let (camera, [w, h]) = (s.camera(), s.map_viewport);
            let whole = camera.pan.iter().all(|p| *p >= -1.0)
                && camera.pan[0] + size[0] * camera.zoom <= w + 1.0
                && camera.pan[1] + size[1] * camera.zoom <= h + 1.0;
            s.framing.set(if whole {
                Framing::Follow
            } else {
                Framing::Held
            });
        }
        if s.framing.get() == Framing::Follow
            && let Some(viewport) = rune::memo::layout_size("map")
        {
            let camera = s.frame(size, viewport);
            s.camera_zoom.set(camera.zoom);
            s.camera_pan.set(camera.pan);
        }
        s.read_out_zoom();
    }
    let mut map = kit::stage(el().key("map"))
        .help("Drag or scroll to pan. Pinch to zoom. Use Fit map to see every idea.")
        .w(0.0)
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .clip_rounded(kit::CARD)
        .on_layout_tx(|s: &mut State, rect, tx| {
            if s.replay.is_none() {
                s.map_viewport = [rect[2], rect[3]];
                if s.framing.get() == Framing::Follow {
                    s.fit_map();
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
                s.framing.set(Framing::Held);
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
                    s.framing.set(Framing::Held);
                }
                rune::Phase::Up if d.cancelled() => {
                    s.camera_pan.set(s.drag_pan);
                    // A drag that never happened leaves a fitted view following the map.
                    if s.frame(s.map_size.get(), s.map_viewport) == s.camera() {
                        s.framing.set(Framing::Follow);
                    }
                }
                _ => {}
            }
            tx.live_only();
        })
        .child(
            sheet
                .absolute(shift[0], shift[1])
                .origin(0.0, 0.0)
                .live_scale(&s.camera_zoom)
                .live_offset(&s.camera_pan),
        );
    if s.replay.is_some() {
        return map;
    }
    let concepts = s.projection.session.thoughts.len();
    if concepts > 0 {
        map = map.child(
            // The count stays readable when ideas pass beneath it.
            row()
                .center_y()
                .h(24.0)
                .px(10.0)
                .radius(12.0)
                .fill(PAPER.alpha(0.9))
                .pin(Some(10.0), None, None, Some(22.0))
                .inert()
                .child(kit::caption(
                    if concepts == 1 {
                        "1 concept".into()
                    } else {
                        format!("{concepts} concepts")
                    },
                    11.5,
                    500.0,
                    MUTED,
                )),
        );
    }
    map.child(navigation(s))
}

fn mode_controls(s: &State) -> El {
    let mut modes = row()
        .key("input-modes")
        .role("tablist")
        .label("Input mode")
        .focus_group()
        .gap(2.0)
        .padding(3.0)
        .radius(20.0)
        .fill(kit::wash(1.0));
    for (title, key, direction) in [
        ("Brainstorm", "mode-brainstorm", false),
        ("Direct the map", "mode-direct", true),
    ] {
        let active = s.direction_mode == direction;
        modes = modes.child(enabled(
            kit::segment(title, key, active, move |s| {
                if s.direction_mode != direction && !s.input_mode_locked() {
                    s.toggle_direction();
                }
            })
            .role("tab")
            .focus_selected(active)
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
            return kit::notice(message, "voice-progress");
        }
    }
    let action = match s.voice_state {
        VoiceState::Ready => kit::primary(
            Some(Icon::Mic),
            if s.direction_mode {
                "Speak guidance"
            } else {
                "Start recording"
            },
            "voice-start",
            GREEN,
            |s| s.voice_command(VoiceCommand::Start),
        )
        .help("Record your voice. Your typed draft is kept separately."),
        VoiceState::Recording => kit::primary(
            Some(Icon::Stop),
            if s.direction_mode {
                "Stop & direct"
            } else {
                "Finish recording"
            },
            "voice-stop",
            RED,
            |s| s.voice_command(VoiceCommand::Finish),
        ),
        VoiceState::Unavailable => kit::primary(
            Some(Icon::Retry),
            "Reconnect Hex",
            "voice-check",
            INK,
            |s| s.voice_command(VoiceCommand::Check),
        )
        .help("Open Hex with a local model loaded, then retry the connection."),
        state => kit::notice(
            match state {
                VoiceState::Checking => "Connecting to Hex…",
                VoiceState::Starting => "Opening microphone…",
                VoiceState::Finishing => "Finishing transcript…",
                VoiceState::Cancelling => "Discarding guidance…",
                _ => unreachable!(),
            },
            "voice-progress",
        ),
    };
    let mut controls = row().gap(8.0).center_y();
    if s.voice_state == VoiceState::Recording {
        controls = controls.child(
            row()
                .center_y()
                .gap(7.0)
                .px(6.0)
                .child(kit::pulse(RED))
                .child(
                    kit::caption(time(s.voice_seconds * 1000), 13.0, 600.0, RED)
                        .key("recording-timer")
                        .label(format!(
                            "Recording, {} elapsed",
                            time(s.voice_seconds * 1000)
                        )),
                ),
        );
    }
    controls = controls.child(action);
    if s.voice_state == VoiceState::Recording && s.direction_mode {
        controls = controls.child(kit::tool(None, "Cancel", "voice-cancel", |s| {
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
                "Listening. Your words appear above as the map takes shape."
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
        return "Voice is available in the desktop app. You can type a thought instead.";
    }
    &s.voice_status
}
fn drawer(s: &State) -> El {
    let can_switch = !s.voice_in_progress()
        && !s.directing
        && s.queued_direction.is_none()
        && s.pending_recordings.is_empty()
        && !s.video_busy();
    let mut list = col().gap(2.0).grow(1.0).min_h(0.0).scroll(ScrollSpec {
        stick_bottom: false,
    });
    for (index, entry) in s.session_entries.iter().enumerate() {
        let path = entry.path.clone();
        let current = s.path.as_ref() == Some(&path);
        list = list.child(enabled(
            row()
                .key(format!("session-{index}"))
                .role("button")
                .label(&entry.name)
                .focusable(true)
                .w_full()
                .gap(9.0)
                .px(10.0)
                .py(9.0)
                .radius(12.0)
                .fill(if current {
                    GREEN.alpha(0.10)
                } else {
                    Color::TRANSPARENT
                })
                .hover(if current {
                    GREEN.alpha(0.10)
                } else {
                    kit::wash(1.4)
                })
                .press_scale(0.98)
                .on_press(move |s: &mut State| s.switch_session(path.clone()))
                .child(
                    kit::dot(if current { GREEN } else { LINE })
                        .mt(6.0)
                        .min_w(7.0),
                )
                .child(
                    label(&entry.name, 13.0, INK)
                        .grow(1.0)
                        .min_w(0.0)
                        .wrap_anywhere(),
                ),
            can_switch || current,
        ));
    }
    let mut naming = col()
        .gap(6.0)
        .child(kit::eyebrow("NAME THIS SESSION", MUTED).pl(6.0))
        .child(enabled(
            // A press anywhere on the pill lands in its one field.
            row()
                .role("textbox")
                .center_y()
                .h(40.0)
                .px(12.0)
                .radius(14.0)
                .fill(kit::wash(1.0))
                .child(
                    text_input(&s.session_name_edit, TextStyle::new(13.5, 400.0))
                        .key("session-name")
                        .label("Rename this session")
                        .help("Choose a name to keep instead of the automatic conversation title.")
                        .single_line()
                        .grow(1.0)
                        .min_w(0.0)
                        .color(INK)
                        .placeholder("Session name", MUTED)
                        .on_edit(|s: &mut State| &mut s.session_name_edit)
                        .on_submit(|s: &mut State, _| {
                            if s.can_rename() {
                                s.rename_session();
                            }
                        }),
                ),
            !s.read_only,
        ));
    if s.can_rename() {
        // The confirmation appears with the edit it confirms, and leaves once the name is kept.
        naming = naming.child(
            kit::primary(
                None,
                "Rename current",
                "session-rename",
                GREEN,
                State::rename_session,
            )
            .h(38.0)
            .radius(19.0)
            .justify_center(),
        );
    }
    kit::card(col().key("session-drawer"))
        .w(250.0)
        .min_w(250.0)
        .padding(14.0)
        .gap(12.0)
        .transition(rune::Transition::offset(-250.0, 0.0))
        .child(
            row()
                .center_y()
                .pl(6.0)
                .child(kit::caption("Your sessions", 16.0, 600.0, INK).grow(1.0))
                .child(kit::icon_button(
                    Icon::Close,
                    "Close",
                    "sessions-back",
                    |s| s.sessions_open = false,
                )),
        )
        .child(enabled(
            kit::tool(Some(Icon::Plus), "New session", "session-create", |s| {
                s.session_name_edit = TextEdit::new("");
                s.create_session();
            })
            .h(38.0)
            .radius(19.0)
            .fill(kit::wash(1.0)),
            can_switch,
        ))
        .child(list)
        .child(naming)
        .child(
            label(
                if can_switch {
                    "Each session keeps its audio, transcript and map history."
                } else {
                    "Finish recording, transcription, guidance or export before switching sessions."
                },
                11.0,
                MUTED,
            )
            .px(6.0),
        )
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
            .child(kit::tool(
                Some(Icon::Branch),
                "Move to next branch",
                "move",
                State::move_selected,
            ))
            .child(kit::tool(
                Some(Icon::Trash),
                "Remove",
                "remove",
                State::remove_selected,
            ));
        if t.pinned {
            controls = controls.child(kit::tool(
                Some(Icon::Spark),
                "Allow AI to revise",
                "unpin",
                State::unpin_selected,
            ));
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
    // Every other idea this one leads to is a pill that selects it.
    let link = |glyph, title: &str, key: &str, id| {
        kit::tool(glyph, title, key, move |s| s.selected = Some(id)).fill(kit::wash(1.0))
    };
    if let Some(r) = retired {
        detail = detail.child(label(r.reason.clone(), 13.0, MUTED));
        if !r.replaced_by.is_empty() {
            let mut now = row().gap(6.0).flex_wrap();
            for id in &r.replaced_by {
                now = now.child(link(
                    Some(Icon::Spark),
                    &format!("Now: {}", short_label(concept_label(s, *id))),
                    &format!("replacement-{id}"),
                    *id,
                ));
            }
            detail = detail.child(now);
        }
    }
    if retired.is_some() {
        let mut history = row().gap(6.0).flex_wrap();
        for r in s.projection.session.retired.iter().rev() {
            let id = r.thought.id;
            history = history.child(link(
                None,
                r.thought.map_label(),
                &format!("retired-{id}"),
                id,
            ));
        }
        detail = detail
            .child(kit::eyebrow("MAP HISTORY", MUTED).mt(4.0))
            .child(history);
    }
    // Keep semantic cross-links discoverable after removing the right inspector.
    if let Some(i) = &s.projection.session.interpretation {
        let related = || {
            i.relationships
                .iter()
                .filter(|r| r.from == t.id || r.to == t.id)
        };
        if related().next().is_some() {
            let mut links = row().gap(6.0).flex_wrap();
            for r in related() {
                let id = if r.from == t.id { r.to } else { r.from };
                links = links.child(link(
                    Some(Icon::Branch),
                    &format!("{} → {}", r.label, short_label(concept_label(s, id))),
                    &format!("connection-{id}"),
                    id,
                ));
            }
            detail = detail.child(links);
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
    let mut dock = kit::card(col().key("composer")).gap(10.0).padding(14.0);

    if inspecting || has_transcript(s) || s.voice_in_progress() {
        let mut tabs = row()
            .gap(4.0)
            .center_y()
            .min_h(32.0)
            .child(if inspecting {
                kit::tool(Some(Icon::Back), "Transcript", "show-transcript", |s| {
                    s.selected = None
                })
            } else {
                kit::caption("Transcript", 12.0, 600.0, MUTED).px(6.0)
            })
            .child(el().grow(1.0));
        if let Some(r) = s.projection.session.retired.last() {
            let id = r.thought.id;
            tabs = tabs.child(kit::tool(
                None,
                "Prior concepts",
                "concept-history",
                move |s| s.selected = Some(id),
            ));
        }
        if !inspecting && s.replay.is_none() {
            if !s.transcript_following {
                let title = if s.voice_state == VoiceState::Recording {
                    "Jump to live"
                } else {
                    "Jump to latest"
                };
                tabs = tabs.child(
                    row()
                        .key("transcript-follow")
                        .role("button")
                        .label(title)
                        .focusable(true)
                        .center_y()
                        .gap(4.0)
                        .pl(12.0)
                        .pr(8.0)
                        .h(28.0)
                        .radius(14.0)
                        .fill(GREEN.alpha(0.10))
                        .hover(GREEN.alpha(0.18))
                        .press_scale(0.96)
                        .on_press_tx(|s: &mut State, tx| {
                            s.transcript_following = true;
                            tx.scroll_to_instant("transcript-scroll", 0.0, 1_000_000_000.0);
                        })
                        .child(kit::caption(title, 12.0, 600.0, GREEN))
                        .child(kit::icon(Icon::Down, 13.0, GREEN)),
                );
            } else if s.voice_state == VoiceState::Recording {
                tabs = tabs.child(kit::caption("Following live", 11.5, 500.0, GREEN).px(8.0));
            }
        }
        if s.replay.is_none() {
            tabs = tabs.child(
                kit::tool(
                    Some(if s.transcript_expanded {
                        Icon::Down
                    } else {
                        Icon::Up
                    }),
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
            } else if s.viewport[1] < 700.0 {
                // A short window keeps only the latest line, so the map keeps its room.
                32.0
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
        dock = dock.child(
            col()
                .key("dock-detail")
                .gap(4.0)
                // The card opens first, then its transcript arrives in the room it made.
                .enter_delay(0.12)
                .child(tabs)
                .child(content),
        );
    }
    if s.replay.is_some() {
        return dock;
    }

    // One bar for everything you can say to the map: how, typed, or spoken.
    // Keyed, so the bar stays itself when the transcript first appears above it.
    let mut bar = row().key("dock-bar").gap(10.0).items_end();
    if s.read_only {
        bar = bar.child(el().grow(1.0));
    } else {
        let draft = if s.direction_mode {
            &s.direction_draft
        } else {
            &s.draft
        };
        let style = TextStyle::new(14.0, 400.0);
        let field = text_input(draft, style)
            .key("thought-input")
            .label(if s.direction_mode {
                "Type map guidance"
            } else {
                "Type a thought"
            })
            .w_full()
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
            });
        // The pill grows with a long thought for a few lines, then scrolls.
        let mut input = row()
            .gap(6.0)
            .items_end()
            .grow(1.0)
            .min_w(0.0)
            .min_h(40.0)
            .pl(16.0)
            .pr(4.0)
            .py(4.0)
            .radius(20.0)
            .fill(kit::wash(1.0))
            .child(
                rune::auto_grow(field, 1, 4)
                    .key("thought-lines")
                    .w(0.0)
                    .grow(1.0)
                    .min_w(0.0)
                    // One line sits centered beside the round send button.
                    .my((32.0 - style.line_px()) / 2.0),
            );
        if !draft.text.trim().is_empty() {
            input = input.child(enabled(
                kit::round(
                    Icon::Send,
                    if s.direction_mode {
                        "Apply guidance"
                    } else {
                        "Add thought"
                    },
                    "capture",
                    INK,
                    |s| {
                        if s.can_submit_draft() {
                            s.capture();
                        }
                    },
                )
                .help("Enter"),
                s.can_submit_draft(),
            ));
        }
        bar = bar.child(mode_controls(s)).child(input);
    }
    dock = dock.child(bar.child(voice_controls(s)));
    if !s.read_only {
        dock = dock.child(
            label(voice_feedback(s), 11.5, MUTED)
                .key("voice-feedback")
                .px(6.0)
                .wrap_anywhere(),
        );
    }
    dock
}

/// The opening words of a thought in each of the map's four lenses.
pub(super) const STARTERS: [(Role, &str, &str); 4] = [
    (Role::Observation, "starter-noticing", "I keep noticing "),
    (Role::Question, "starter-wondering", "Why do we "),
    (Role::Tension, "starter-tension", "But what about "),
    (Role::Possibility, "starter-possibility", "What if "),
];

fn empty_map(s: &State) -> El {
    // Three ideas finding each other: the whole product in one small drawing.
    let mark = Shape::new()
        .view_box(120.0, 84.0)
        .stroke(
            vec![[26.0, 60.0], [42.0, 34.0], [60.0, 24.0]],
            2.0,
            GREEN.alpha(0.45),
        )
        .stroke(
            vec![[60.0, 24.0], [80.0, 40.0], [96.0, 62.0]],
            2.0,
            GOLD.alpha(0.5),
        )
        .stroke(vec![[26.0, 60.0], [96.0, 62.0]], 2.0, BLUE.alpha(0.3));
    let stage = kit::stage(col().key("empty-map"))
        .w_full()
        .h_full()
        .center()
        .gap(12.0)
        .padding(32.0)
        .child(
            el().size(120.0, 84.0)
                .child(rune::canvas(mark).size(120.0, 84.0))
                .child(
                    kit::dot(GREEN)
                        .size(18.0, 18.0)
                        .radius(9.0)
                        .absolute(17.0, 51.0),
                )
                .child(
                    kit::dot(GOLD)
                        .size(22.0, 22.0)
                        .radius(11.0)
                        .absolute(49.0, 13.0),
                )
                .child(
                    kit::dot(BLUE)
                        .size(14.0, 14.0)
                        .radius(7.0)
                        .absolute(89.0, 55.0),
                ),
        )
        .child(
            label("A little space to think.", 30.0, INK)
                .font(TextStyle::new(30.0, 600.0).tracking(-0.8)),
        )
        .child(
            // Each line is its own label, so both sit centered beneath the heading.
            col()
                .items_center()
                .gap(2.0)
                .child(label("Speak a thought. Follow a question.", 15.0, MUTED))
                .child(label("Let the connections take shape.", 15.0, MUTED)),
        );
    // A recording or queued guidance holds its mode, and then a starter could not open a thought.
    if s.read_only || s.direction_mode && s.input_mode_locked() {
        return stage;
    }
    let mut starters = row()
        .key("starters")
        .label("Ways to begin")
        .flex_wrap()
        .justify_center()
        .gap(8.0)
        .mt(12.0);
    for (role, key, stem) in STARTERS {
        starters = starters.child(
            kit::seed(
                color(role),
                &format!("{}…", stem.trim_end()),
                key,
                move |s| s.start_thought(stem),
            )
            .help(role.label()),
        );
    }
    stage.child(starters)
}

fn navigation(s: &State) -> El {
    kit::card(row().key("map-navigation"))
        .pin(None, None, Some(12.0), Some(12.0))
        .center_y()
        .gap(2.0)
        .padding(4.0)
        .radius(20.0)
        .child(
            kit::icon_button(Icon::Minus, "Zoom out", "zoom-out", |s| {
                s.zoom_center(1.0 / 1.2)
            })
            .help("Zoom out · Command/Ctrl −"),
        )
        .child(
            // The zoom itself, which is also the way back to actual size.
            kit::readout(&s.zoom_readout, "100%", "Actual size", "zoom-reset", |s| {
                s.zoom_center(1.0 / s.camera_zoom.get())
            })
            .help("Reset to 100% · Command/Ctrl 0"),
        )
        .child(
            kit::icon_button(Icon::Plus, "Zoom in", "zoom-in", |s| s.zoom_center(1.2))
                .help("Zoom in · Command/Ctrl +"),
        )
        .child(el().w(1.0).h(16.0).mx(4.0).fill(LINE))
        .child(
            kit::icon_button(Icon::Fit, "Fit map", "zoom-fit", State::fit_map)
                .help("Show every idea · Command/Ctrl *"),
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
    let unsaved = s.status.starts_with("Not saved:");
    let mut status = row()
        .key("status-bar")
        .gap(8.0)
        .center_y()
        .min_h(32.0)
        .pl(8.0)
        .child(kit::dot(if unsaved {
            RED
        } else if s.busy {
            GOLD
        } else {
            GREEN
        }))
        .child(
            label(
                if organization.is_empty() {
                    s.status.clone()
                } else {
                    format!("{} · {}", s.status, organization)
                },
                11.0,
                MUTED,
            )
            .key("session-status")
            .help(&s.organizer_status)
            .grow(1.0)
            .min_w(0.0)
            .wrap_anywhere(),
        );
    if s.replay.is_none() && !s.read_only {
        if s.path.is_some() && unsaved {
            status = status.child(kit::tool(
                Some(Icon::Retry),
                "Retry save",
                "save",
                State::save,
            ));
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
            status = status.child(kit::tool(
                Some(Icon::Retry),
                "Retry transcript",
                "retry-transcript",
                State::retry_transcript,
            ));
        }
    }
    status.child(session_video_controls(s).key("session-video"))
}

/// Where every screen starts: the mark, the way to the other sessions, and this one's name.
fn header(s: &State) -> El {
    row()
        .gap(6.0)
        .center_y()
        .min_h(36.0)
        .pl(6.0)
        .child(kit::brand())
        .child(sessions_button(s))
        .child(
            label(&s.session_name, 13.0, MUTED)
                .help(&s.session_name)
                .grow(1.0)
                .min_w(0.0)
                .ml(4.0)
                .truncate(),
        )
}

/// Artwork is made when someone asks for it. The header offers the ask while the conversation
/// holds something the mural does not show, says when a scene is on its way, and can put the
/// mural out of sight.
fn artwork(s: &State, mut header: El) -> El {
    let shown = s.art_style != Style::Off;
    let mural = s.gallery.has_mural();
    if shown && s.gallery.is_working() {
        header = header.child(kit::progress("Composing artwork…", "art-progress"));
    } else if (shown || !mural)
        && !s.read_only
        && s.gallery.worker.is_some()
        && s.gallery.is_behind()
    {
        header = header.child(
            kit::tool(
                Some(Icon::Spark),
                if mural {
                    "Update artwork"
                } else {
                    "Create artwork"
                },
                "art-compose",
                |s| {
                    s.art_style = Style::Ink;
                    s.gallery.compose();
                },
            )
            .help(if mural {
                "Add what has been said since to the mural"
            } else {
                "Draw this conversation as a mural"
            }),
        );
    }
    if !s.gallery.layers.is_empty() {
        header = header.child(kit::tool(
            Some(Icon::Image),
            if shown {
                "Hide artwork"
            } else {
                "Show artwork"
            },
            "art-style",
            |s| s.art_style = s.art_style.toggle(),
        ));
    }
    header
}

/// The window around a screen's column, with the sessions drawer beside it or over it.
fn window(s: &State, cx: &mut Cx<State>, center: El) -> El {
    let reduced_motion = cx.is_reduced_motion();
    // The drawer belongs to this window, whatever size a replayed session was recorded at.
    let roomy = cx.layout_size("root").map_or(s.viewport[0], |size| size[0]) >= 1080.0;
    let mut root = row()
        .key("root")
        .w_full()
        .h_full()
        .padding(16.0)
        .gap(14.0)
        .fill(PAPER)
        .on_layout(move |s: &mut State, rect| {
            if s.replay.is_none() {
                s.viewport = [rect[2], rect[3]];
                s.reduced_motion = reduced_motion;
            }
        });
    if s.sessions_open && roomy {
        root = root.child(drawer(s).h_full());
    }
    root = root.child(center);
    if s.sessions_open && !roomy {
        // Too narrow to share the window: the drawer floats over a dimmed workspace, and a
        // press outside it or Escape puts it away.
        root = root
            .child(
                el().key("sessions-scrim")
                    .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                    .fill(INK.alpha(0.16))
                    .inert()
                    .transition(rune::Transition::opacity()),
            )
            .child(
                drawer(s)
                    .pin(Some(16.0), Some(16.0), None, Some(16.0))
                    .shadow(24.0, 60.0, INK.alpha(0.22))
                    .modal(true, |s: &mut State| s.sessions_open = false),
            );
    }
    root
}

/// A saved session's original pixels, framed the way its map would be.
pub(super) fn video(s: &State, cx: &mut Cx<State>) -> El {
    let picture = if let Some((_, image)) = &s.pixel_image {
        el().pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
            .w_full()
            .h(0.0)
            .grow(1.0)
            .min_h(0.0)
    } else {
        col()
            .key("video-gap")
            .w_full()
            .grow(1.0)
            .center()
            .gap(10.0)
            .padding(24.0)
            .child(kit::icon(Icon::Film, 28.0, MUTED.alpha(0.7)))
            .child(label(
                "No original pixels were captured at this time.",
                14.0,
                MUTED,
            ))
    };
    let center = col()
        .key("workspace")
        .w(0.0)
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .gap(12.0)
        .child(
            header(s).child(
                row()
                    .center_y()
                    .gap(7.0)
                    .pr(8.0)
                    .child(kit::icon(Icon::Film, 16.0, GREEN))
                    .child(kit::eyebrow("SESSION VIDEO", GREEN)),
            ),
        )
        .child(
            kit::stage(col())
                .w_full()
                .h(0.0)
                .grow(1.0)
                .min_h(0.0)
                .clip_rounded(kit::CARD)
                .child(picture),
        )
        .child(replay_controls(s))
        .child(status(s));
    window(s, cx, center)
}

pub(super) fn view(s: &State, cx: &mut Cx<State>) -> El {
    if s.replay.is_some() {
        cx.reduced_motion(s.reduced_motion);
    }
    let empty = s.projection.session.thoughts.is_empty()
        && s.projection.session.categories.is_empty()
        && s.draft.text.trim().is_empty();
    let mut header = header(s);
    if s.replay.is_none() {
        // The tools that come and go most sit furthest left, so the rest keep still for them.
        if !s.story {
            header = artwork(s, header);
        }
        if !s.read_only && (!s.history.is_empty() || s.queued_direction.is_some()) {
            header = header.child(kit::tool(Some(Icon::Undo), "Undo", "undo", State::undo));
        }
        if !empty || s.story {
            header = header.child(kit::tool(
                Some(if s.story { Icon::Map } else { Icon::Thread }),
                if s.story {
                    "Back to map"
                } else {
                    "Read the thread"
                },
                "toggle-story",
                |s| s.story = !s.story,
            ));
        }
    }
    let main = if s.story {
        kit::stage(col().key("story"))
            .grow(1.0)
            .min_h(0.0)
            .px(36.0)
            .py(30.0)
            .gap(14.0)
            .child(kit::eyebrow("THE THREAD SO FAR", GREEN))
            .child(
                label(s.projection.session.narrative(), 21.0, INK)
                    .max_w(760.0)
                    .selectable(true),
            )
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
        empty_map(s)
    } else {
        tree(s, cx)
    };
    // Keyed, so the workspace slides aside for the drawer instead of being rebuilt beside it.
    let mut center = col()
        .key("workspace")
        .w(0.0)
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .gap(12.0)
        .child(header)
        .child(row().h(0.0).grow(1.0).min_h(0.0).child(main))
        .child(dock(s));
    if s.replay.is_some() {
        center = center.child(replay_controls(s));
    } else if s.art_style != Style::Off
        && s.gallery.worker.is_some()
        && !s.gallery.status.is_empty()
        && !s.gallery.status.starts_with("Art ·")
    {
        // Nothing tries again by itself, so a failure stays until someone retries it or puts
        // it away.
        center = center.child(
            row()
                .key("art-notice")
                .gap(4.0)
                .center_y()
                .pl(14.0)
                .pr(6.0)
                .py(4.0)
                .radius(20.0)
                .fill(GOLD.alpha(0.09))
                .child(
                    label(&s.gallery.status, 12.0, INK)
                        .grow(1.0)
                        .min_w(0.0)
                        .mr(4.0)
                        .wrap_anywhere(),
                )
                .child(enabled(
                    kit::tool(Some(Icon::Retry), "Retry art", "retry-art", |s| {
                        s.gallery.compose()
                    }),
                    !s.read_only,
                ))
                .child(kit::icon_button(
                    Icon::Close,
                    "Dismiss",
                    "art-dismiss",
                    |s| s.gallery.status.clear(),
                )),
        );
    }
    center = center.child(status(s));
    window(s, cx, center)
}
