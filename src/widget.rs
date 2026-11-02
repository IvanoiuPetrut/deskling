use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::animation::Animations;
use crate::dashboard::{Thresholds, WidgetConfig, WidgetKind};
use crate::glass::{self, ScreenRect};
use crate::media::{self, Command};
use crate::stats::{HISTORY_LEN, SystemStats};
use crate::weather;
use crate::widgets::{Content, Graph, Player, widget_type};

/// How often the desktop picture behind the glass is retaken, to catch wallpaper/icon changes.
const GLASS_REFRESH: Duration = Duration::from_secs(10);
/// Rows of a list widget (batteries): text size in parts of the font size, and line height.
const ROW_FONT: f32 = 0.6;
const ROW_LINE: f32 = 1.3;
/// Now Playing: the title's text size and the play button's icon size, in parts of the font size.
const PLAYER_TITLE: f32 = 0.7;
const PLAYER_BUTTON: f32 = 0.6;
/// Now Playing is at least this many font sizes wide, to fit the cover beside the title.
const PLAYER_WIDTH: f32 = 12.5;
/// After a media button press, look again this long after the click (a skip takes the app a
/// moment) instead of waiting for the next reading.
const MEDIA_REFRESH: Duration = Duration::from_millis(700);

// Media button icons. GPUI draws an SVG in one color (the text color), so only the shapes matter.
const PLAY_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M7 4.6v14.8a1 1 0 0 0 1.53.85l11.8-7.4a1 1 0 0 0 0-1.7L8.53 3.75A1 1 0 0 0 7 4.6z"/></svg>"#;
const PAUSE_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="5.5" y="4" width="4.5" height="16" rx="1.2"/><rect x="14" y="4" width="4.5" height="16" rx="1.2"/></svg>"#;
const NEXT_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4 5.3v13.4a1 1 0 0 0 1.55.84l9.9-6.7a1 1 0 0 0 0-1.68l-9.9-6.7A1 1 0 0 0 4 5.3z"/><rect x="17" y="4.5" width="3" height="15" rx="1"/></svg>"#;
const PREVIOUS_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M20 5.3v13.4a1 1 0 0 1-1.55.84l-9.9-6.7a1 1 0 0 1 0-1.68l9.9-6.7A1 1 0 0 1 20 5.3z"/><rect x="4" y="4.5" width="3" height="15" rx="1"/></svg>"#;
/// Shown instead of the cover while there's none.
const NOTE_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="6.5" cy="17.5" r="3.5"/><circle cx="17.5" cy="15.5" r="3.5"/><path d="M8 17.5V6.3a1 1 0 0 1 .8-1l11-2.2A1 1 0 0 1 21 4.1v11.4h-2V7.4l-9 1.8v8.3z"/></svg>"#;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BorderKind {
    None,
    Solid,
    Dashed,
}

impl BorderKind {
    pub const ALL: [BorderKind; 3] = [BorderKind::None, BorderKind::Solid, BorderKind::Dashed];
}

/// Everything about how the widget looks. The main panel edits it, the widget redraws from it.
#[derive(Clone, Serialize, Deserialize)]
// Fields missing from an older saved file get their default value instead of failing to load.
#[serde(default)]
pub struct WidgetStyle {
    pub width: f32,
    pub height: f32,
    pub padding: f32,
    pub corner_radius: f32,
    pub font_size: f32,
    pub text_color: Hsla,
    pub background: Hsla,
    /// 0.0 = fully see-through, 1.0 = solid. Applied to `background` only.
    pub opacity: f32,
    pub border: BorderKind,
    pub border_width: f32,
    pub border_color: Hsla,
    /// Blur of the desktop behind the widget, in pixels. 0 = no glass, plain see-through.
    pub blur: f32,
    /// Font name, e.g. "Segoe UI"; empty = the default font.
    pub font_family: String,
    /// 100 (thin) to 900 (black); 400 is regular, 700 bold.
    pub font_weight: f32,
    /// Size of the small label, in % of `font_size`.
    pub label_size: f32,
    /// Color of bars and graphs.
    pub accent_color: Hsla,
    /// Used instead of the accent (and for the value) above the warning / critical threshold.
    pub warning_color: Hsla,
    pub critical_color: Hsla,
}

impl Default for WidgetStyle {
    fn default() -> Self {
        Self {
            width: 200.,
            height: 100.,
            padding: 8.,
            corner_radius: 12.,
            font_size: 24.,
            text_color: white(),
            background: black(),
            opacity: 0.6,
            border: BorderKind::None,
            border_width: 1.,
            border_color: white().opacity(0.3),
            blur: 0.,
            font_family: String::new(),
            font_weight: 400.,
            label_size: 45.,
            accent_color: white(),
            warning_color: rgb(0xf5a623).into(),
            critical_color: rgb(0xff4d4f).into(),
        }
    }
}

thread_local! {
    /// One desktop picture shared by all widgets (it's ~50 MB on big screens).
    static DESKTOP: RefCell<Option<(Instant, Weak<glass::Snapshot>)>> = const { RefCell::new(None) };
}

