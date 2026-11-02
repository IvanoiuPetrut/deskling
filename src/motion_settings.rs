use std::time::Instant;

use gpui_kit::component::button::Button;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Selectable, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::animation::{Animations, CURVE_MAX_Y, CURVE_MIN_Y, Curve, MotionKind};

/// Size of the curve editor, and the space around the curve inside it for the handles.
const PLOT_WIDTH: f32 = 280.;
const PLOT_HEIGHT: f32 = 300.;
const PLOT_INSET: f32 = 14.;
const HANDLE_SIZE: f32 = 14.;

/// Converts between curve coordinates (x 0–1, y `CURVE_MIN_Y`–`CURVE_MAX_Y`) and pixels in
/// the editor, so the whole range a handle can be dragged to is visible.
#[derive(Clone, Copy)]
struct Plot {
    bounds: Bounds<Pixels>,
}

impl Plot {
    fn inner(self) -> (f32, f32) {
        let size = self.bounds.size;
        (f32::from(size.width) - 2. * PLOT_INSET, f32::from(size.height) - 2. * PLOT_INSET)
    }

    fn to_px(self, x: f32, y: f32) -> Point<Pixels> {
        let (width, height) = self.inner();
        let up = (y - CURVE_MIN_Y) / (CURVE_MAX_Y - CURVE_MIN_Y);
        point(self.bounds.left() + px(PLOT_INSET + x * width), self.bounds.bottom() - px(PLOT_INSET + up * height))
    }

    fn to_curve(self, position: Point<Pixels>) -> (f32, f32) {
        let (width, height) = self.inner();
        let x = (f32::from(position.x - self.bounds.left()) - PLOT_INSET) / width;
        let up = (f32::from(self.bounds.bottom() - position.y) - PLOT_INSET) / height;
        (x, CURVE_MIN_Y + up * (CURVE_MAX_Y - CURVE_MIN_Y))
    }
}

/// The editor for one animation in the dashboard: on/off, duration, and its timing curve, which
/// can be picked from presets or shaped by dragging its two handles. Changes go straight into
/// the shared `Animations`, and the widgets use them from their next animation on.
pub struct MotionSettings {
    animations: Entity<Animations>,
    kind: MotionKind,
    duration: Entity<SliderState>,
    /// The handle (0 or 1) being dragged.
    dragging: Option<usize>,
    /// When "Play" was last clicked, and which way the preview runs this time.
    preview_started: Option<Instant>,
    preview_forward: bool,
    _subscriptions: Vec<Subscription>,
}

