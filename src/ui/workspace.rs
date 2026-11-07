//! Voice-first workspace; the original renderer remains available for legacy replay.
use super::*;
use tacit::camera::Camera;

impl State {
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
                tx.instant();
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
                tx.instant();
            }
            true
        })
        .on_smart_magnify(|s: &mut State, _, _, tx| {
            s.fit_map();
            tx.instant();
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
                .scale(s.camera_zoom.get())
                .scale_motion(rune::Spring::NONE)
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
    row()
        .gap(4.0)
        .padding(4.0)
        .radius(16.0)
        .fill(LINE)
        .child(pill(
            "Brainstorm",
            "mode-brainstorm",
            !s.direction_mode,
            |s| {
                if s.direction_mode {
                    s.toggle_direction();
                }
            },
        ))
        .child(pill(
            "Direct the map",
            "mode-direct",
            s.direction_mode,
            |s| {
                if !s.direction_mode {
                    s.toggle_direction();
                }
            },
        ))
}
fn voice_controls(s: &State) -> El {
    let action = match s.voice_state {
        VoiceState::Ready => pill("●  Start recording", "voice-start", true, |s| {
            s.voice_command(VoiceCommand::Start)
        })
        .fill(GREEN),
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
        .fill(RED),
        VoiceState::Unavailable => pill("Connect microphone", "voice-check", false, |s| {
            s.voice_command(VoiceCommand::Check)
        }),
        _ => label("Preparing audio…", 13.0, MUTED).px(12.0),
    };
    let mut controls = row().gap(8.0).center_y().child(action);
    if s.voice_state == VoiceState::Recording {
        controls = controls.child(label(
            format!("{:02}:{:02}", s.voice_seconds / 60, s.voice_seconds % 60),
            13.0,
            RED,
        ));
        if s.direction_mode {
            controls = controls.child(button("Cancel", "voice-cancel", |s| {
                s.voice_command(VoiceCommand::Cancel)
            }));
        }
    }
    controls
}
fn drawer(s: &State) -> El {
    let mut list = col().gap(6.0).grow(1.0).min_h(0.0).scroll(ScrollSpec {
        stick_bottom: false,
    });
    for (index, entry) in s.session_entries.iter().enumerate() {
        let path = entry.path.clone();
        let current = s.path.as_ref() == Some(&path);
        list = list.child(
            pill(
                &entry.name,
                &format!("session-{index}"),
                current,
                move |s| s.switch_session(path.clone()),
            )
            .w_full(),
        );
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
        .child(
            pill(
                "+  New session",
                "session-create",
                false,
                State::create_session,
            )
            .fill(LINE),
        )
        .child(list)
        .child(
            text_input(&s.session_name_edit, TextStyle::new(14.0, 400.0))
                .key("session-name")
                .h(40.0)
                .color(INK)
                .placeholder("Session name", MUTED)
                .on_edit(|s: &mut State| &mut s.session_name_edit),
        )
        .child(button(
            "Rename current",
            "session-rename",
            State::rename_session,
        ))
        .child(label(
            "Each session keeps its audio, transcript and map history.",
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
    let mut controls = row()
        .gap(6.0)
        .center_y()
        .child(label(s.projection.session.placement_path(t), 11.0, color(t.role)).grow(1.0));
    if s.replay.is_none() && !s.read_only && retired.is_none() {
        controls = controls
            .child(button("Move", "move", State::move_selected))
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
                t.concept.as_deref().unwrap_or("Concept not yet distilled"),
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
                &format!("Follow replacement {id}"),
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
                &format!("{} → {id}", r.label),
                &format!("connection-{id}"),
                move |s| s.selected = Some(id),
            ));
        }
    }
    Some(detail)
}
fn dock(s: &State) -> El {
    let selected = selected(s);
    let mut tabs = row()
        .gap(8.0)
        .center_y()
        .child(button(
            if selected.is_some() {
                "← Transcript"
            } else {
                "Transcript"
            },
            "show-transcript",
            |s| s.selected = None,
        ))
        .child(
            label(
                if selected.is_some() {
                    "Concept & original words"
                } else {
                    "Your words, in order"
                },
                11.0,
                MUTED,
            )
            .grow(1.0),
        );
    if let Some(r) = s.projection.session.retired.last() {
        let id = r.thought.id;
        tabs = tabs.child(button("Prior concepts", "concept-history", move |s| {
            s.selected = Some(id)
        }));
    }
    tabs = tabs
        .child(
            button(
                if s.transcript_following {
                    "Following live"
                } else {
                    "Jump to live"
                },
                "transcript-follow",
                |_| {},
            )
            .on_press_tx(|s: &mut State, tx| {
                if s.replay.is_none() {
                    s.transcript_following = !s.transcript_following;
                    s.selected = None;
                    if s.transcript_following {
                        tx.scroll_to_instant("transcript-scroll", 0.0, 1_000_000_000.0);
                    }
                }
            }),
        )
        .child(button(
            if s.transcript_expanded {
                "Collapse"
            } else {
                "Expand"
            },
            "transcript-expand",
            |s| {
                if s.replay.is_none() {
                    s.transcript_expanded = !s.transcript_expanded;
                }
            },
        ));
    let content = col()
        .key("transcript-scroll")
        .h(if s.transcript_expanded {
            (s.viewport[1] * 0.34).clamp(160.0, 360.0)
        } else if s.selected.is_some() {
            104.0
        } else {
            72.0
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
            if s.replay.is_none() && e.dy > 0.0 {
                s.transcript_following = false;
            }
            false
        })
        .child(selected.unwrap_or_else(|| transcript(s)))
        .scroll(ScrollSpec {
            stick_bottom: s.selected.is_none() && s.transcript_following,
        });
    let input = row()
        .gap(10.0)
        .center_y()
        .child(
            text_input(
                if s.direction_mode {
                    &s.direction_draft
                } else {
                    &s.draft
                },
                TextStyle::new(14.0, 400.0),
            )
            .key("thought-input")
            .grow(1.0)
            .min_w(0.0)
            .h(36.0)
            .color(INK)
            .placeholder(
                if s.direction_mode {
                    "Or type an instruction to reshape the map…"
                } else {
                    "Or type a thought…"
                },
                MUTED,
            )
            .on_edit(|s: &mut State| {
                if s.direction_mode {
                    &mut s.direction_draft
                } else {
                    &mut s.draft
                }
            })
            .on_submit(|s: &mut State, _| s.capture()),
        )
        .child(button(
            if s.direction_mode {
                "Apply direction ↗"
            } else {
                "Add thought ↗"
            },
            "capture",
            State::capture,
        ));
    let mut dock = col()
        .key("composer")
        .gap(8.0)
        .padding(12.0)
        .fill(WHITE)
        .radius(18.0)
        .border(1.0, LINE)
        .child(tabs)
        .child(content);
    if s.replay.is_none() {
        dock = dock.child(input).child(label(
            if s.direction_mode {
                s.direction_status.clone()
            } else {
                s.voice_status.clone()
            },
            11.0,
            MUTED,
        ));
    }
    dock
}

pub(super) fn view(s: &State, cx: &mut Cx<State>) -> El {
    if s.replay.is_some() {
        cx.reduced_motion(s.reduced_motion);
    }
    let reduced_motion = cx.is_reduced_motion();
    let header = row()
        .gap(10.0)
        .center_y()
        .child(sessions_button(s))
        .child(label("tacit", 25.0, INK).font(TextStyle::new(25.0, 500.0).tracking(-1.0)))
        .child(
            label(
                if s.session_name.is_empty() {
                    "A living conversation"
                } else {
                    &s.session_name
                },
                13.0,
                MUTED,
            )
            .grow(1.0)
            .min_w(0.0),
        )
        .child(button(
            if s.story { "Map" } else { "Story" },
            "toggle-story",
            |s| {
                if s.replay.is_none() {
                    s.story = !s.story;
                }
            },
        ))
        .child(button(s.art_style.label(), "art-style", |s| {
            if s.replay.is_none() {
                s.art_style = s.art_style.next();
            }
        }))
        .child(button("Undo", "undo", State::undo));
    let controls = row()
        .gap(12.0)
        .center_y()
        .child(mode_controls(s))
        .child(el().grow(1.0))
        .child(voice_controls(s));
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
    } else {
        tree(s, cx)
    };
    let navigation = row()
        .center_y()
        .gap(4.0)
        .child(
            label(
                format!(
                    "{} concepts · drag to pan · pinch to zoom",
                    s.projection.session.thoughts.len()
                ),
                11.0,
                MUTED,
            )
            .grow(1.0),
        )
        .child(button("−", "zoom-out", |s| s.zoom_center(1.0 / 1.2)))
        .child(button("100%", "zoom-reset", |s| {
            s.zoom_center(1.0 / s.camera_zoom.get())
        }))
        .child(button("+", "zoom-in", |s| s.zoom_center(1.2)))
        .child(button("Fit map", "zoom-fit", State::fit_map));
    let status = row()
        .gap(6.0)
        .center_y()
        .child(
            label(
                format!("{} · {}", s.status, s.organizer_status),
                10.0,
                MUTED,
            )
            .grow(1.0)
            .min_w(0.0),
        )
        .child(if s.gallery.worker.is_some() {
            button("Retry art", "retry-art", |s| s.gallery.retry())
        } else {
            el()
        })
        .child(if s.journal.is_some() && !s.archive_only {
            button(
                if s.replay.is_some() {
                    "Back to live"
                } else {
                    "Replay"
                },
                "replay",
                State::toggle_replay,
            )
        } else {
            el()
        })
        .child(
            if s.replay.is_none()
                && s.projection.audio.iter().any(|a| {
                    !s.projection
                        .transcripts
                        .iter()
                        .any(|(id, _, _)| id == &a.id)
                })
            {
                button(
                    "Retry transcript",
                    "retry-transcript",
                    State::retry_transcript,
                )
            } else {
                el()
            },
        )
        .child(if s.replay.is_none() {
            button("Save", "save", State::save)
        } else {
            el()
        });
    let mut center = col()
        .w(0.0)
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .gap(8.0)
        .child(header);
    if s.replay.is_none() {
        center = center.child(controls);
    }
    center = center
        .child(row().h(0.0).grow(1.0).min_h(0.0).child(main))
        .child(navigation)
        .child(dock(s));
    if s.replay.is_some() {
        center = center.child(replay_controls(s));
    }
    center = center.child(status);
    let mut root = row()
        .key("root")
        .w_full()
        .h_full()
        .padding(14.0)
        .gap(12.0)
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
        root = root.child(drawer(s).absolute(14.0, 14.0));
    }
    root
}