/// The shared desktop picture. Retaken when it's getting old or no widget holds it any more.
fn desktop_snapshot() -> Option<Rc<glass::Snapshot>> {
    DESKTOP.with_borrow_mut(|cached| {
        if let Some((taken, weak)) = cached
            && taken.elapsed() < GLASS_REFRESH / 2
            && let Some(snapshot) = weak.upgrade()
        {
            return Some(snapshot);
        }
        let snapshot = Rc::new(win32::capture_desktop()?);
        *cached = Some((Instant::now(), Rc::downgrade(&snapshot)));
        Some(snapshot)
    })
}

/// One widget window on the desktop: a clock, CPU, RAM or disk, depending on its config.
struct DesktopWidget {
    config: Entity<WidgetConfig>,
    shared_style: Entity<WidgetStyle>,
    stats: Entity<SystemStats>,
    animations: Entity<Animations>,
    /// The numbers drawn when the latest reading came in, and that reading: while the values
    /// animation runs, the widget shows a blend of the two.
    from_stats: SystemStats,
    latest_stats: SystemStats,
    /// When `latest_stats` came in.
    stats_changed: Instant,
    /// Whether the latest reading changes anything shown (if not, there's nothing to animate).
    stats_moving: bool,
    /// When the window opened, for the fade-in.
    opened: Instant,
    /// Picture of the desktop, kept so moving the widget only needs a new crop + blur.
    desktop: Option<Rc<glass::Snapshot>>,
    /// The blurred background currently shown, if the glass effect is on.
    glass: Option<Arc<RenderImage>>,
    /// Now Playing: the cover art last drawn, so its GPU memory can be freed when it changes.
    art: Option<Arc<RenderImage>>,
    _subscriptions: Vec<Subscription>,
    _refresh: Task<()>,
}

