//! The workspace's shared visual language: icons, surfaces and controls.
use super::*;

/// Corner radius shared by every card-sized surface.
pub(super) const CARD: f32 = 20.0;

/// A quiet tint for wells and hover states that works on paper and on cards.
pub(super) fn wash(strength: f32) -> Color {
    INK.alpha(0.055 * strength)
}

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Mic,
    Stop,
    Plus,
    Minus,
    Fit,
    Sidebar,
    Play,
    Pause,
    Save,
    Undo,
    Image,
    Thread,
    Map,
    Close,
    Send,
    Up,
    Down,
    Spark,
    Back,
    Trash,
    Branch,
    Folder,
    Retry,
    Rewind,
    Forward,
    Next,
    Film,
}

fn ring(c: [f32; 2], r: f32) -> Path {
    let k = r * 0.5523;
    Path::new()
        .move_to([c[0], c[1] - r])
        .cubic_to([c[0] + k, c[1] - r], [c[0] + r, c[1] - k], [c[0] + r, c[1]])
        .cubic_to([c[0] + r, c[1] + k], [c[0] + k, c[1] + r], [c[0], c[1] + r])
        .cubic_to([c[0] - k, c[1] + r], [c[0] - r, c[1] + k], [c[0] - r, c[1]])
        .cubic_to([c[0] - r, c[1] - k], [c[0] - k, c[1] - r], [c[0], c[1] - r])
        .close()
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let k = r * 0.4477;
    Path::new()
        .move_to([x + r, y])
        .line_to([x + w - r, y])
        .cubic_to([x + w - k, y], [x + w, y + k], [x + w, y + r])
        .line_to([x + w, y + h - r])
        .cubic_to([x + w, y + h - k], [x + w - k, y + h], [x + w - r, y + h])
        .line_to([x + r, y + h])
        .cubic_to([x + k, y + h], [x, y + h - k], [x, y + h - r])
        .line_to([x, y + r])
        .cubic_to([x, y + k], [x + k, y], [x + r, y])
        .close()
}

