//! Rune presentation adapter. Portable session events do not depend on this layout.
use crate::{
    art::{Garden, Style},
    model::Session,
};
use crate::{
    session_event::SessionEvent,
    timeline::{Event, RENDERER, SessionArtwork},
};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Artwork {
    pub key: String,
    pub fingerprint: String,
    pub slot: usize,
    pub asset: String,
    pub born: f64,
    pub retired: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub session: Session,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_time: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session_title: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub direction_mode: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub direction_draft: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub direction_status: String,
    pub draft: String,
    pub selected: Option<u64>,
    pub style: Style,
    pub story: bool,
    pub garden: Garden,
    pub artwork: Vec<Artwork>,
    pub busy: bool,
    pub settled_at: f64,
    pub organizer_status: String,
    pub voice_status: String,
    pub art_status: String,
    pub map_scroll: [f32; 2],
    pub story_scroll: [f32; 2],
    pub inspector_scroll: [f32; 2],
    pub viewport: [f32; 2],
    #[serde(default, skip_serializing_if = "zero_size")]
    pub map_viewport: [f32; 2],
    pub reduced_motion: bool,
}
fn zero_size(size: &[f32; 2]) -> bool {
    *size == [0.0; 2]
}
impl Default for Scene {
    fn default() -> Self {
        Self {
            session: Session::default(),
            render_time: None,
            session_title: String::new(),
            direction_mode: false,
            direction_draft: String::new(),
            direction_status: String::new(),
            draft: String::new(),
            selected: None,
            style: Style::default(),
            story: false,
            garden: Garden::default(),
            artwork: vec![],
            busy: false,
            settled_at: -10.0,
            organizer_status: String::new(),
            voice_status: String::new(),
            art_status: String::new(),
            map_scroll: [0.0; 2],
            story_scroll: [0.0; 2],
            inspector_scroll: [0.0; 2],
            viewport: [1280.0, 860.0],
            map_viewport: [0.0; 2],
            reduced_motion: false,
        }
    }
}

impl Scene {
    pub fn presentation_value(&self) -> std::io::Result<serde_json::Value> {
        let mut value = serde_json::to_value(self).map_err(std::io::Error::other)?;
        value.as_object_mut().unwrap().remove("session");
        value.as_object_mut().unwrap().remove("render_time");
        value.as_object_mut().unwrap().insert(
            "content_hash".into(),
            crate::timeline::content_hash(&self.session)?.into(),
        );
        Ok(value)
    }
}
impl crate::timeline::Projection {
    pub fn replay_scene(&self) -> io::Result<Scene> {
        let presentation = self
            .presentations
            .get(RENDERER)
            .ok_or_else(|| io::Error::other("No recorded Rune presentation at this time"))?;
        if presentation.version != 1 {
            return Err(io::Error::other(
                "Unsupported recorded presentation version",
            ));
        }
        let session = self
            .presented_sessions
            .get(RENDERER)
            .ok_or_else(|| io::Error::other("Missing presented content revision"))?;
        let mut state = presentation.state.clone();
        state
            .as_object_mut()
            .ok_or_else(|| io::Error::other("Malformed presentation"))?
            .insert(
                "session".into(),
                serde_json::to_value(session).map_err(io::Error::other)?,
            );
        let mut scene: Scene = serde_json::from_value(state).map_err(io::Error::other)?;
        scene.render_time = self.frame_times.get(RENDERER).copied();
        if scene.viewport.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(io::Error::other("Invalid recorded viewport"));
        }
        Ok(scene)
    }
    /// Rune-specific adapter. Other renderers read `session`, `artwork`, audio and transcripts directly.
    pub fn scene(&self) -> Scene {
        let mut scene = self
            .presentations
            .get(crate::timeline::RENDERER)
            .filter(|p| p.version == 1)
            .and_then(|p| {
                let mut state = p.state.clone();
                state
                    .as_object_mut()?
                    .insert("session".into(), serde_json::to_value(&self.session).ok()?);
                serde_json::from_value::<Scene>(state).ok()
            })
            .unwrap_or_else(|| {
                // A session authored in another renderer still gets a useful Rune view.
                let mut scene = Scene {
                    garden: Garden::settled(&self.session),
                    ..Scene::default()
                };
                for (slot, category) in self.session.categories.iter().enumerate() {
                    if let Some(art) = self.artwork.iter().rev().find(|a| a.theme == category.id) {
                        scene.artwork.push(Artwork {
                            key: format!("theme-{}", category.id),
                            fingerprint: art.fingerprint.clone(),
                            slot,
                            asset: art.asset.clone(),
                            born: -10.0,
                            retired: None,
                        });
                    }
                }
                scene
            });
        scene.session = self.session.clone();
        scene
    }
}