impl DesktopWidget {
    fn new(
        config: Entity<WidgetConfig>,
        shared_style: Entity<WidgetStyle>,
        stats: Entity<SystemStats>,
        animations: Entity<Animations>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let own_style = config.read(cx).style.clone();
        let subscriptions = vec![
            // Any setting or either style changed: resize if needed and redraw.
            cx.observe_in(&config, window, |this, _, window, cx| this.restyle(window, cx)),
            cx.observe_in(&own_style, window, |this, _, window, cx| this.restyle(window, cx)),
            cx.observe_in(&shared_style, window, |this, _, window, cx| this.restyle(window, cx)),
            // New numbers every second (this also keeps the clock ticking). A list widget may
            // also need to grow or shrink when a device comes or goes.
            cx.observe_in(&stats, window, |this, _, window, cx| {
                this.take_new_stats(cx);
                resize_if_needed(window, this.wanted_size(cx));
                cx.notify();
            }),
            // Dragged or resized: remember where, and what's behind it changed.
            cx.observe_window_bounds(window, |this, window, cx| {
                let origin = window.bounds().origin;
                // No `cx.notify()` in here: the position needs saving, not redrawing.
                this.config.update(cx, |config, _| config.position = origin);
                this.update_glass(window, cx);
            }),
        ];

        let refresh = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(GLASS_REFRESH).await;
                let updated = this.update_in(cx, |this, window, cx| {
                    this.desktop = None;
                    this.update_glass(window, cx);
                });
                if updated.is_err() {
                    break; // the widget was closed
                }
            }
        });

        let latest_stats = stats.read(cx).clone();
        let mut this = Self {
            config,
            shared_style,
            stats,
            animations,
            from_stats: latest_stats.clone(),
            latest_stats,
            stats_changed: Instant::now(),
            stats_moving: false,
            opened: Instant::now(),
            desktop: None,
            glass: None,
            art: None,
            _subscriptions: subscriptions,
            _refresh: refresh,
        };
        this.update_glass(window, cx);
        this
    }

    /// The style in use: the dashboard's shared one, or this widget's own.
    fn style<'a>(&self, cx: &'a App) -> &'a WidgetStyle {
        let config = self.config.read(cx);
        if config.use_shared_style { self.shared_style.read(cx) } else { config.style.read(cx) }
    }

    /// A new reading came in: glide to it from whatever is on screen right now (which may be
    /// partway through the previous glide).
    fn take_new_stats(&mut self, cx: &App) {
        let shown = self.shown_stats(cx).into_owned();
        self.latest_stats = self.stats.read(cx).clone();
        self.stats_changed = Instant::now();
        self.stats_moving = !self.content(&shown, cx).looks_like(&self.content(&self.latest_stats, cx));
        self.from_stats = shown;
    }

    /// The numbers to draw now: a blend while the values animation runs, else the latest reading.
    fn shown_stats(&self, cx: &App) -> std::borrow::Cow<'_, SystemStats> {
        let progress = self.stats_moving.then(|| self.animations.read(cx).values.progress(self.stats_changed));
        match progress.flatten() {
            Some(t) => std::borrow::Cow::Owned(SystemStats::blend(&self.from_stats, &self.latest_stats, t)),
            None => std::borrow::Cow::Borrowed(&self.latest_stats),
        }
    }

    fn restyle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        watch_weather(self.config.read(cx));
        resize_if_needed(window, self.wanted_size(cx));
        self.update_glass(window, cx);
    }

    /// The style's size, but tall enough for every row of a list widget. The heights here
    /// follow the layout in `render`, with a few pixels to spare.
    fn wanted_size(&self, cx: &App) -> Size<Pixels> {
        let style = self.style(cx);
        let config = self.config.read(cx);
        let parts = &config.parts;
        let content = self.content(&self.latest_stats, cx);
        let (mut width, mut height) = (style.width, style.height);
        let border = if style.border == BorderKind::None { 0. } else { 2. * style.border_width };
        if config.kind == WidgetKind::NowPlaying {
            width = width.max(style.font_size * PLAYER_WIDTH);
            // Title and artist (4 px apart, `gap_1`) in the middle, with room above for the buttons
            // and below for the bar: as much below as above, since the titles stay centered.
            let title = if parts.show_value { style.font_size * PLAYER_TITLE * 1.618 } else { 0. };
            let artist = if parts.show_label { style.font_size * style.label_size / 100. * 1.618 } else { 0. };
            let gap = if parts.show_value && parts.show_label { 4. } else { 0. };
            let buttons = if config.media.show_controls { style.font_size * PLAYER_BUTTON * 1.4 } else { 0. };
            let bar = if parts.show_bar { 3. } else { 0. };
            let edge = buttons.max(bar);
            let edge = if edge > 0. { edge + 4. } else { 0. };
            height = height.max(2. * style.padding + border + title + gap + artist + 2. * edge + 4.);
        }
        if !content.rows.is_empty() {
            // GPUI's default line height is 1.618 x the text size.
            let label = if parts.show_label { style.font_size * style.label_size / 100. * 1.618 } else { 0. };
            let value = match parts.show_value && !content.value.is_empty() {
                true => style.font_size * 1.618 + 4.,
                false => 0.,
            };
            let rows: f32 = content
                .rows
                .iter()
                .map(|row| {
                    let bar = if parts.show_bar && row.fill.is_some() { 2. + 3. } else { 0. };
                    style.font_size * ROW_FONT * ROW_LINE + bar + 4.
                })
                .sum();
            // Label, value and rows are 4 px apart (`gap_1`).
            let needed = 2. * style.padding + border + label + value + rows + 6.;
            height = height.max(needed);
        }
        size(px(width), px(height))
    }

    /// Rebuilds the blurred background for the widget's current position, size and style.
    fn update_glass(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let style = self.style(cx);
        let (blur, border, border_width) = (style.blur, style.border, style.border_width);

        let new_glass = if blur > 0. {
            if self.desktop.is_none() {
                self.desktop = desktop_snapshot();
            }
            let scale = window.scale_factor();
            // With a solid border the glass only fills the inside, so skip the border strip.
            let inset = if border == BorderKind::Solid { (border_width * scale).round() as i32 } else { 0 };
            self.desktop.as_ref().zip(win32::window_screen_rect(window)).and_then(|(desktop, rect)| {
                let inside = ScreenRect {
                    x: rect.x + inset,
                    y: rect.y + inset,
                    width: rect.width - 2 * inset,
                    height: rect.height - 2 * inset,
                };
                glass::blurred_region(desktop, inside, (blur * scale).round() as u32)
            })
        } else {
            self.desktop = None; // glass is off: let go of the big picture
            None
        };

        // Free the old image's GPU memory.
        if let Some(old) = std::mem::replace(&mut self.glass, new_glass) {
            window.drop_image(old).ok();
        }
        cx.notify();
    }

    /// Sends a media button press, and shows the result without waiting for the next reading.
    fn send_media(&mut self, command: Command, cx: &mut Context<Self>) {
        fn refresh(stats: &mut SystemStats, cx: &mut Context<SystemStats>) {
            stats.media = media::latest();
            cx.notify();
        }
        media::send(command);
        self.stats.update(cx, refresh);
        let stats = self.stats.downgrade();
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(MEDIA_REFRESH).await;
            stats.update(cx, refresh).ok();
        })
        .detach();
    }

    /// Previous, play/pause and next for the Now Playing widget, with the play icon `size` big.
    /// Buttons the app doesn't allow right now are faded and can't be clicked.
    fn media_buttons(&self, player: &Player, size: f32, color: Hsla, cx: &Context<Self>) -> Div {
        let button = |id: &'static str, icon: &'static [u8], size: f32, enabled: bool, command: Command| {
            let button = div()
                .id(id)
                .flex_none()
                .p(px(size * 0.2))
                .rounded_full()
                .child(svg().data(icon).size(px(size)).text_color(color));
            match enabled {
                // `occlude`: the button gets the click, not the drag area under it.
                true => button
                    .occlude()
                    .cursor_pointer()
                    .hover(|button| button.bg(color.opacity(0.15)))
                    .on_click(cx.listener(move |this, _, _, cx| this.send_media(command, cx))),
                false => button.opacity(0.35),
            }
        };
        let play = if player.playing { PAUSE_ICON } else { PLAY_ICON };
        div()
            .flex()
            .items_center()
            .gap(px(size * 0.4))
            .child(button("previous", PREVIOUS_ICON, size * 0.8, player.can_previous, Command::Previous))
            .child(button("play-pause", play, size, player.can_play_pause, Command::PlayPause))
            .child(button("next", NEXT_ICON, size * 0.8, player.can_next, Command::Next))
    }

    /// What the widget shows for `stats`, from its widget type (see `src/widgets/`).
    fn content(&self, stats: &SystemStats, cx: &App) -> Content {
        let config = self.config.read(cx);
        widget_type(config.kind).content(config, stats)
    }
}