/// Line icons drawn on a 24-point grid and stretched to `size`.
pub(super) fn glyph(icon: Icon, size: f32, ink: Color) -> Shape {
    let w = (size / 24.0 * 1.9).clamp(1.4, 2.4);
    let cap = LineCap::Round;
    let s = Shape::new().view_box(24.0, 24.0);
    let lines = |s: Shape, sets: &[&[[f32; 2]]]| {
        sets.iter()
            .fold(s, |s, points| s.stroke(points.to_vec(), w, ink))
    };
    match icon {
        Icon::Mic => lines(
            s.stroke_path(&rounded(9.0, 3.0, 6.0, 11.0, 3.0), w, cap, ink)
                .stroke_path(
                    &Path::new()
                        .move_to([5.5, 11.5])
                        .cubic_to([5.5, 15.1], [8.4, 18.0], [12.0, 18.0])
                        .cubic_to([15.6, 18.0], [18.5, 15.1], [18.5, 11.5]),
                    w,
                    cap,
                    ink,
                ),
            &[&[[12.0, 18.0], [12.0, 21.0]]],
        ),
        Icon::Stop => s.fill_path(&rounded(6.5, 6.5, 11.0, 11.0, 2.6), ink),
        Icon::Plus => lines(
            s,
            &[&[[12.0, 5.5], [12.0, 18.5]], &[[5.5, 12.0], [18.5, 12.0]]],
        ),
        Icon::Minus => lines(s, &[&[[5.5, 12.0], [18.5, 12.0]]]),
        Icon::Fit => lines(
            s,
            &[
                &[[4.5, 9.0], [4.5, 4.5], [9.0, 4.5]],
                &[[15.0, 4.5], [19.5, 4.5], [19.5, 9.0]],
                &[[19.5, 15.0], [19.5, 19.5], [15.0, 19.5]],
                &[[9.0, 19.5], [4.5, 19.5], [4.5, 15.0]],
            ],
        ),
        Icon::Sidebar => lines(
            s.stroke_path(&rounded(3.5, 4.5, 17.0, 15.0, 3.2), w, cap, ink),
            &[&[[9.5, 4.5], [9.5, 19.5]]],
        ),
        Icon::Play => s.fill_path(
            &Path::new()
                .move_to([8.0, 5.6])
                .line_to([18.6, 12.0])
                .line_to([8.0, 18.4])
                .close(),
            ink,
        ),
        Icon::Pause => s
            .fill_path(&rounded(7.0, 5.5, 3.6, 13.0, 1.2), ink)
            .fill_path(&rounded(13.4, 5.5, 3.6, 13.0, 1.2), ink),
        Icon::Save => lines(
            s,
            &[
                &[[12.0, 4.0], [12.0, 15.0]],
                &[[7.5, 10.5], [12.0, 15.0], [16.5, 10.5]],
                &[[5.0, 19.5], [19.0, 19.5]],
            ],
        ),
        Icon::Undo => lines(
            s.stroke_path(
                &Path::new()
                    .move_to([4.5, 9.0])
                    .line_to([14.5, 9.0])
                    .cubic_to([17.5, 9.0], [20.0, 11.5], [20.0, 14.5])
                    .cubic_to([20.0, 17.5], [17.5, 20.0], [14.5, 20.0])
                    .line_to([11.0, 20.0]),
                w,
                cap,
                ink,
            ),
            &[&[[9.0, 4.5], [4.5, 9.0], [9.0, 13.5]]],
        ),
        Icon::Image => lines(
            s.stroke_path(&rounded(3.5, 4.5, 17.0, 15.0, 3.2), w, cap, ink)
                .fill_path(&ring([9.0, 9.8], 1.7), ink),
            &[&[[6.0, 19.0], [15.5, 10.5], [20.5, 15.0]]],
        ),
        Icon::Thread => lines(
            s,
            &[
                &[[5.0, 7.0], [19.0, 7.0]],
                &[[5.0, 12.0], [19.0, 12.0]],
                &[[5.0, 17.0], [13.5, 17.0]],
            ],
        ),
        Icon::Map => lines(
            s.fill_path(&ring([12.0, 12.0], 2.8), ink)
                .fill_path(&ring([5.2, 6.0], 2.0), ink)
                .fill_path(&ring([18.8, 6.0], 2.0), ink)
                .fill_path(&ring([12.0, 20.0], 2.0), ink),
            &[
                &[[12.0, 12.0], [5.2, 6.0]],
                &[[12.0, 12.0], [18.8, 6.0]],
                &[[12.0, 12.0], [12.0, 20.0]],
            ],
        ),
        Icon::Close => lines(
            s,
            &[&[[6.5, 6.5], [17.5, 17.5]], &[[17.5, 6.5], [6.5, 17.5]]],
        ),
        Icon::Send => lines(
            s,
            &[
                &[[12.0, 19.0], [12.0, 5.5]],
                &[[6.5, 11.0], [12.0, 5.5], [17.5, 11.0]],
            ],
        ),
        Icon::Up => lines(s, &[&[[6.5, 14.5], [12.0, 9.0], [17.5, 14.5]]]),
        Icon::Down => lines(s, &[&[[6.5, 9.5], [12.0, 15.0], [17.5, 9.5]]]),
        Icon::Spark => s.fill_path(
            &Path::new()
                .move_to([12.0, 2.5])
                .quad_to([12.9, 11.1], [21.5, 12.0])
                .quad_to([12.9, 12.9], [12.0, 21.5])
                .quad_to([11.1, 12.9], [2.5, 12.0])
                .quad_to([11.1, 11.1], [12.0, 2.5])
                .close(),
            ink,
        ),
        Icon::Back => lines(
            s,
            &[
                &[[19.0, 12.0], [5.5, 12.0]],
                &[[11.0, 6.5], [5.5, 12.0], [11.0, 17.5]],
            ],
        ),
        Icon::Trash => lines(
            s,
            &[
                &[[5.0, 7.0], [19.0, 7.0]],
                &[[7.0, 7.0], [8.0, 20.0], [16.0, 20.0], [17.0, 7.0]],
                &[[10.0, 7.0], [10.0, 4.2], [14.0, 4.2], [14.0, 7.0]],
            ],
        ),
        Icon::Branch => lines(
            s,
            &[
                &[[5.0, 5.0], [5.0, 13.0], [8.0, 16.0], [19.0, 16.0]],
                &[[15.0, 12.0], [19.0, 16.0], [15.0, 20.0]],
            ],
        ),
        Icon::Folder => s.stroke_with(
            vec![
                [3.5, 6.5],
                [9.5, 6.5],
                [11.5, 9.0],
                [20.5, 9.0],
                [20.5, 18.5],
                [3.5, 18.5],
            ],
            true,
            w,
            cap,
            ink,
        ),
        Icon::Retry => lines(
            s.stroke_path(
                &Path::new()
                    .move_to([19.0, 12.5])
                    .cubic_to([19.0, 16.4], [15.9, 19.5], [12.0, 19.5])
                    .cubic_to([8.1, 19.5], [5.0, 16.4], [5.0, 12.5])
                    .cubic_to([5.0, 8.6], [8.1, 5.5], [12.0, 5.5])
                    .cubic_to([14.0, 5.5], [15.8, 6.3], [17.1, 7.6]),
                w,
                cap,
                ink,
            ),
            &[&[[17.6, 3.6], [17.6, 8.0], [13.2, 8.0]]],
        ),
        Icon::Rewind => lines(
            s,
            &[
                &[[11.5, 7.0], [6.5, 12.0], [11.5, 17.0]],
                &[[17.5, 7.0], [12.5, 12.0], [17.5, 17.0]],
            ],
        ),
        Icon::Forward => lines(
            s,
            &[
                &[[6.5, 7.0], [11.5, 12.0], [6.5, 17.0]],
                &[[12.5, 7.0], [17.5, 12.0], [12.5, 17.0]],
            ],
        ),
        Icon::Next => lines(
            s.fill_path(
                &Path::new()
                    .move_to([6.0, 6.0])
                    .line_to([15.0, 12.0])
                    .line_to([6.0, 18.0])
                    .close(),
                ink,
            ),
            &[&[[18.0, 6.0], [18.0, 18.0]]],
        ),
        Icon::Film => s
            .stroke_path(&rounded(3.5, 6.5, 12.0, 11.0, 2.6), w, cap, ink)
            .stroke_with(
                vec![[15.5, 10.6], [20.5, 7.6], [20.5, 16.4], [15.5, 13.4]],
                false,
                w,
                cap,
                ink,
            ),
    }
}

