//! A small visual vocabulary, selected semantically and rendered as hand-drawn ink.
//! Animation is event-driven: unchanged concepts never restart their drawing.
use crate::model::{Role, Session};
use rune::{Color, LineCap, Shape};
use serde::{Deserialize, Serialize};

pub use crate::model::Motif;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Style {
    #[default]
    Pastel,
    Chalk,
    Marker,
    Off,
}
impl Style {
    pub fn next(self) -> Self {
        match self {
            Self::Pastel => Self::Chalk,
            Self::Chalk => Self::Marker,
            Self::Marker => Self::Off,
            Self::Off => Self::Pastel,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Pastel => "Art · pastel",
            Self::Chalk => "Art · chalk",
            Self::Marker => "Art · marker",
            Self::Off => "Art · off",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Concept {
    pub key: String,
    pub motif: Motif,
    pub slot: usize,
}
pub fn concepts(session: &Session) -> Vec<Concept> {
    if session.categories.is_empty() {
        Role::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(slot, role)| {
                let words = session
                    .thoughts
                    .iter()
                    .filter(|t| t.role == role)
                    .map(|t| t.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                (!words.is_empty()).then(|| Concept {
                    key: format!("lens-{slot}"),
                    motif: Motif::infer("", &words),
                    slot,
                })
            })
            .collect()
    } else {
        session
            .categories
            .iter()
            .filter(|c| c.parent.is_none())
            .enumerate()
            .filter_map(|(slot, c)| {
                let words = session
                    .thoughts
                    .iter()
                    .filter(|t| session.within_category(t, &c.id))
                    .map(|t| t.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                (!words.is_empty()).then(|| Concept {
                    key: format!("theme-{}", c.id),
                    motif: c.motif.unwrap_or_else(|| Motif::infer(&c.name, &words)),
                    slot,
                })
            })
            .collect()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub concept: Concept,
    pub serial: u64,
    born: f64,
    retired: Option<f64>,
}
impl Layer {
    pub fn reveal(&self, now: f64) -> f32 {
        ((now - self.born) / 2.2).clamp(0.0, 1.0) as f32
    }
    pub fn opacity(&self, now: f64) -> f32 {
        self.retired
            .map_or(1.0, |t| 1.0 - ((now - t) / 1.4).clamp(0.0, 1.0) as f32)
    }
    pub fn retiring(&self) -> bool {
        self.retired.is_some()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Garden {
    pub layers: Vec<Layer>,
    desired: Vec<Concept>,
    pending: Option<(Vec<Concept>, f64)>,
    serial: u64,
    pub now: f64,
}
impl Garden {
    pub fn settled(session: &Session) -> Self {
        let mut g = Self::default();
        g.update(session, -5.0);
        g.update(session, -4.0);
        g.update(session, 0.0);
        g
    }
    /// Returns a repaint deadline only while waiting for a stable change or animating.
    pub fn update(&mut self, session: &Session, now: f64) -> Option<f64> {
        self.now = now;
        let next = concepts(session);
        if next != self.desired {
            if self.pending.as_ref().is_none_or(|(p, _)| *p != next) {
                self.pending = Some((next.clone(), now));
            }
            if self.pending.as_ref().is_some_and(|(_, t)| now - t >= 0.65) {
                for layer in &mut self.layers {
                    if layer.retired.is_none() {
                        if let Some(c) = next
                            .iter()
                            .find(|c| c.key == layer.concept.key && c.motif == layer.concept.motif)
                        {
                            layer.concept.slot = c.slot;
                        } else {
                            layer.retired = Some(now);
                        }
                    }
                }
                for c in &next {
                    if !self.layers.iter().any(|l| {
                        l.retired.is_none() && l.concept.key == c.key && l.concept.motif == c.motif
                    }) {
                        self.serial += 1;
                        self.layers.push(Layer {
                            concept: c.clone(),
                            serial: self.serial,
                            born: now,
                            retired: None,
                        });
                    }
                }
                self.desired = next;
                self.pending = None;
            }
        } else {
            self.pending = None;
        }
        self.layers.retain(|l| l.opacity(now) > 0.0);
        if self
            .layers
            .iter()
            .any(|l| l.reveal(now) < 1.0 || l.retired.is_some())
        {
            Some(1.0 / 30.0)
        } else {
            self.pending
                .as_ref()
                .map(|(_, t)| (t + 0.65 - now).max(0.001))
        }
    }
}

type Stroke = Vec<[f32; 2]>;
fn arc(x: f32, y: f32, rx: f32, ry: f32, start: f32, end: f32) -> Stroke {
    (0..=40)
        .map(|i| {
            let a = start + (end - start) * i as f32 / 40.0;
            [x + rx * a.cos(), y + ry * a.sin()]
        })
        .collect()
}
fn circle(x: f32, y: f32, r: f32) -> Stroke {
    arc(x, y, r, r, 0.0, std::f32::consts::TAU)
}
fn strokes(motif: Motif) -> Vec<Stroke> {
    use Motif::*;
    let mut s = match motif {
        Clock => vec![
            circle(50., 49., 32.),
            vec![[50., 26.], [49., 50.], [67., 59.]],
            vec![[28., 12.], [18., 21.]],
            vec![[72., 12.], [82., 21.]],
            vec![[31., 79.], [26., 88.]],
            vec![[68., 79.], [74., 87.]],
        ],
        Compass => vec![
            circle(50., 50., 33.),
            vec![[62., 27.], [56., 57.], [36., 74.], [43., 43.], [62., 27.]],
            vec![[50., 8.], [50., 15.]],
            vec![[85., 50.], [92., 50.]],
            vec![[50., 85.], [50., 92.]],
            vec![[8., 50.], [15., 50.]],
        ],
        People => vec![
            circle(50., 28., 10.),
            arc(
                50.,
                61.,
                19.,
                20.,
                std::f32::consts::PI,
                std::f32::consts::TAU,
            ),
            circle(20., 42., 8.),
            arc(
                20.,
                72.,
                14.,
                17.,
                std::f32::consts::PI,
                std::f32::consts::TAU,
            ),
            circle(80., 42., 8.),
            arc(
                80.,
                72.,
                14.,
                17.,
                std::f32::consts::PI,
                std::f32::consts::TAU,
            ),
            arc(50., 66., 37., 24., 0.3, 2.8),
        ],
        Sprout => vec![
            vec![[49., 87.], [47., 65.], [50., 47.], [57., 26.]],
            arc(38., 48., 14., 8., 0., std::f32::consts::TAU),
            arc(64., 33., 14., 8., 0., std::f32::consts::TAU),
            vec![[22., 87.], [48., 84.], [79., 87.]],
            vec![[34., 94.], [59., 94.]],
        ],
        Bridge => vec![
            vec![[9., 75.], [9., 34.], [18., 34.], [18., 75.]],
            vec![[82., 75.], [82., 34.], [91., 34.], [91., 75.]],
            arc(50., 34., 34., 25., 0., std::f32::consts::PI),
            vec![[5., 70.], [95., 70.]],
            vec![[28., 53.], [28., 69.]],
            vec![[50., 59.], [50., 69.]],
            vec![[72., 53.], [72., 69.]],
        ],
        Mountain => vec![
            vec![[7., 82.], [40., 22.], [70., 82.]],
            vec![[55., 55.], [69., 31.], [95., 82.]],
            vec![[29., 42.], [39., 47.], [48., 39.]],
            vec![[40., 22.], [40., 8.], [58., 12.], [40., 17.]],
            vec![[12., 89.], [87., 89.]],
        ],
        Book => vec![
            vec![
                [50., 29.],
                [32., 20.],
                [10., 22.],
                [10., 77.],
                [30., 75.],
                [50., 85.],
                [70., 75.],
                [91., 77.],
                [91., 22.],
                [70., 20.],
                [50., 29.],
                [50., 85.],
            ],
            vec![[19., 36.], [32., 35.], [41., 39.]],
            vec![[19., 49.], [32., 48.], [41., 52.]],
            vec![[60., 39.], [73., 35.], [82., 36.]],
            vec![[60., 52.], [73., 48.], [82., 49.]],
        ],
        Bulb => vec![
            arc(50., 40., 25., 28., 2.4, 7.0),
            vec![[31., 59.], [38., 71.], [61., 71.], [69., 59.]],
            vec![[38., 79.], [61., 79.]],
            arc(50., 84., 10., 7., 0., std::f32::consts::PI),
            vec![[44., 69.], [42., 44.], [50., 52.], [58., 44.], [55., 69.]],
            vec![[9., 36.], [17., 38.]],
            vec![[83., 38.], [92., 36.]],
            vec![[21., 10.], [27., 18.]],
            vec![[73., 18.], [80., 10.]],
        ],
        Waves => (0..4)
            .map(|j| {
                (0..=50)
                    .map(|i| {
                        let x = 8. + i as f32 * 1.65;
                        [x, 30. + j as f32 * 14. + (x * 0.08 + j as f32).sin() * 7.]
                    })
                    .collect()
            })
            .collect(),
        Gears => vec![
            circle(40., 43., 23.),
            circle(40., 43., 8.),
            circle(72., 70., 15.),
            circle(72., 70., 5.),
            vec![[40., 12.], [40., 19.]],
            vec![[40., 67.], [40., 74.]],
            vec![[9., 43.], [16., 43.]],
            vec![[64., 43.], [72., 43.]],
            vec![[18., 21.], [24., 27.]],
            vec![[58., 61.], [63., 66.]],
            vec![[18., 65.], [24., 59.]],
            vec![[57., 27.], [63., 21.]],
        ],
        Shield => vec![
            vec![
                [50., 12.],
                [81., 25.],
                [79., 58.],
                [68., 76.],
                [50., 89.],
                [32., 76.],
                [21., 58.],
                [19., 25.],
                [50., 12.],
            ],
            vec![[34., 49.], [46., 62.], [69., 36.]],
            arc(50., 52., 45., 45., 0.2, 1.0),
        ],
        Telescope => vec![
            vec![[18., 46.], [70., 18.], [83., 42.], [31., 69.], [18., 46.]],
            vec![[65., 21.], [78., 45.]],
            vec![[25., 60.], [13., 67.], [9., 58.], [21., 51.]],
            vec![[51., 59.], [51., 72.], [32., 92.]],
            vec![[51., 72.], [72., 92.]],
            vec![[51., 72.], [51., 94.]],
            vec![[87., 10.], [87., 20.]],
            vec![[82., 15.], [92., 15.]],
        ],
    };
    // Small, deliberately imperfect marks make a composition rather than a stock icon.
    s.push(vec![[7., 87.], [10., 90.], [14., 85.]]);
    s
}
/// Reveal by total pen distance, including the fractional final segment.
pub fn draw(
    motif: Motif,
    style: Style,
    ink: Color,
    accent: Color,
    size: f32,
    progress: f32,
) -> Shape {
    let paths = strokes(motif);
    let distance =
        |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
    let total: f32 = paths
        .iter()
        .flat_map(|p| p.windows(2))
        .map(|p| distance(p[0], p[1]))
        .sum();
    let mut remaining = total * progress.clamp(0., 1.);
    let mut shape = Shape::new();
    if style == Style::Off {
        return shape;
    }
    for (i, path) in paths.iter().enumerate() {
        for (j, pair) in path.windows(2).enumerate() {
            let length = distance(pair[0], pair[1]);
            if remaining <= 0. {
                return shape;
            }
            let amount = (remaining / length.max(0.001)).min(1.);
            let mut a = pair[0];
            let mut b = [
                a[0] + (pair[1][0] - a[0]) * amount,
                a[1] + (pair[1][1] - a[1]) * amount,
            ];
            let wobble = |p: [f32; 2]| {
                [
                    (p[0] + (p[1] * 0.47 + i as f32).sin() * 0.55) * size / 100.,
                    (p[1] + (p[0] * 0.32).sin() * 0.5) * size / 100.,
                ]
            };
            a = wobble(a);
            b = wobble(b);
            let pigment = if i % 3 == 1 { accent } else { ink };
            let thickness = if style == Style::Marker { 3.6 } else { 2.3 };
            if style != Style::Marker {
                shape = shape.line(a, b, 7.0 * size / 100., LineCap::Round, pigment.alpha(0.10));
                let offset = if style == Style::Chalk { 1.2 } else { 0.7 } * size / 100.;
                shape = shape.line(
                    [a[0] + offset, a[1] - offset],
                    [b[0] + offset, b[1] - offset],
                    0.7 * size / 100.,
                    LineCap::Round,
                    pigment.alpha(0.25),
                );
            }
            shape = shape.line(
                a,
                b,
                thickness * size / 100. * (1. + (j as f32 * 1.7).sin() * 0.12),
                LineCap::Round,
                pigment.alpha(if style == Style::Marker { 0.65 } else { 0.46 }),
            );
            remaining -= length;
        }
    }
    shape
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strokes_reveal_progressively_and_texture_is_deterministic() {
        let ink = Color {
            r: 0.03,
            g: 0.3,
            b: 0.4,
            a: 1.0,
        };
        let accent = Color {
            r: 0.8,
            g: 0.3,
            b: 0.05,
            a: 1.0,
        };
        assert_eq!(
            draw(Motif::Clock, Style::Pastel, ink, accent, 100., 0.),
            Shape::new()
        );
        let half = draw(Motif::Clock, Style::Pastel, ink, accent, 100., 0.5);
        let full = draw(Motif::Clock, Style::Pastel, ink, accent, 100., 1.);
        assert_ne!(half, full);
        assert_eq!(
            full,
            draw(Motif::Clock, Style::Pastel, ink, accent, 100., 1.)
        );
        assert_eq!(
            draw(Motif::Clock, Style::Off, ink, accent, 100., 1.),
            Shape::new()
        );
    }
    #[test]
    fn name_has_priority_and_unchanged_concepts_do_not_redraw() {
        assert_eq!(
            Motif::infer("Time to think", "people people people"),
            Motif::Clock
        );
        let s = Session::demo();
        let mut g = Garden::settled(&s);
        let ids: Vec<_> = g.layers.iter().map(|l| l.serial).collect();
        assert_eq!(g.update(&s, 20.), None);
        assert_eq!(ids, g.layers.iter().map(|l| l.serial).collect::<Vec<_>>());
    }
    #[test]
    fn replacements_overlap_then_retire_and_idle_stops() {
        let mut s = Session::default();
        s.add("time calendar");
        let mut g = Garden::settled(&s);
        s.thoughts[0].text = "people together".into();
        g.update(&s, 1.);
        assert_eq!(g.layers.len(), 1);
        g.update(&s, 1.7);
        assert_eq!(g.layers.len(), 2);
        assert!(g.layers[0].retiring());
        assert_eq!(g.layers[1].reveal(1.7), 0.);
        assert_eq!(g.update(&s, 5.), None);
        assert_eq!(g.layers.len(), 1);
        assert_eq!(g.layers[0].concept.motif, Motif::People);
    }
    #[test]
    fn brief_changes_are_coalesced_and_removed_themes_fade() {
        let mut s = Session::default();
        s.add("time");
        let mut g = Garden::settled(&s);
        s.thoughts[0].text = "people".into();
        g.update(&s, 1.);
        s.thoughts[0].text = "time".into();
        assert_eq!(g.update(&s, 1.2), None);
        assert_eq!(g.layers.len(), 1);
        s.thoughts.clear();
        g.update(&s, 2.);
        g.update(&s, 2.7);
        assert!(g.layers[0].retiring());
        g.update(&s, 5.);
        assert!(g.layers.is_empty());
    }
}