/// A Weather widget's place gets its weather fetched (from then on, every 15 minutes).
fn watch_weather(config: &WidgetConfig) {
    if config.kind == WidgetKind::Weather
        && let Some(place) = &config.weather.place
    {
        weather::watch(place);
    }
}

/// Resizes the window unless it's already about `wanted` (sizes round to whole device pixels,
/// so an exact compare could resize again on every update).
fn resize_if_needed(window: &mut Window, wanted: Size<Pixels>) {
    let current = window.viewport_size();
    let off = |a: Pixels, b: Pixels| f32::from(a - b).abs() > 1.;
    if off(current.width, wanted.width) || off(current.height, wanted.height) {
        window.resize(wanted);
    }
}

/// The threshold color for a level in %, if it's past the warning or critical one. With
/// `when_low` (batteries) "past" means at or below, otherwise at or above.
fn threshold_color(percent: f32, thresholds: Thresholds, when_low: bool, style: &WidgetStyle) -> Option<Hsla> {
    let past = |limit: f32| if when_low { percent <= limit } else { percent >= limit };
    match thresholds.enabled {
        true if past(thresholds.critical) => Some(style.critical_color),
        true if past(thresholds.warning) => Some(style.warning_color),
        _ => None,
    }
}

/// A thin rounded bar, filled to `fill` (0.0-1.0). The caller sets its width.
fn usage_bar(fill: f32, color: Hsla, height: f32) -> Div {
    div()
        .h(px(height))
        .rounded_full()
        .bg(color.opacity(0.25))
        .child(div().h_full().w(relative(fill.clamp(0., 1.))).rounded_full().bg(color))
}

/// A small area chart of `graph`, newest value at the right edge, drawn with GPUI paths.
fn history_graph(graph: Graph, color: Hsla) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let points = graph.points;
            if points.len() < 2 {
                return;
            }
            let step = bounds.size.width / (HISTORY_LEN - 1) as f32;
            let first_slot = HISTORY_LEN - points.len();
            let x = |i: usize| bounds.left() + step * (first_slot + i) as f32;
            let y = |value: f32| bounds.bottom() - bounds.size.height * (value / graph.max).clamp(0., 1.);

            // Filled area under the line, then the line on top.
            let mut area = PathBuilder::fill();
            area.move_to(point(x(0), bounds.bottom()));
            for (i, value) in points.iter().enumerate() {
                area.line_to(point(x(i), y(*value)));
            }
            area.line_to(point(x(points.len() - 1), bounds.bottom()));
            area.close();
            if let Ok(path) = area.build() {
                window.paint_path(path, color.opacity(0.25));
            }

            let mut line = PathBuilder::stroke(px(1.5));
            line.move_to(point(x(0), y(points[0])));
            for (i, value) in points.iter().enumerate().skip(1) {
                line.line_to(point(x(i), y(*value)));
            }
            if let Ok(path) = line.build() {
                window.paint_path(path, color);
            }
        },
    )
}

impl Render for DesktopWidget {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let animations = *self.animations.read(cx);
        let stats = self.shown_stats(cx);
        let gliding = matches!(stats, std::borrow::Cow::Owned(_));
        let Content { label, value, bar, graph, rows, player, icon } = self.content(&stats, cx);
        let fade = animations.fade_in.progress(self.opened);
        // Keep drawing frames until both animations are done.
        if gliding || fade.is_some() {
            window.request_animation_frame();
        }
        // New cover art: free the old one's GPU memory.
        let art = player.as_ref().and_then(|player| player.art.clone());
        let same_art = match (&art, &self.art) {
            (Some(new), Some(old)) => Arc::ptr_eq(new, old),
            (new, old) => new.is_none() && old.is_none(),
        };
        if !same_art && let Some(old) = std::mem::replace(&mut self.art, art) {
            window.drop_image(old).ok();
        }
        let config = self.config.read(cx);
        let (parts, thresholds, show_graph, locked) =
            (config.parts.clone(), config.thresholds, config.show_graph, config.locked);
        let warns_when_low = widget_type(config.kind).warns_when_low();
        let style = self.style(cx).clone();

        // A custom label from the dashboard replaces the automatic one.
        let label = if parts.label.trim().is_empty() { label } else { parts.label };

        // Usage in % (the bar's fill), for the threshold colors. Widgets without a bar have none.
        let usage = bar.map(|fill| fill * 100.);
        let level_color = usage.and_then(|usage| threshold_color(usage, thresholds, warns_when_low, &style));
        let accent = level_color.unwrap_or(style.accent_color);

        // The graph replaces the bar when it's turned on and the widget has one.
        let (bar, graph) = match graph {
            Some(graph) if show_graph => (None, Some(graph)),
            _ => (bar, None),
        };