pub(super) fn icon(icon: Icon, size: f32, ink: Color) -> El {
    canvas(glyph(icon, size, ink)).size(size, size)
}

/// Single-line text for controls; `label` wraps and is meant for reading.
pub(super) fn caption(content: impl Into<String>, size: f32, weight: f32, color: Color) -> El {
    text(content)
        .font(TextStyle::new(size, weight))
        .color(color)
}

/// Small capitals that name a region.
pub(super) fn eyebrow(content: impl Into<String>, color: Color) -> El {
    text(content)
        .font(TextStyle::new(10.0, 600.0).tracking(0.9))
        .color(color)
}

/// The mark and the name every screen's header opens with.
pub(super) fn brand() -> El {
    row()
        .center_y()
        .gap(8.0)
        .mr(8.0)
        .child(icon(Icon::Map, 22.0, GREEN))
        .child(
            text("tacit")
                .font(TextStyle::new(21.0, 600.0).tracking(-0.8))
                .color(INK),
        )
}

/// The raised paper every panel sits on.
pub(super) fn card(surface: El) -> El {
    surface
        .fill(WHITE)
        .radius(CARD)
        .border(1.0, LINE.alpha(0.55))
        .shadow(1.0, 2.0, INK.alpha(0.04))
        .shadow(10.0, 28.0, INK.alpha(0.06))
}