impl MotionSettings {
    pub fn new(animations: Entity<Animations>, kind: MotionKind, cx: &mut Context<Self>) -> Self {
        let current = animations.read(cx).get(kind).duration_ms;
        let duration = cx.new(|_| SliderState::new().min(100.).max(2000.).step(50.).default_value(current));
        let target = animations.clone();
        let subscriptions = vec![
            cx.subscribe(&duration, move |_, _, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event {
                    target.update(cx, |animations, cx| {
                        animations.get_mut(kind).duration_ms = value.start();
                        cx.notify();
                    });
                    cx.notify();
                }
            }),
        ];
        Self {
            animations,
            kind,
            duration,
            dragging: None,
            preview_started: None,
            preview_forward: false,
            _subscriptions: subscriptions,
        }
    }

    fn change(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut crate::animation::Motion)) {
        let kind = self.kind;
        self.animations.update(cx, |animations, cx| {
            change(animations.get_mut(kind));
            cx.notify();
        });
        cx.notify();
    }

    /// Moves a handle to `position`, rounded to hundredths so the numbers stay readable.
    fn move_handle(&mut self, handle: usize, plot: Plot, position: Point<Pixels>, cx: &mut Context<Self>) {
        let (x, y) = plot.to_curve(position);
        let round = |value: f32| (value * 100.).round() / 100.;
        self.change(cx, |motion| motion.curve.set_point(handle, round(x), round(y)));
    }

    /// A press in the editor grabs the closest handle and brings it to the pointer.
    fn start_drag(&mut self, plot: Plot, position: Point<Pixels>, cx: &mut Context<Self>) {
        let curve = self.animations.read(cx).get(self.kind).curve;
        let distance = |handle: usize| {
            let (x, y) = curve.point(handle);
            let at = plot.to_px(x, y);
            f32::from(at.x - position.x).hypot(f32::from(at.y - position.y))
        };
        let handle = if distance(0) <= distance(1) { 0 } else { 1 };
        self.dragging = Some(handle);
        self.move_handle(handle, plot, position, cx);
    }

    fn play(&mut self, cx: &mut Context<Self>) {
        self.preview_forward = !self.preview_forward;
        self.preview_started = Some(Instant::now());
        cx.notify();
    }

    /// The curve, its handles and (while the preview plays) a dot running along it. Dragging
    /// is handled here too: the window-wide mouse listeners keep a drag going outside the editor.
    fn curve_editor(&self, curve: Curve, time: Option<f32>, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let accent = cx.theme().primary;
        let faint = cx.theme().muted_foreground.opacity(0.35);
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let plot = Plot { bounds };
                let at = |(x, y): (f32, f32)| plot.to_px(x, y);
                let stroke = |width: f32, points: &[Point<Pixels>], color: Hsla, window: &mut Window| {
                    let mut line = PathBuilder::stroke(px(width));
                    line.add_polygon(points, false);
                    if let Ok(path) = line.build() {
                        window.paint_path(path, color);
                    }
                };

                // The 0–1 square, and the straight line a linear curve would follow.
                let corners = [at((0., 0.)), at((1., 0.)), at((1., 1.)), at((0., 1.)), at((0., 0.))];
                stroke(1., &corners, faint, window);
                let mut diagonal = PathBuilder::stroke(px(1.)).dash_array(&[px(4.), px(4.)]);
                diagonal.move_to(at((0., 0.)));
                diagonal.line_to(at((1., 1.)));
                if let Ok(path) = diagonal.build() {
                    window.paint_path(path, faint);
                }

                // Handle arms, then the curve over them.
                let (first, second) = (curve.point(0), curve.point(1));
                stroke(1.5, &[at((0., 0.)), at(first)], accent.opacity(0.5), window);
                stroke(1.5, &[at((1., 1.)), at(second)], accent.opacity(0.5), window);
                let mut line = PathBuilder::stroke(px(2.5));
                line.move_to(at((0., 0.)));
                line.cubic_bezier_to(at((1., 1.)), at(first), at(second));
                if let Ok(path) = line.build() {
                    window.paint_path(path, accent);
                }

                let dot = |center: Point<Pixels>, size: f32, color: Hsla, window: &mut Window| {
                    let bounds = Bounds::centered_at(center, gpui_kit::size(px(size), px(size)));
                    window.paint_quad(fill(bounds, color).corner_radii(px(size / 2.)));
                };
                for handle in [first, second] {
                    dot(at(handle), HANDLE_SIZE, white(), window);
                    dot(at(handle), HANDLE_SIZE - 4., accent, window);
                }
                if let Some(t) = time {
                    dot(at((t, curve.ease(t))), 8., accent.opacity(0.8), window);
                }

                let down = this.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && bounds.contains(&event.position)
                    {
                        down.update(cx, |this, cx| this.start_drag(plot, event.position, cx));
                    }
                });
                let moved = this.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && event.dragging() {
                        moved.update(cx, |this, cx| {
                            if let Some(handle) = this.dragging {
                                this.move_handle(handle, plot, event.position, cx);
                            }
                        });
                    }
                });
                let up = this.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        up.update(cx, |this, _| this.dragging = None);
                    }
                });
            },
        )
        .w(px(PLOT_WIDTH))
        .h(px(PLOT_HEIGHT))
    }

    /// What the animation looks like on a widget, at `progress` (0–1, eased).
    fn preview(&self, progress: f32, cx: &App) -> Div {
        let accent = cx.theme().primary;
        let sample = div()
            .w(px(160.))
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .p_3()
            .rounded_lg()
            .bg(black().opacity(0.75))
            .text_color(white());
        match self.kind {
            // A reading going from 20% to 80% (or back).
            MotionKind::Values => {
                let percent = 20. + 60. * progress;
                sample.child(div().text_xs().opacity(0.8).child("CPU")).child(format!("{percent:.0}%")).child(
                    div().w(relative(0.8)).h(px(4.)).rounded_full().bg(accent.opacity(0.25)).child(
                        div().h_full().w(relative((percent / 100.).clamp(0., 1.))).rounded_full().bg(accent),
                    ),
                )
            }
            MotionKind::FadeIn => sample
                .opacity(progress.clamp(0., 1.))
                .child(div().text_xs().opacity(0.8).child("Mon 2 Nov"))
                .child("14:05"),
        }
    }
}