        let text = match player {
            // Now Playing: the cover on the left; title, artist, progress bar and buttons beside it.
            Some(player) => {
                let border = if style.border == BorderKind::None { 0. } else { 2. * style.border_width };
                let cover_size = (f32::from(window.viewport_size().height) - 2. * style.padding - border).max(0.);
                // Follows the widget's own corners, the padding further in.
                let cover_radius = (style.corner_radius - style.padding).max(3.);
                let cover = div()
                    .flex_none()
                    .size(px(cover_size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(cover_radius))
                    .bg(style.text_color.opacity(0.12))
                    .map(|cover| match player.art.clone() {
                        Some(art) => {
                            cover.child(img(art).size_full().object_fit(ObjectFit::Cover).rounded(px(cover_radius)))
                        }
                        None => cover.child(
                            svg().data(NOTE_ICON).size(px(cover_size * 0.4)).text_color(style.text_color.opacity(0.5)),
                        ),
                    });
                let button_size = style.font_size * PLAYER_BUTTON;
                // Title and artist stay in the middle; the buttons sit in the top-right corner and
                // the progress bar along the bottom, both laid over the column so they don't move it.
                let titles = div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(parts.show_value, |titles| {
                        titles.child(div().truncate().text_size(px(style.font_size * PLAYER_TITLE)).child(value))
                    })
                    .when(parts.show_label, |titles| {
                        let size = style.font_size * style.label_size / 100.;
                        titles.child(div().truncate().text_size(px(size)).opacity(0.8).child(label))
                    });
                let details = div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(titles)
                    .when(player.show_controls, |details| {
                        // Pushed out by the buttons' padding, so the icons line up with the corner.
                        let buttons = self.media_buttons(&player, button_size, style.text_color, cx);
                        let inset = px(-button_size * 0.2);
                        details.child(buttons.absolute().top(inset).right(inset))
                    })
                    .when(parts.show_bar, |details| {
                        details.children(
                            bar.map(|fill| usage_bar(fill, accent, 3.).absolute().bottom_0().left_0().w_full()),
                        )
                    });
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .items_center()
                    .gap(px(style.padding.max(6.)))
                    .when(player.show_art, |row| row.child(cover))
                    .child(details)
            }
            None => div()
                .relative()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .when(parts.show_label, |text| {
                    text.child(
                        div().text_size(px(style.font_size * style.label_size / 100.)).opacity(0.8).child(label),
                    )
                })
                .when(parts.show_value && !value.is_empty(), |text| {
                    let value = div().when_some(level_color, |value, color| value.text_color(color)).child(value);
                    text.child(match icon {
                        Some(icon) => div()
                            .flex()
                            .items_center()
                            .gap(px(style.font_size * 0.3))
                            .child(svg().data(icon).size(px(style.font_size * 1.1)).text_color(style.text_color))
                            .child(value),
                        None => value,
                    })
                })
                .when(rows.is_empty() && parts.show_bar, |text| {
                    text.children(bar.map(|fill| usage_bar(fill, accent, 4.).w(relative(0.8))))
                    .children(graph.map(|graph| {
                        div().w(relative(0.85)).h(px(style.font_size * 1.1)).child(history_graph(graph, accent).size_full())
                    }))
                })
                .children(rows.into_iter().map(|row| {
                    let color = row.fill.and_then(|fill| threshold_color(fill * 100., thresholds, warns_when_low, &style));
                    let icon_size = style.font_size * ROW_FONT * 1.15;
                    let icon = row.icon.map(|icon| svg().data(icon).flex_none().size(px(icon_size)).text_color(style.text_color));
                    let name = div().flex_1().min_w_0().truncate().child(row.name);
                    let value = div().flex_none().when_some(color, |value, color| value.text_color(color)).child(row.value);
                    div()
                        .w(relative(0.9))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .text_size(px(style.font_size * ROW_FONT))
                        .line_height(relative(ROW_LINE))
                        .when(row.dimmed, |row| row.opacity(0.45))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .children(icon)
                                .child(name)
                                .when(parts.show_value, |line| line.child(value)),
                        )
                        .when(parts.show_bar, |line| {
                            line.children(
                                row.fill.map(|fill| usage_bar(fill, color.unwrap_or(style.accent_color), 3.).w_full()),
                            )
                        })
                })),
        };

        // Layers, back to front: blurred desktop, tint (background color + opacity), text.
        let content = |radius: f32| {
            div()
                .relative()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(style.padding))
                .rounded(px(radius))
                .text_color(style.text_color)
                .text_size(px(style.font_size))
                .font_weight(FontWeight(style.font_weight))
                .when(!style.font_family.is_empty(), |content| content.font_family(style.font_family.clone()))
                .children(self.glass.clone().map(|glass| {
                    img(glass)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Fill)
                        .rounded(px(radius))
                }))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(radius))
                        .bg(Hsla { a: style.opacity, ..style.background }),
                )
                .child(text)
        };

        let frame = div()
            .size_full()
            .rounded(px(style.corner_radius))
            // An overshooting curve could go past fully shown, so keep it within 0–1.
            .opacity(fade.unwrap_or(1.).clamp(0., 1.))
            // Grabbing anywhere drags the window, unless the widget is locked.
            .when(!locked, |frame| frame.window_control_area(WindowControlArea::Drag));

        match style.border {
            BorderKind::None => frame.child(content(style.corner_radius)),
            // Drawn as two filled shapes (border color behind, fill on top) instead of GPUI's
            // `.border()`: on a transparent window that leaves dark pixels on the outer edge.
            BorderKind::Solid => frame
                .bg(style.border_color)
                .p(px(style.border_width))
                .child(content((style.corner_radius - style.border_width).max(0.))),
            // Dashes need GPUI's own border drawing, so this one keeps `.border()`.
            BorderKind::Dashed => frame.child(
                content(style.corner_radius)
                    .border(px(style.border_width))
                    .border_color(style.border_color)
                    .border_dashed(),
            ),
        }
    }
}