/// `ink` washed over white paper, as one opaque colour that hides what lies beneath it.
pub(super) fn tint(ink: Color, strength: f32) -> Color {
    let mix = |paper: f32, ink: f32| paper + (ink - paper) * strength;
    Color {
        r: mix(WHITE.r, ink.r),
        g: mix(WHITE.g, ink.g),
        b: mix(WHITE.b, ink.b),
        a: 1.0,
    }
}

/// A thought resting on the map: paper edged in its branch colour. `glow` runs from 0 (at
/// rest) to 1 (selected).
pub(super) fn leaf(surface: El, ink: Color, glow: f32) -> El {
    surface
        .radius(14.0)
        .fill(tint(ink, 0.14 * glow))
        .border(1.0 + 0.5 * glow, ink.alpha(0.22 + 0.68 * glow))
        .shadow(1.0, 2.0, INK.alpha(0.05))
        .shadow(6.0, 16.0, ink.alpha(0.05 + 0.13 * glow))
        .hover(tint(ink, 0.07 + 0.11 * glow))
        .press(tint(ink, 0.18))
}

/// A group of thoughts on the map: paper dyed with its branch colour.
pub(super) fn bough(surface: El, ink: Color) -> El {
    surface
        .radius(14.0)
        .fill(tint(ink, 0.11))
        .border(1.0, ink.alpha(0.2))
}

/// A theme's name on the map: the solid, coloured start of everything beneath it.
pub(super) fn chip(title: impl Into<String>, ink: Color, max_w: f32) -> El {
    row()
        .h(34.0)
        .min_w(88.0)
        .max_w(max_w)
        .px(15.0)
        .center()
        .radius(17.0)
        .fill(ink)
        .shadow(4.0, 12.0, ink.alpha(0.24))
        .child(caption(title, 14.5, 600.0, WHITE).truncate())
}

fn pressable(control: El, key: &str, action: impl Fn(&mut State) + 'static) -> El {
    control
        .key(key)
        .role("button")
        .focusable(true)
        .press_scale(0.96)
        .on_press(action)
}

/// A quiet action: an optional icon and a short title on a pill that tints on hover.
pub(super) fn tool(
    glyph: Option<Icon>,
    title: &str,
    key: &str,
    action: impl Fn(&mut State) + 'static,
) -> El {
    let mut control = pressable(
        row()
            .center_y()
            .gap(6.0)
            .px(if glyph.is_some() { 10.0 } else { 12.0 })
            .pr(12.0)
            .h(32.0)
            .radius(16.0)
            .hover(wash(1.6)),
        key,
        action,
    )
    .label(title);
    if let Some(glyph) = glyph {
        control = control.child(icon(glyph, 15.0, INK));
    }
    control.child(caption(title, 12.5, 500.0, INK))
}

/// A quiet action titled by a value that keeps changing, such as a zoom level. `sample` is
/// its widest reading, which reserves the room; `name` is what assistive technology announces.
pub(super) fn readout(
    value: &rune::LiveText,
    sample: &str,
    name: &str,
    key: &str,
    action: impl Fn(&mut State) + 'static,
) -> El {
    pressable(
        row()
            .center_y()
            .px(12.0)
            .h(32.0)
            .radius(16.0)
            .hover(wash(1.6)),
        key,
        action,
    )
    .label(name)
    .child(
        text(sample)
            // Figures of one width hold the reading still while it counts.
            .font(TextStyle::new(12.5, 500.0).features("'tnum' 1"))
            .color(INK)
            .align(rune::Align::Center)
            .live_text(value),
    )
}

/// A round, icon-only action. `name` is what assistive technology announces.
pub(super) fn icon_button(
    glyph: Icon,
    name: &str,
    key: &str,
    action: impl Fn(&mut State) + 'static,
) -> El {
    pressable(
        row()
            .center()
            .size(32.0, 32.0)
            .radius(16.0)
            .hover(wash(1.6)),
        key,
        action,
    )
    .label(name)
    .child(icon(glyph, 16.0, INK))
}