impl Render for MotionSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let motion = *self.animations.read(cx).get(self.kind);
        let (switch_label, note) = match self.kind {
            MotionKind::Values => (
                "Animate number and bar changes",
                "Numbers, bars and labels glide to each new reading instead of jumping. Readings come \
                 once a second, so anything over 1000 ms is still moving when the next one arrives.",
            ),
            MotionKind::FadeIn => {
                ("Fade in when a widget is shown", "Widgets fade in when the app starts and when you turn them on.")
            }
        };

        // Time into the preview (0–1), while it plays.
        let time = self.preview_started.map(|started| {
            (started.elapsed().as_secs_f32() * 1000. / motion.duration_ms.max(1.)).min(1.)
        });
        let playing = time.is_some_and(|t| t < 1.);
        if playing {
            window.request_animation_frame();
        }
        let eased = motion.curve.ease(time.unwrap_or(1.));
        let progress = if self.preview_forward || time.is_none() { eased } else { 1. - eased };

        let id = self.kind as usize;
        let presets = Curve::PRESETS.iter().enumerate().map(|(index, (name, preset))| {
            let preset = *preset;
            Button::new(SharedString::from(format!("preset-{id}-{index}")))
                .small()
                .label(*name)
                .selected(motion.curve.preset_name() == Some(*name))
                .on_click(cx.listener(move |this, _, _, cx| this.change(cx, |motion| motion.curve = preset)))
        });
        let presets: Vec<_> = presets.collect();
        let curve = motion.curve;
        let curve_text = format!(
            "{}: cubic-bezier({:.2}, {:.2}, {:.2}, {:.2})",
            curve.preset_name().unwrap_or("Custom"),
            curve.x1,
            curve.y1,
            curve.x2,
            curve.y2
        );

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(Switch::new(("motion-enabled", id)).checked(motion.enabled).label(switch_label).on_click(cx.listener(
                |this, on: &bool, _, cx| this.change(cx, |motion| motion.enabled = *on),
            )))
            .child(div().text_sm().opacity(0.7).child(note))
            .when(motion.enabled, |page| {
                page.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(format!("Duration: {:.0} ms", motion.duration_ms))
                        .child(Slider::new(&self.duration)),
                )
                .child(div().flex().flex_wrap().gap_1().children(presets))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_6()
                        .child(
                            div()
                                .flex_none()
                                .rounded_lg()
                                .border_1()
                                .border_color(black().opacity(0.1))
                                .bg(black().opacity(0.02))
                                .child(self.curve_editor(curve, time.filter(|_| playing), cx)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_start()
                                .gap_3()
                                .child(div().text_sm().child(curve_text))
                                .child(div().text_sm().opacity(0.7).max_w(px(260.)).child(
                                    "Across is time, up is progress. Drag the two handles to shape the \
                                     curve; above the box overshoots, below it pulls back first.",
                                ))
                                .child(self.preview(progress, cx))
                                .child(Button::new(("play-preview", id)).small().label("Play preview").on_click(
                                    cx.listener(|this, _, _, cx| this.play(cx)),
                                )),
                        ),
                )
            })
    }
}