/// Opens one widget window and returns its handle, so it can be closed later.
pub fn open(
    config: Entity<WidgetConfig>,
    shared_style: Entity<WidgetStyle>,
    stats: Entity<SystemStats>,
    animations: Entity<Animations>,
    cx: &mut App,
) -> AnyWindowHandle {
    let current = config.read(cx);
    if current.kind == WidgetKind::NowPlaying {
        media::start();
    }
    watch_weather(current);
    let style = if current.use_shared_style { shared_style.read(cx) } else { current.style.read(cx) };
    let bounds = Bounds::new(current.position, size(px(style.width), px(style.height)));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        kind: WindowKind::PopUp,
        window_background: WindowBackgroundAppearance::Transparent,
        is_resizable: false,
        focus: false,
        ..Default::default()
    };

    // GPUI's own `cx.open_window`, not gpui-kit's `open_window`: gpui-kit wraps the window
    // in its `Root`, which paints the theme background and would hide the transparency.
    let handle = cx
        .open_window(options, |window, cx| {
            cx.new(|cx| DesktopWidget::new(config, shared_style, stats, animations, window, cx))
        })
        .expect("Failed to open widget");
    // The Win32 calls make Windows send the window messages right away. If GPUI is in the middle
    // of something (creating or updating a window) it can't handle them and logs "RefCell already
    // borrowed", so they run in a task, after GPUI is done with this window.
    #[cfg(windows)]
    if let Ok(Some(hwnd)) = handle.update(cx, |_, window, _| win32::raw_hwnd(window)) {
        cx.spawn(async move |_| win32::pin_to_desktop(hwnd)).detach();
    }
    handle.into()
}

/// Makes dragged widgets snap to a grid of `size` logical pixels, or turns snapping off (`None`).
pub fn set_snap_grid(size: Option<f32>) {
    #[cfg(windows)]
    win32::set_snap_grid(size);
    #[cfg(not(windows))]
    let _ = size;
}

/// Keeps the widget on the desktop layer, like a Rainmeter skin set to "On desktop":
/// below every normal window, but always directly above the desktop itself, so it
/// also stays visible after Win+D (which raises the desktop over everything).
#[cfg(windows)]
mod win32 {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