/// A filled, icon-only action for the end of an input.
pub(super) fn round(
    glyph: Icon,
    name: &str,
    key: &str,
    fill: Color,
    action: impl Fn(&mut State) + 'static,
) -> El {
    pressable(
        row()
            .center()
            .size(32.0, 32.0)
            .radius(16.0)
            .fill(fill)
            .hover(fill.alpha(0.82)),
        key,
        action,
    )
    .label(name)
    .child(icon(glyph, 16.0, WHITE))
}

/// The one loud action in a region: a filled pill with a soft glow in its own colour.
pub(super) fn primary(
    glyph: Option<Icon>,
    title: &str,
    key: &str,
    fill: Color,
    action: impl Fn(&mut State) + 'static,
) -> El {
    let mut control = pressable(
        row()
            .center_y()
            .gap(8.0)
            .px(16.0)
            .h(40.0)
            .radius(20.0)
            .fill(fill)
            .hover(fill.alpha(0.86))
            .shadow(4.0, 14.0, fill.alpha(0.28)),
        key,
        action,
    )
    .label(title);
    if let Some(glyph) = glyph {
        control = control.child(icon(glyph, 16.0, WHITE));
    }
    control.child(caption(title, 13.5, 600.0, WHITE))
}

/// One option of a segmented control; the active one is a raised chip.
pub(super) fn segment(
    title: &str,
    key: &str,
    active: bool,
    action: impl Fn(&mut State) + 'static,
) -> El {
    let mut control = pressable(
        row()
            .center()
            .px(13.0)
            .h(34.0)
            .radius(17.0)
            .fill(if active { WHITE } else { Color::TRANSPARENT })
            .hover(if active { WHITE } else { wash(1.4) }),
        key,
        action,
    )
    .label(title);
    if active {
        control = control.shadow(1.0, 3.0, INK.alpha(0.12));
    }
    control.child(caption(
        title,
        12.5,
        550.0,
        if active { INK } else { MUTED },
    ))
}

/// An invitation to begin: a paper pill marked with the colour of the branch it leads to.
pub(super) fn seed(
    ink: Color,
    title: &str,
    key: &str,
    action: impl Fn(&mut State) + 'static,
) -> El {
    pressable(
        row()
            .center_y()
            .gap(8.0)
            .pl(12.0)
            .pr(14.0)
            .h(36.0)
            .radius(18.0)
            .fill(WHITE)
            .border(1.0, ink.alpha(0.26))
            .shadow(1.0, 2.0, INK.alpha(0.05))
            .shadow(6.0, 16.0, ink.alpha(0.07))
            .hover(tint(ink, 0.1))
            .press(tint(ink, 0.18)),
        key,
        action,
    )
    .label(title)
    .child(dot(ink).size(8.0, 8.0).radius(4.0))
    .child(caption(title, 13.0, 500.0, INK))
}

/// The translucent sheet behind the map, the thread and the empty state.
pub(super) fn stage(surface: El) -> El {
    surface
        .fill(WHITE.alpha(0.5))
        .radius(CARD)
        .border(1.0, LINE.alpha(0.5))
}

/// A small status light.
pub(super) fn dot(color: Color) -> El {
    el().size(7.0, 7.0).radius(3.5).fill(color)
}

/// A status light for something under way: it breathes until the work is done.
pub(super) fn pulse(color: Color) -> El {
    dot(color).looping(rune::LoopSpec::opacity(1.4, 0.0, 0.3, 1.0))
}

/// Work under way, the height of the tool that started it so the row it sits in keeps still.
pub(super) fn progress(content: impl Into<String>, key: &str) -> El {
    row()
        .key(key)
        .center_y()
        .gap(7.0)
        .px(12.0)
        .h(32.0)
        .child(pulse(GOLD))
        .child(caption(content, 12.5, 500.0, MUTED))
}

/// A short status message shaped like the control it stands in for.
pub(super) fn notice(content: impl Into<String>, key: &str) -> El {
    row()
        .key(key)
        .center_y()
        .px(14.0)
        .h(40.0)
        .radius(20.0)
        .fill(wash(1.0))
        .child(caption(content, 12.5, 500.0, MUTED))
}