// Adapter helpers; portable consumers only need Journal::at and Projection::content.
impl crate::timeline::Journal {
    /// Compatibility helper for fixtures/imports. Live edits dispatch SessionChanged directly.
    pub fn scene(&mut self, at_ms: u64, reason: &str, scene: Scene) -> io::Result<()> {
        if scene.session != self.projection.session {
            self.append(
                at_ms,
                Event::SessionChanged {
                    change: SessionEvent::Restored {
                        session: scene.session.clone(),
                        reason: reason.into(),
                    },
                },
            )?;
        }
        self.present_scene(at_ms, scene)
    }
    pub fn present_scene(&mut self, at_ms: u64, scene: Scene) -> io::Result<()> {
        let mut events = Vec::new();
        for a in &scene.artwork {
            let artwork = SessionArtwork {
                theme: a.key.strip_prefix("theme-").unwrap_or(&a.key).into(),
                fingerprint: a.fingerprint.clone(),
                asset: a.asset.clone(),
            };
            if !self.projection.artwork.contains(&artwork) {
                events.push(Event::ArtworkAvailable { artwork });
            }
        }
        let state = scene.presentation_value()?;
        if self
            .projection
            .presentations
            .get(RENDERER)
            .is_none_or(|p| p.state != state)
        {
            events.push(Event::Presentation {
                renderer: RENDERER.into(),
                version: 1,
                state,
            });
        }
        if let Some(time) = scene.render_time
            && (self.projection.frame_times.get(RENDERER) != Some(&time) || !events.is_empty())
        {
            events.push(Event::Frame {
                renderer: RENDERER.into(),
                time,
            });
        }
        if !events.is_empty() {
            self.append(at_ms, Event::Transaction { events })?;
        }
        Ok(())
    }
    /// Strict playback: never synthesize a layout when the recorded track is unavailable.
    pub fn replay_scene_at(&self, at_ms: u64) -> io::Result<Scene> {
        self.at(at_ms)?.replay_scene()
    }
    pub fn first_frame_ms(&self) -> Option<u64> {
        self.entries.iter().find(|e| e.event.leaves().any(|event| matches!(event, Event::Scene { .. }) || matches!(event, Event::Presentation { renderer, .. } if renderer == RENDERER))).map(|e| e.at_ms)
    }
    pub fn validate_replay(&self) -> io::Result<()> {
        self.validate_assets()?;
        let mut projection = crate::timeline::Projection::default();
        let mut frames = 0;
        for entry in &self.entries {
            projection.apply(&entry.event)?;
            if entry.event.leaves().any(|e| {
                matches!(e, Event::Scene { .. })
                    || matches!(e, Event::Presentation { renderer, .. } if renderer == RENDERER)
            }) {
                projection.replay_scene()?;
                frames += 1;
            }
        }
        if frames == 0 {
            return Err(io::Error::other(
                "No compatible recorded presentation; content can be opened in another renderer, but cannot be replayed exactly",
            ));
        }
        Ok(())
    }
    pub fn scene_at(&self, at_ms: u64) -> Scene {
        self.at(at_ms).expect("previously validated events").scene()
    }
}