    use gpui_kit::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};

    use crate::glass::{self, ScreenRect};
    use windows::Win32::Graphics::Dwm::{
        DWM_WINDOW_CORNER_PREFERENCE, DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR,
        DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_DONOTROUND, DwmSetWindowAttribute,
    };
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_SYSTEM_FOREGROUND, FindWindowW, GW_HWNDPREV, GWL_EXSTYLE, GetWindow, GetWindowLongW,
        GetWindowRect,
        HWND_BOTTOM, HWND_TOP, KillTimer, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SetTimer, SetWindowPos, WINDOWPOS, WINEVENT_OUTOFCONTEXT, WM_NCCALCSIZE,
        GetCursorPos, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_MOVING, WM_NCDESTROY, WM_TIMER, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING, WS_EX_TOPMOST,
    };
    use windows::core::{PCWSTR, w};

    const SUBCLASS_ID: usize = 1;
    // Not in the `windows` crate: makes PrintWindow return what's actually on screen (incl. DWM content).
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);
    // Unusual id so it can't clash with GPUI's own timers on this window.
    const RECHECK_TIMER_ID: usize = 0x5EED;
    const RECHECK_TICKS: u32 = 10;
    const RECHECK_INTERVAL_MS: u32 = 100;

    // The hooks are plain functions with no `self`, so they find the widgets through these.
    // Each entry: (widget window, how many re-checks its timer has done).
    static WIDGETS: Mutex<Vec<(isize, u32)>> = Mutex::new(Vec::new());
    static FOREGROUND_HOOK: AtomicIsize = AtomicIsize::new(0);
    static PLACING: AtomicBool = AtomicBool::new(false);
    /// Snap grid in logical pixels; 0 = off.
    static SNAP_GRID: AtomicU32 = AtomicU32::new(0);
    /// While a widget is dragged: where the cursor grabbed it, relative to its top-left corner.
    static GRAB_OFFSET: Mutex<Option<(i32, i32)>> = Mutex::new(None);

    pub fn set_snap_grid(size: Option<f32>) {
        let size = size.map_or(0, |size| size.round().max(1.) as u32);
        SNAP_GRID.store(size, Ordering::Relaxed);
    }

    fn widget_windows() -> Vec<HWND> {
        WIDGETS.lock().unwrap().iter().map(|(hwnd, _)| HWND(*hwnd as *mut _)).collect()
    }

    fn hwnd_of(window: &Window) -> Option<HWND> {
        // GPUI's Window has its own `window_handle()`, so call the raw-window-handle one by name.
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::Win32(win32) = handle.as_raw() else { return None };
        Some(HWND(win32.hwnd.get() as *mut _))
    }

    /// The window's position and size on screen, in physical pixels.
    pub fn window_screen_rect(window: &Window) -> Option<ScreenRect> {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd_of(window)?, &mut rect) }.ok()?;
        Some(ScreenRect {
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        })
    }

    /// Takes a picture of the desktop (wallpaper + icons) without any windows on it,
    /// by asking the desktop window to draw itself into an off-screen bitmap.
    pub fn capture_desktop() -> Option<glass::Snapshot> {
        unsafe {
            let desktop = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
            let mut rect = RECT::default();
            GetWindowRect(desktop, &mut rect).ok()?;
            let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);

            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // negative = rows go top to bottom
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let snapshot = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) => {
                    let previous = SelectObject(dc, bitmap.into());
                    let drawn = PrintWindow(desktop, dc, PW_RENDERFULLCONTENT).as_bool();
                    let snapshot = drawn.then(|| glass::Snapshot {
                        left: rect.left,
                        top: rect.top,
                        width,
                        height,
                        bgra: std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize)
                            .to_vec(),
                    });
                    SelectObject(dc, previous);
                    let _ = DeleteObject(bitmap.into());
                    snapshot
                }
                Err(_) => None,
            };
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            snapshot
        }
    }

    /// The window's Win32 handle as a plain number, so it can be used outside a GPUI update.
    pub fn raw_hwnd(window: &Window) -> Option<isize> {
        hwnd_of(window).map(|hwnd| hwnd.0 as isize)
    }

    pub fn pin_to_desktop(raw_hwnd: isize) {
        let hwnd = HWND(raw_hwnd as *mut _);
        WIDGETS.lock().unwrap().push((raw_hwnd, 0));

        unsafe {
            // Windows 11 draws a thin border around every window; turn it off (no-op on Windows 10).
            let no_border = DWMWA_COLOR_NONE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &no_border as *const u32 as *const _,
                size_of::<u32>() as u32,
            );

            // Windows 11 also rounds window corners on its own; turn that off so the
            // widget's own corner radius (even 0) decides the shape.
            let no_rounding = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &no_rounding as *const DWM_WINDOW_CORNER_PREFERENCE as *const _,
                size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );

            // No window frame drawing at all: this removes the drop shadow (which outlines the
            // window's square, not the rounded widget) and the accent-colored line on top.
            let no_frame = DWMNCRP_DISABLED;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_NCRENDERING_POLICY,
                &no_frame as *const DWMNCRENDERINGPOLICY as *const _,
                size_of::<DWMNCRENDERINGPOLICY>() as u32,
            );

            // Windows asks before every move/raise; this hook answers "stay above the desktop".
            // It also answers WM_NCCALCSIZE with "the whole window is content, no frame".
            let _ = SetWindowSubclass(hwnd, Some(keep_above_desktop), SUBCLASS_ID, 0);

            // Make Windows ask WM_NCCALCSIZE again now that the hook is in place.
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );

            // Win+D changes the foreground window to the desktop, so re-place the widgets then.
            // One hook for all widgets: only the first widget installs it.
            if FOREGROUND_HOOK.load(Ordering::Relaxed) != 0 {
                place(hwnd);
                return;
            }
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground_change),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            FOREGROUND_HOOK.store(hook.0 as isize, Ordering::Relaxed);
        }

        // Place it right now (this also drops PopUp's always-on-top).
        place(hwnd);
    }

    /// The `hwndInsertAfter` that puts `hwnd` in the group of widgets directly above the
    /// desktop, or `None` when it's already in that group.
    fn slot_above_desktop(hwnd: HWND) -> Option<HWND> {
        let Ok(desktop) = (unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }) else {
            return Some(HWND_BOTTOM);
        };
        // Walk up from the desktop over the other widgets, so widgets don't keep pushing
        // each other out of the spot right above the desktop.
        let widgets = widget_windows();
        let mut above = unsafe { GetWindow(desktop, GW_HWNDPREV) };
        while let Ok(window) = above {
            if window == hwnd {
                return None;
            }
            if !widgets.contains(&window) {
                break;
            }
            above = unsafe { GetWindow(window, GW_HWNDPREV) };
        }
        match above {
            Ok(window) if !is_topmost(window) => Some(window),
            // Nothing normal above the desktop (e.g. after Win+D): go to the top of the normal windows.
            _ => Some(HWND_TOP),
        }
    }

    fn is_topmost(hwnd: HWND) -> bool {
        let style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
        style & WS_EX_TOPMOST.0 != 0
    }

    fn place(hwnd: HWND) {
        let Some(after) = slot_above_desktop(hwnd) else { return };
        PLACING.store(true, Ordering::Relaxed);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        PLACING.store(false, Ordering::Relaxed);
    }

    unsafe extern "system" fn on_foreground_change(
        _hook: HWINEVENTHOOK,
        _event: u32,
        _hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        // The event fires before the desktop actually moves (Win+D raising it, or dropping it
        // back on restore), so keep re-checking for a short moment afterwards.
        WIDGETS.lock().unwrap().iter_mut().for_each(|(_, rechecks)| *rechecks = 0);
        for widget in widget_windows() {
            place(widget);
            unsafe { SetTimer(Some(widget), RECHECK_TIMER_ID, RECHECK_INTERVAL_MS, None) };
        }
    }

    unsafe extern "system" fn keep_above_desktop(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            // A drag starts: remember where on the widget the cursor grabbed it.
            WM_ENTERSIZEMOVE => {
                let mut cursor = POINT::default();
                let mut rect = RECT::default();
                if unsafe { GetCursorPos(&mut cursor).is_ok() && GetWindowRect(hwnd, &mut rect).is_ok() } {
                    *GRAB_OFFSET.lock().unwrap() = Some((cursor.x - rect.left, cursor.y - rect.top));
                }
            }
            WM_EXITSIZEMOVE => *GRAB_OFFSET.lock().unwrap() = None,
            // Being dragged: Windows lets us adjust the new rectangle before it moves there.
            WM_MOVING => {
                let grid = SNAP_GRID.load(Ordering::Relaxed);
                let grab = *GRAB_OFFSET.lock().unwrap();
                let mut cursor = POINT::default();
                if grid > 0
                    && let Some((grab_x, grab_y)) = grab
                    && unsafe { GetCursorPos(&mut cursor) }.is_ok()
                {
                    // The rectangle is in physical pixels; the grid is in logical ones.
                    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                    let grid = (grid * dpi / 96) as i32;
                    let snap = |value: i32| ((value as f32 / grid as f32).round() as i32) * grid;
                    let rect = unsafe { &mut *(lparam.0 as *mut RECT) };
                    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
                    // Work from the real cursor rather than the proposed rectangle, so snapping
                    // never builds up: the widget follows the mouse, in grid steps.
                    rect.left = snap(cursor.x - grab_x);
                    rect.top = snap(cursor.y - grab_y);
                    rect.right = rect.left + width;
                    rect.bottom = rect.top + height;
                    return LRESULT(1);
                }
            }
            // "The client area is the whole window": no frame left for Windows to draw.
            // Returned before GPUI sees it, because GPUI keeps the side and bottom frame.
            WM_NCCALCSIZE if wparam.0 != 0 => return LRESULT(0),
            WM_WINDOWPOSCHANGING => {
                let pos = unsafe { &mut *(lparam.0 as *mut WINDOWPOS) };
                match slot_above_desktop(hwnd) {
                    Some(after) => {
                        pos.hwndInsertAfter = after;
                        pos.flags &= !SWP_NOZORDER;
                    }
                    None => pos.flags |= SWP_NOZORDER,
                }
            }
            // Activation (e.g. clicking the widget) ignores the answer above, so put it back.
            WM_WINDOWPOSCHANGED => {
                let pos = unsafe { &*(lparam.0 as *const WINDOWPOS) };
                if !pos.flags.contains(SWP_NOZORDER) && !PLACING.load(Ordering::Relaxed) {
                    place(hwnd);
                }
            }
            WM_TIMER if wparam.0 == RECHECK_TIMER_ID => {
                place(hwnd);
                let done = {
                    let mut widgets = WIDGETS.lock().unwrap();
                    let entry = widgets.iter_mut().find(|(widget, _)| *widget == hwnd.0 as isize);
                    entry.map_or(RECHECK_TICKS, |(_, rechecks)| {
                        *rechecks += 1;
                        *rechecks
                    })
                };
                if done >= RECHECK_TICKS {
                    let _ = unsafe { KillTimer(Some(hwnd), RECHECK_TIMER_ID) };
                }
                return LRESULT(0);
            }
            WM_NCDESTROY => unsafe {
                let _ = KillTimer(Some(hwnd), RECHECK_TIMER_ID);
                let _ = RemoveWindowSubclass(hwnd, Some(keep_above_desktop), SUBCLASS_ID);
                let none_left = {
                    let mut widgets = WIDGETS.lock().unwrap();
                    widgets.retain(|(widget, _)| *widget != hwnd.0 as isize);
                    widgets.is_empty()
                };
                // Last widget gone: stop listening for foreground changes.
                if none_left {
                    let hook = FOREGROUND_HOOK.swap(0, Ordering::Relaxed);
                    if hook != 0 {
                        let _ = UnhookWinEvent(HWINEVENTHOOK(hook as *mut _));
                    }
                }
            },
            _ => {}
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}

/// Other platforms: no desktop pinning and no glass (there's no desktop picture to blur).
#[cfg(not(windows))]
mod win32 {
    use gpui_kit::Window;

    use crate::glass::{Snapshot, ScreenRect};

    pub fn capture_desktop() -> Option<Snapshot> {
        None
    }

    pub fn window_screen_rect(_: &Window) -> Option<ScreenRect> {
        None
    }
}
