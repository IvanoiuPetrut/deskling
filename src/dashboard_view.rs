//! The main window: the list of desktop widgets on the left, the selected one's options on the right.

use std::time::Duration;

use gpui_kit::component::{Disableable, Sizable};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::animation::MotionKind;
use crate::dashboard::{BatteryOptions, Dashboard, DateFormat, GraphStyle, WeatherOptions, WidgetConfig, WidgetKind};
use crate::stats::SystemStats;
use crate::theme::{self, Theme, ThemeLibrary};
use crate::weather::{self, Place};
use crate::motion_settings::MotionSettings;
use crate::widget;
use crate::widget_settings::WidgetSettings;
use crate::widgets::{gigabytes, utc_name, widget_type};

/// Which item of the list on the left is open on the right.
#[derive(Clone, Copy, PartialEq)]
enum Selected {
    Widget(usize),
    General,
    SharedStyle,
    Themes,
    Layout,
    Animations,
}

/// How long typing has to pause before the city search runs.
const SEARCH_DELAY: Duration = Duration::from_millis(400);

/// The editing state of one widget's options page (text box, sliders, style editor).
struct WidgetPanel {
    style: Entity<WidgetSettings>,
    label: Entity<InputState>,
    warning: Entity<SliderState>,
    critical: Entity<SliderState>,
    utc_offset: Entity<SliderState>,
    /// Batteries widget: a name box per device, by the device's Windows name. Added as devices
    /// show up (see `DashboardView::add_battery_name_boxes`).
    battery_names: Vec<(String, Entity<InputState>)>,
    /// Weather widget: the city search box, the places it found, and how the search went.
    city: Entity<InputState>,
    places: Vec<Place>,
    search_status: Option<SharedString>,
    /// Counts searches, so an answer that comes in after a newer search started is ignored.
    searches: usize,
    // Dropped together with the panel when the widget is removed.
    _subscriptions: Vec<Subscription>,
}

impl WidgetPanel {
    fn new(
        config: &Entity<WidgetConfig>,
        themes: &Entity<ThemeLibrary>,
        window: &mut Window,
        cx: &mut Context<DashboardView>,
    ) -> Self {
        let current = config.read(cx);
        let (style, label_text, thresholds, clock) =
            (current.style.clone(), current.parts.label.clone(), current.thresholds, current.clock);

        let themes = themes.clone();
        let style = cx.new(|cx| WidgetSettings::new(style, themes, window, cx));
        let label = cx.new(|cx| InputState::new(window, cx).placeholder("Automatic").default_value(label_text));
        let city = cx.new(|cx| InputState::new(window, cx).placeholder("Type a city, e.g. Oslo"));
        let slider = |min: f32, max: f32, step: f32, value: f32, cx: &mut Context<DashboardView>| {
            cx.new(|_| SliderState::new().min(min).max(max).step(step).default_value(value))
        };
        let warning = slider(0., 100., 1., thresholds.warning, cx);
        let critical = slider(0., 100., 1., thresholds.critical, cx);
        let utc_offset = slider(-12., 14., 0.5, clock.utc_offset.unwrap_or(0.), cx);

        let label_config = config.clone();
        let subscriptions = vec![
            cx.subscribe(&label, move |_, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    label_config.update(cx, |config, cx| {
                        config.parts.label = text;
                        cx.notify();
                    });
                }
            }),
            cx.subscribe(&city, {
                let config = config.clone();
                move |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        let name = input.read(cx).value().to_string();
                        this.search_places(&config, name, cx);
                    }
                }
            }),
            Self::on_slide(&warning, config, |config, value| config.thresholds.warning = value, cx),
            Self::on_slide(&critical, config, |config, value| config.thresholds.critical = value, cx),
            Self::on_slide(&utc_offset, config, |config, value| config.clock.utc_offset = Some(value), cx),
        ];

        Self {
            style,
            label,
            warning,
            critical,
            utc_offset,
            battery_names: Vec::new(),
            city,
            places: Vec::new(),
            search_status: None,
            searches: 0,
            _subscriptions: subscriptions,
        }
    }

    /// Writes a slider's value into the widget's config whenever it moves.
    fn on_slide(
        slider: &Entity<SliderState>,
        config: &Entity<WidgetConfig>,
        set: fn(&mut WidgetConfig, f32),
        cx: &mut Context<DashboardView>,
    ) -> Subscription {
        let config = config.clone();
        cx.subscribe(slider, move |_, _, event: &SliderEvent, cx| {
            if let SliderEvent::Change(value) = event {
                config.update(cx, |config, cx| {
                    set(config, value.start());
                    cx.notify();
                });
                cx.notify(); // redraw the dashboard so the slider's label shows the new value
            }
        })
    }
}

pub struct DashboardView {
    dashboard: Dashboard,
    stats: Entity<SystemStats>,
    /// The open desktop window of each widget, in the same order as `dashboard.widgets`.
    windows: Vec<Option<AnyWindowHandle>>,
    selected: Selected,
    shared_panel: Entity<WidgetSettings>,
    /// Each widget's options page state, in the same order as `dashboard.widgets`.
    widget_panels: Vec<WidgetPanel>,
    grid_size: Entity<SliderState>,
    /// Editors for the values animation and the fade-in.
    motion_panels: [Entity<MotionSettings>; 2],
    /// The built-in and the user's themes, shared with every style editor.
    themes: Entity<ThemeLibrary>,
    /// Result of the last import / export / delete on the Themes page.
    theme_status: Option<SharedString>,
    /// The theme whose "Delete" was clicked once; a second click deletes it.
    confirm_delete: Option<String>,
    /// Result of the last "Save" click.
    save_status: Option<SharedString>,
    /// Shown on the General page when "Start with Windows" couldn't be changed.
    startup_status: Option<SharedString>,
    /// With a tray icon, closing the dashboard only hides it; without one, it quits the app.
    has_tray: bool,
    starts_with_windows: bool,
    _subscriptions: Vec<Subscription>,
}

impl DashboardView {
    pub fn new(
        dashboard: Dashboard,
        stats: Entity<SystemStats>,
        has_tray: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let themes = cx.new(|_| ThemeLibrary::load());
        let shared_style = dashboard.shared_style.clone();
        let shared_panel = cx.new(|cx| WidgetSettings::new(shared_style, themes.clone(), window, cx));
        let widget_panels =
            dashboard.widgets.iter().map(|config| WidgetPanel::new(config, &themes, window, cx)).collect();
        let grid_size =
            cx.new(|_| SliderState::new().min(5.).max(100.).step(5.).default_value(dashboard.grid_size));

        let animations = dashboard.animations.clone();
        let motion_panels = [MotionKind::Values, MotionKind::FadeIn]
            .map(|kind| cx.new(|cx| MotionSettings::new(animations.clone(), kind, cx)));

        let subscriptions = vec![
            // The Disk widget's drive list and the Batteries widget's device list come from the
            // stats, so redraw when they change (only while such a widget is open on the right,
            // to avoid redrawing every second).
            cx.observe(&stats, |this: &mut Self, _, cx| {
                if matches!(this.selected_kind(cx), Some(WidgetKind::Disk | WidgetKind::Battery)) {
                    cx.notify();
                }
            }),
            cx.subscribe(&grid_size, |this: &mut Self, _, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event {
                    this.dashboard.grid_size = value.start();
                    this.apply_snap();
                    cx.notify();
                }
            }),
        ];

        // With a tray icon, closing the dashboard just hides it (bring it back from the tray).
        // Without one, closing quits; otherwise the widgets would keep the app running unseen.
        #[cfg(windows)]
        let raw_hwnd = crate::tray::raw_hwnd(window);
        window.on_window_should_close(cx, move |_, cx| {
            #[cfg(windows)]
            if has_tray && let Some(hwnd) = raw_hwnd {
                // Hidden from a task, after GPUI is done handling this close request.
                cx.spawn(async move |_| crate::tray::set_window_visible(hwnd, false)).detach();
                return false;
            }
            cx.quit();
            true
        });

        // Open the widgets only once this window is fully built: opening windows while another
        // one is still being created makes GPUI log "RefCell already borrowed" and lose events.
        cx.defer_in(window, |this, _, cx| {
            for index in 0..this.dashboard.widgets.len() {
                if this.dashboard.widgets[index].read(cx).enabled {
                    this.set_shown(index, true, cx);
                }
            }
        });

        let count = dashboard.widgets.len();
        let this = Self {
            dashboard,
            stats,
            windows: vec![None; count],
            selected: if count > 0 { Selected::Widget(0) } else { Selected::SharedStyle },
            shared_panel,
            widget_panels,
            grid_size,
            motion_panels,
            themes,
            theme_status: None,
            confirm_delete: None,
            save_status: None,
            startup_status: None,
            has_tray,
            #[cfg(windows)]
            starts_with_windows: crate::tray::starts_with_windows(),
            #[cfg(not(windows))]
            starts_with_windows: false,
            _subscriptions: subscriptions,
        };
        this.apply_snap();
        this
    }

    fn selected_kind(&self, cx: &App) -> Option<WidgetKind> {
        match self.selected {
            Selected::Widget(index) => Some(self.dashboard.widgets[index].read(cx).kind),
            Selected::General | Selected::SharedStyle | Selected::Themes | Selected::Layout | Selected::Animations => {
                None
            }
        }
    }

    fn apply_snap(&self) {
        widget::set_snap_grid(self.dashboard.snap_to_grid.then_some(self.dashboard.grid_size));
    }

    /// Changes one widget's settings; its window watches them and redraws.
    fn update_widget(&self, index: usize, cx: &mut App, change: impl FnOnce(&mut WidgetConfig)) {
        self.dashboard.widgets[index].update(cx, |config, cx| {
            change(config);
            cx.notify();
        });
    }

    /// Shows or hides one widget on the desktop.
    fn set_shown(&mut self, index: usize, shown: bool, cx: &mut App) {
        self.update_widget(index, cx, |config| config.enabled = shown);
        match (shown, self.windows[index].take()) {
            (true, None) => {
                let config = self.dashboard.widgets[index].clone();
                let shared_style = self.dashboard.shared_style.clone();
                let animations = self.dashboard.animations.clone();
                self.windows[index] = Some(widget::open(config, shared_style, self.stats.clone(), animations, cx));
            }
            (true, Some(handle)) => self.windows[index] = Some(handle),
            (false, Some(handle)) => {
                handle.update(cx, |_, window, _| window.remove_window()).ok();
            }
            (false, None) => {}
        }
    }

    /// Adds a new widget of `kind`, shows it on the desktop and opens its options.
    fn add_widget(&mut self, kind: WidgetKind, window: &mut Window, cx: &mut Context<Self>) {
        let config = self.dashboard.add(kind, cx);
        self.widget_panels.push(WidgetPanel::new(&config, &self.themes, window, cx));
        self.windows.push(None);
        let index = self.dashboard.widgets.len() - 1;
        self.set_shown(index, true, cx);
        self.selected = Selected::Widget(index);
        cx.notify();
    }

    /// Closes a widget's window and forgets it (it's gone from the file on the next Save).
    fn remove_widget(&mut self, index: usize, cx: &mut Context<Self>) {
        self.set_shown(index, false, cx);
        self.dashboard.widgets.remove(index);
        self.widget_panels.remove(index);
        self.windows.remove(index);
        self.selected = match self.dashboard.widgets.len() {
            0 => Selected::SharedStyle,
            count => Selected::Widget(index.min(count - 1)),
        };
        cx.notify();
    }

    fn set_all_locked(&self, locked: bool, cx: &mut App) {
        for index in 0..self.dashboard.widgets.len() {
            self.update_widget(index, cx, |config| config.locked = locked);
        }
    }

    /// The name shown in the list: the custom label if there is one, else the type
    /// (e.g. "CPU" or "Disk space (D:\)").
    fn widget_label(config: &WidgetConfig) -> SharedString {
        if !config.parts.label.trim().is_empty() {
            return config.parts.label.clone().into();
        }
        match config.kind {
            WidgetKind::Disk => format!("{} ({})", config.kind.name(), config.disk).into(),
            WidgetKind::Weather if let Some(place) = &config.weather.place => {
                format!("{} ({})", config.kind.name(), place.name).into()
            }
            kind => kind.name().into(),
        }
    }

    fn list_item(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        item: Selected,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .when(self.selected == item, |row| row.bg(black().opacity(0.07)))
            .hover(|row| row.bg(black().opacity(0.04)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = item;
                cx.notify();
            }))
            .child(div().overflow_hidden().text_ellipsis().child(label.into()))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let widgets = self.dashboard.widgets.iter().enumerate().map(|(index, config)| {
            let config = config.read(cx);
            let (label, enabled) = (Self::widget_label(config), config.enabled);
            self.list_item(("widget", index), label, Selected::Widget(index), cx).child(
                Switch::new(("shown", index)).checked(enabled).on_click(cx.listener(
                    move |this, shown: &bool, _, cx| {
                        this.set_shown(index, *shown, cx);
                        cx.notify();
                    },
                )),
            )
        });
        let widgets: Vec<_> = widgets.collect();
        let heading = |text: &'static str| div().px_3().pt_4().pb_1().text_sm().opacity(0.6).child(text);

        div()
            .id("sidebar")
            .w(px(220.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(black().opacity(0.1))
            .child(div().px_3().pb_1().text_sm().opacity(0.6).child("Widgets"))
            .children(widgets)
            .child(heading("Add widget"))
            .child(div().flex().flex_wrap().gap_1().px_2().children(WidgetKind::ALL.map(|kind| {
                Button::new(("add", kind as usize)).small().label(format!("+ {}", kind.name())).on_click(
                    cx.listener(move |this, _, window, cx| this.add_widget(kind, window, cx)),
                )
            })))
            .child(heading("Settings"))
            .child(self.list_item("general", "General", Selected::General, cx))
            .child(self.list_item("shared-style", "Shared style", Selected::SharedStyle, cx))
            .child(self.list_item("themes", "Themes", Selected::Themes, cx))
            .child(self.list_item("layout", "Layout", Selected::Layout, cx))
            .child(self.list_item("animations", "Animations", Selected::Animations, cx))
    }

    /// A checkbox that edits one yes/no setting of widget `index`.
    fn checkbox(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        index: usize,
        set: fn(&mut WidgetConfig, bool),
        cx: &mut Context<Self>,
    ) -> Checkbox {
        Checkbox::new(id).checked(checked).label(label).on_click(cx.listener(move |this, value: &bool, _, cx| {
            this.update_widget(index, cx, |config| set(config, *value));
            cx.notify();
        }))
    }

    fn widget_options(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let config = self.dashboard.widgets[index].read(cx);
        let kind = config.kind;
        let (enabled, use_shared_style, locked) = (config.enabled, config.use_shared_style, config.locked);
        let (show_temperature, show_graph, disk) = (config.show_temperature, config.show_graph, config.disk.clone());
        let (graph_style, graph_fade) = (config.graph_style, config.graph_fade);
        let (parts, thresholds, clock) = (config.parts.clone(), config.thresholds, config.clock);
        let (battery, media, weather) = (config.battery.clone(), config.media, config.weather.clone());
        let panel = &self.widget_panels[index];
        let has_graph = widget_type(kind).has_graph();
        // Widgets with a usage % (shown as their bar) can be colored by it. Batteries warn when low.
        let has_usage = matches!(kind, WidgetKind::Cpu | WidgetKind::Ram | WidgetKind::Disk | WidgetKind::Battery);
        let low = widget_type(kind).warns_when_low();

        // Now Playing's label is the artist and its value the title (a custom label would hide the artist).
        let player = kind == WidgetKind::NowPlaying;
        let (show_label, show_value, show_bar) = match kind {
            WidgetKind::NowPlaying => ("Show artist", "Show title", "Show progress bar"),
            WidgetKind::Weather => ("Show place and sky", "Show temperature", ""),
            _ => ("Show label", "Show value", "Show bar / graph"),
        };

        // --- Content ---
        let content = section("Content")
            .when(!player, |content| {
                content.child(div().flex().flex_col().gap_1().child("Label").child(Input::new(&panel.label)))
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .child(self.checkbox("show-label", show_label, parts.show_label, index, |c, v| c.parts.show_label = v, cx))
                    .child(self.checkbox("show-value", show_value, parts.show_value, index, |c, v| c.parts.show_value = v, cx))
                    .when(!matches!(kind, WidgetKind::Clock | WidgetKind::Weather), |row| {
                        row.child(self.checkbox(
                            "show-bar",
                            show_bar,
                            parts.show_bar,
                            index,
                            |c, v| c.parts.show_bar = v,
                            cx,
                        ))
                    }),
            )
            .when(has_graph, |content| {
                content.child(self.checkbox(
                    "show-graph",
                    "History graph instead of the bar (last minute)",
                    show_graph,
                    index,
                    |c, v| c.show_graph = v,
                    cx,
                ))
            })
            .when(has_graph && show_graph, |content| {
                content.child(
                    div().flex().flex_col().gap_1().child("Graph style").child(
                        RadioGroup::horizontal("graph-style")
                            .children(GraphStyle::ALL.map(GraphStyle::name))
                            .selected_index(GraphStyle::ALL.iter().position(|style| *style == graph_style))
                            .on_click(cx.listener(move |this, picked: &usize, _, cx| {
                                this.update_widget(index, cx, |c| c.graph_style = GraphStyle::ALL[*picked]);
                                cx.notify();
                            })),
                    ),
                )
                .when(graph_style == GraphStyle::Heat, |content| {
                    content.child(div().text_sm().opacity(0.7).child(
                        "The line goes from green to yellow, then to the \"Warning\" and \"Critical\" colors \
                         in the style as it gets higher.",
                    ))
                })
                .child(self.checkbox(
                    "graph-fade",
                    "Fade out the graph's left and right edges",
                    graph_fade,
                    index,
                    |c, v| c.graph_fade = v,
                    cx,
                ))
            })
            .when(kind == WidgetKind::Cpu, |content| {
                content.child(self.checkbox(
                    "show-temperature",
                    "Show temperature (most PCs only report it to apps run as admin)",
                    show_temperature,
                    index,
                    |c, v| c.show_temperature = v,
                    cx,
                ))
            })
            .when(kind == WidgetKind::Disk, |content| content.child(self.drive_picker(index, &disk, cx)));

        // --- Clock ---
        let clock_options = (kind == WidgetKind::Clock).then(|| {
            let now = chrono::Local::now();
            let examples: Vec<String> =
                DateFormat::ALL.iter().map(|format| now.format(format.pattern()).to_string()).collect();
            let offset = panel.utc_offset.read(cx).value().start();
            section("Clock")
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_4()
                        .child(self.checkbox("hour24", "24-hour clock", clock.hour24, index, |c, v| c.clock.hour24 = v, cx))
                        .child(self.checkbox("seconds", "Show seconds", clock.seconds, index, |c, v| c.clock.seconds = v, cx)),
                )
                .child(
                    div().flex().flex_col().gap_1().child("Date").child(
                        RadioGroup::horizontal("date-format")
                            .children(examples)
                            .selected_index(DateFormat::ALL.iter().position(|format| *format == clock.date))
                            .on_click(cx.listener(move |this, picked: &usize, _, cx| {
                                this.update_widget(index, cx, |c| c.clock.date = DateFormat::ALL[*picked]);
                                cx.notify();
                            })),
                    ),
                )
                .child(
                    Checkbox::new("other-time-zone")
                        .checked(clock.utc_offset.is_some())
                        .label("Other time zone (set a custom label like \"Tokyo\" above)")
                        .on_click(cx.listener(move |this, on: &bool, _, cx| {
                            let offset = this.widget_panels[index].utc_offset.read(cx).value().start();
                            this.update_widget(index, cx, |c| c.clock.utc_offset = on.then_some(offset));
                            cx.notify();
                        })),
                )
                .when(clock.utc_offset.is_some(), |clock| {
                    clock.child(labeled_slider(format!("Time zone: {}", utc_name(offset)), &panel.utc_offset))
                })
        });

        // --- Media ---
        let media_options = player.then(|| {
            section("Media")
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_4()
                        .child(self.checkbox("show-art", "Show cover art", media.show_art, index, |c, v| c.media.show_art = v, cx))
                        .child(self.checkbox(
                            "show-controls",
                            "Show play/pause and skip buttons",
                            media.show_controls,
                            index,
                            |c, v| c.media.show_controls = v,
                            cx,
                        )),
                )
                .child(div().text_sm().opacity(0.7).child(
                    "Shows what Windows shows in its media pop-up (next to the volume overlay): Spotify,                      browsers, Media Player and most other players. When several apps are playing, Windows                      picks one. Buttons the app doesn't allow right now (like \"previous\" on the first                      track) are faded. The widget is at least wide enough for the cover and the title.",
                ))
        });

        // --- Weather ---
        let weather_options = (kind == WidgetKind::Weather).then(|| self.weather_options(index, &weather, cx));

        // --- Batteries ---
        let battery_options = (kind == WidgetKind::Battery).then(|| self.battery_devices(index, &battery, cx));

        // --- Color by usage ---
        let usage_colors = has_usage.then(|| {
            let (title, check, limit) = match low {
                true => ("Low battery", "Change color when a battery runs low", "% or less"),
                false => ("Color by usage", "Change color when usage is high", "% and up"),
            };
            let note = match low {
                true => "Recolors the level and bar. The colors are \"Warning\" and \"Critical\" in the style.",
                false => "Recolors the value, bar and graph. The colors are \"Warning\" and \"Critical\" \
                          in the style. For disks this is the used space.",
            };
            section(title)
                .child(self.checkbox("thresholds", check, thresholds.enabled, index, |c, v| c.thresholds.enabled = v, cx))
                .when(thresholds.enabled, |colors| {
                    colors
                        .child(labeled_slider(format!("Warning at {:.0}{limit}", thresholds.warning), &panel.warning))
                        .child(labeled_slider(format!("Critical at {:.0}{limit}", thresholds.critical), &panel.critical))
                        .child(div().text_sm().opacity(0.7).child(note))
                })
        });

        // --- Style ---
        let style = section("Style")
            .child(
                Switch::new("use-shared-style")
                    .checked(use_shared_style)
                    .label("Use shared style")
                    .on_click(cx.listener(move |this, shared: &bool, _, cx| {
                        this.update_widget(index, cx, |config| config.use_shared_style = *shared);
                        cx.notify();
                    })),
            )
            .map(|style| {
                if use_shared_style {
                    style.child(
                        div()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap_2()
                            .child(div().opacity(0.7).child(
                                "This widget uses the shared style. Turn off \"Use shared style\" to give it its own look.",
                            ))
                            .child(Button::new("edit-shared").label("Edit shared style").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.selected = Selected::SharedStyle;
                                    cx.notify();
                                },
                            ))),
                    )
                } else {
                    style.child(panel.style.clone())
                }
            });

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_xl().child(kind.name()))
                    .child(Button::new("remove").small().label("Remove widget").on_click(cx.listener(
                        move |this, _, _, cx| this.remove_widget(index, cx),
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_6()
                    .child(Switch::new("shown").checked(enabled).label("Show on desktop").on_click(cx.listener(
                        move |this, shown: &bool, _, cx| {
                            this.set_shown(index, *shown, cx);
                            cx.notify();
                        },
                    )))
                    .child(self.checkbox("locked", "Lock position", locked, index, |c, v| c.locked = v, cx)),
            )
            .child(content)
            .children(clock_options)
            .children(media_options)
            .children(weather_options)
            .children(battery_options)
            .children(usage_colors)
            .child(style)
    }

    /// The Batteries widget's device list: show or hide each one, and give it your own name.
    fn battery_devices(&self, index: usize, options: &BatteryOptions, cx: &mut Context<Self>) -> Div {
        let batteries = self.stats.read(cx).batteries.clone();
        let panel = &self.widget_panels[index];
        let found = !batteries.is_empty();
        let rows = batteries.into_iter().enumerate().map(|(row, battery)| {
            let name = battery.name.clone();
            let shown = !options.hidden.contains(&name);
            let status = match battery.connected {
                true => format!("{}%", battery.level),
                false => format!("{}% · switched off", battery.level),
            };
            let name_box = panel.battery_names.iter().find(|(device, _)| *device == name).map(|(_, input)| input);
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x_3()
                .gap_y_1()
                // Fixed width, so the name boxes line up.
                .child(
                    div()
                        .w(px(300.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(Checkbox::new(("battery-shown", row)).checked(shown).label(name.clone()).on_click(
                            cx.listener(move |this, show: &bool, _, cx| {
                                let name = name.clone();
                                this.update_widget(index, cx, |config| {
                                    config.battery.hidden.retain(|hidden| *hidden != name);
                                    if !*show {
                                        config.battery.hidden.push(name);
                                    }
                                });
                                cx.notify();
                            }),
                        ))
                        .child(div().text_sm().opacity(0.6).child(status)),
                )
                .children(name_box.map(|input| div().flex_1().min_w(px(160.)).child(Input::new(input))))
        });
        let rows: Vec<_> = rows.collect();

        section("Devices")
            .child(self.checkbox(
                "show-disconnected",
                "Also show devices that are switched off (faded, with the last level Windows heard)",
                options.show_disconnected,
                index,
                |c, v| c.battery.show_disconnected = v,
                cx,
            ))
            .children(rows)
            .when(!found, |devices| {
                devices.child(div().opacity(0.7).child("No batteries found yet. The list is read every 15 seconds."))
            })
            .child(div().text_sm().opacity(0.7).child(
                "Untick a device to leave it out, and type a name to show instead of its own. These are the \
                 levels Windows shows in Settings > Bluetooth & devices; devices send them every few \
                 minutes. Devices on their own USB receiver (like Logitech Lightspeed) don't report to \
                 Windows, so they can't be listed.",
            ))
    }

    /// The Weather widget's place (picked with a city search), unit and forecast rows.
    fn weather_options(&self, index: usize, options: &WeatherOptions, cx: &mut Context<Self>) -> Div {
        let panel = &self.widget_panels[index];
        let results = panel.places.iter().enumerate().map(|(row, place)| {
            let picked = place.clone();
            Button::new(("place", row)).small().label(place.full_name()).on_click(cx.listener(move |this, _, _, cx| {
                let place = picked.clone();
                this.update_widget(index, cx, |config| config.weather.place = Some(place));
                this.widget_panels[index].places.clear();
                cx.notify();
            }))
        });
        let results: Vec<_> = results.collect();
        let current = match &options.place {
            Some(place) => format!("Showing the weather in {}.", place.full_name()),
            None => "No city picked yet: type one above and pick it from the list.".into(),
        };

        section("Weather")
            .child(div().flex().flex_col().gap_1().child("City").child(Input::new(&panel.city)))
            .when(!results.is_empty(), |weather| {
                weather.child(div().flex().flex_col().items_start().gap_1().children(results))
            })
            .children(panel.search_status.clone().map(|status| div().text_sm().opacity(0.7).child(status)))
            .child(div().child(current))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .child(self.checkbox(
                        "fahrenheit",
                        "Fahrenheit (°F)",
                        options.fahrenheit,
                        index,
                        |c, v| c.weather.fahrenheit = v,
                        cx,
                    ))
                    .child(self.checkbox(
                        "forecast",
                        "Show today and the next 2 days",
                        options.forecast,
                        index,
                        |c, v| c.weather.forecast = v,
                        cx,
                    )),
            )
            .child(div().text_sm().opacity(0.7).child(
                "Weather data by Open-Meteo.com (free, no account needed), fetched every 15 minutes. \
                 Without an internet connection the widget keeps the last weather it got and says \
                 \"offline\".",
            ))
    }

    /// Looks up places called `name` for the Weather widget whose settings are `config`, once
    /// typing pauses. The answer shows as a list to pick from.
    fn search_places(&mut self, config: &Entity<WidgetConfig>, name: String, cx: &mut Context<Self>) {
        // Found by its config: the panel's index changes when a widget above it is removed.
        fn panel_of<'a>(this: &'a mut DashboardView, config: &Entity<WidgetConfig>) -> Option<&'a mut WidgetPanel> {
            let index = this.dashboard.widgets.iter().position(|widget| widget == config)?;
            Some(&mut this.widget_panels[index])
        }
        let Some(panel) = panel_of(self, config) else { return };
        panel.searches += 1;
        let search = panel.searches;
        if name.trim().chars().count() < 2 {
            panel.places.clear();
            panel.search_status = None;
            cx.notify();
            return;
        }
        let config = config.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            // Still typing: a newer search takes over.
            let latest = this.update(cx, |this, cx| {
                let panel = panel_of(this, &config).filter(|panel| panel.searches == search)?;
                panel.search_status = Some("Searching…".into());
                cx.notify();
                Some(())
            });
            if !matches!(latest, Ok(Some(()))) {
                return;
            }
            let found = cx.background_executor().spawn(async move { weather::search(&name) }).await;
            this.update(cx, |this, cx| {
                let Some(panel) = panel_of(this, &config).filter(|panel| panel.searches == search) else { return };
                match found {
                    Ok(places) => {
                        panel.search_status = places.is_empty().then(|| "No places found by that name.".into());
                        panel.places = places;
                    }
                    Err(error) => panel.search_status = Some(format!("Could not search: {error}").into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Makes a name box for every battery the open Batteries widget doesn't have one for yet.
    /// Done while drawing, since a device can show up at any time.
    fn add_battery_name_boxes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Selected::Widget(index) = self.selected else { return };
        let config = self.dashboard.widgets[index].clone();
        if config.read(cx).kind != WidgetKind::Battery {
            return;
        }
        let devices: Vec<String> = self.stats.read(cx).batteries.iter().map(|battery| battery.name.clone()).collect();
        for device in devices {
            if self.widget_panels[index].battery_names.iter().any(|(name, _)| *name == device) {
                continue;
            }
            let current = config.read(cx).battery.names.get(&device).cloned().unwrap_or_default();
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(device.clone()).default_value(current));
            let (config, key) = (config.clone(), device.clone());
            let subscription = cx.subscribe(&input, move |_, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().trim().to_string();
                    config.update(cx, |config, cx| {
                        if text.is_empty() {
                            config.battery.names.remove(&key);
                        } else {
                            config.battery.names.insert(key.clone(), text);
                        }
                        cx.notify();
                    });
                }
            });
            let panel = &mut self.widget_panels[index];
            panel.battery_names.push((device, input));
            panel._subscriptions.push(subscription);
        }
    }

    fn drive_picker(&self, index: usize, current: &str, cx: &mut Context<Self>) -> Div {
        let disks = self.stats.read(cx).disks.clone();
        let selected = disks.iter().position(|d| d.mount == current);
        let labels: Vec<String> = disks
            .iter()
            .map(|d| {
                let name = if d.name.is_empty() { String::new() } else { format!(" ({})", d.name) };
                format!("{}{name}  {} free of {}", d.mount, gigabytes(d.free), gigabytes(d.total))
            })
            .collect();
        let mounts: Vec<String> = disks.into_iter().map(|d| d.mount).collect();
        div().flex().flex_col().gap_2().child("Drive").child(
            RadioGroup::vertical("drive").children(labels).selected_index(selected).on_click(cx.listener(
                move |this, picked: &usize, _, cx| {
                    let mount = mounts[*picked].clone();
                    this.update_widget(index, cx, |config| config.disk = mount);
                    cx.notify();
                },
            )),
        )
    }

    fn layout_options(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_xl().child("Layout"))
            .child(
                Switch::new("snap")
                    .checked(self.dashboard.snap_to_grid)
                    .label("Snap widgets to a grid while dragging")
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.dashboard.snap_to_grid = *on;
                        this.apply_snap();
                        cx.notify();
                    })),
            )
            .when(self.dashboard.snap_to_grid, |layout| {
                layout.child(labeled_slider(format!("Grid size: {:.0}px", self.dashboard.grid_size), &self.grid_size))
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Button::new("lock-all").label("Lock all widgets").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.set_all_locked(true, cx);
                            cx.notify();
                        },
                    )))
                    .child(Button::new("unlock-all").label("Unlock all").on_click(cx.listener(|this, _, _, cx| {
                        this.set_all_locked(false, cx);
                        cx.notify();
                    }))),
            )
            .child(div().text_sm().opacity(0.7).child(
                "Locked widgets can't be dragged. Each widget also has its own \"Lock position\" option.",
            ))
    }

    fn animation_options(&self) -> Div {
        let [values, fade_in] = self.motion_panels.clone();
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_xl().child("Animations"))
            .child(div().opacity(0.7).child(
                "Used by every widget. Changes apply from the next animation on; click Save to keep them.",
            ))
            .child(section("Value changes").child(values))
            .child(section("Fade in").child(fade_in))
    }

    fn set_starts_with_windows(&mut self, enabled: bool) {
        #[cfg(windows)]
        match crate::tray::set_starts_with_windows(enabled) {
            Ok(()) => {
                self.starts_with_windows = enabled;
                self.startup_status = None;
            }
            Err(error) => self.startup_status = Some(format!("Could not change Start with Windows: {error}").into()),
        }
        #[cfg(not(windows))]
        let _ = enabled;
    }

    /// Writes the dashboard to disk. "Saved" goes away after a few seconds; an error stays.
    fn save(&mut self, cx: &mut Context<Self>) {
        let status: SharedString = match self.dashboard.save(cx) {
            Ok(_) => "Saved".into(),
            Err(error) => format!("Could not save: {error}").into(),
        };
        self.save_status = Some(status.clone());
        cx.notify();
        if status == "Saved" {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                this.update(cx, |this, cx| {
                    // Unless something newer is showing by now.
                    if this.save_status.as_ref() == Some(&status) {
                        this.save_status = None;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    fn general_options(&self, cx: &mut Context<Self>) -> Div {
        let folder = crate::dashboard::settings_folder();
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_xl().child("General"))
            .when(cfg!(windows), |page| {
                page.child(
                    Switch::new("start-with-windows")
                        .checked(self.starts_with_windows)
                        .label("Start with Windows")
                        .on_click(cx.listener(|this, enabled: &bool, _, cx| {
                            this.set_starts_with_windows(*enabled);
                            cx.notify();
                        })),
                )
                .child(div().text_sm().opacity(0.7).child(
                    "Starts the app when you sign in, with only the widgets and the tray icon. It remembers \
                     where this program is, so turn it on from the release build, not from target\\debug.",
                ))
                .children(self.startup_status.clone().map(|status| div().text_sm().child(status)))
            })
            .children(folder.map(|folder| {
                section("Settings")
                    .child(div().text_sm().opacity(0.7).child(format!(
                        "Save writes the dashboard to dashboard.json, and your themes are kept in themes.json, \
                         both in {}",
                        folder.display()
                    )))
                    .child(div().flex().child(Button::new("open-folder").label("Open settings folder").on_click(
                        move |_, _, cx| {
                            // Not there before the first save.
                            if std::fs::create_dir_all(&folder).is_ok() {
                                cx.open_with_system(&folder);
                            }
                        },
                    )))
            }))
    }

    fn shared_style_options(&self) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_xl().child("Shared style"))
            .child(div().opacity(0.7).child("Used by every widget that has \"Use shared style\" on."))
            .child(self.shared_panel.clone())
    }

    /// Gives the shared style `theme`'s look, and with `everywhere` also every widget's own style.
    fn use_theme(&mut self, theme: &Theme, everywhere: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.shared_panel.update(cx, |panel, cx| panel.apply_theme(theme, window, cx));
        if everywhere {
            for panel in &self.widget_panels {
                panel.style.update(cx, |panel, cx| panel.apply_theme(theme, window, cx));
            }
        }
        self.theme_status = Some(match everywhere {
            true => format!("\"{}\" is now used by every widget.", theme.name).into(),
            false => format!("\"{}\" is now the shared style.", theme.name).into(),
        });
        cx.notify();
    }

    /// "Use everywhere" also replaces widgets' own styles, so ask first.
    fn confirm_use_everywhere(&mut self, theme: &Theme, window: &mut Window, cx: &mut Context<Self>) {
        let own_styles = self.dashboard.widgets.iter().filter(|config| !config.read(cx).use_shared_style).count();
        let detail = match own_styles {
            0 => "Every widget uses the shared style right now, so only the shared style changes.".to_string(),
            1 => "This also replaces the look of the 1 widget that has its own style.".to_string(),
            count => format!("This also replaces the look of the {count} widgets that have their own style."),
        };
        let detail = format!("{detail} Sizes, padding and font sizes stay. Nothing is kept until you click Save.");
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Use \"{}\" for every widget?", theme.name),
            Some(&detail),
            &[PromptButton::Ok("Use everywhere".into()), PromptButton::Cancel("Cancel".into())],
            cx,
        );
        let theme = theme.clone();
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                this.update_in(cx, |this, window, cx| this.use_theme(&theme, true, window, cx)).ok();
            }
        })
        .detach();
    }

    /// Asks for theme files and adds the themes in them.
    fn import_themes(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import themes".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            this.update(cx, |this, cx| {
                let mut added = 0;
                let mut problems = Vec::new();
                for path in paths {
                    match this.themes.update(cx, |themes, cx| {
                        let imported = themes.import(&path);
                        cx.notify();
                        imported
                    }) {
                        Ok(count) => added += count,
                        Err(error) => problems.push(format!(
                            "{}: {error}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        )),
                    }
                }
                let mut status = match added {
                    0 => "No new themes found.".to_string(),
                    1 => "Imported 1 theme.".to_string(),
                    count => format!("Imported {count} themes."),
                };
                if !problems.is_empty() {
                    status = format!("{status} Skipped {}", problems.join("; "));
                }
                this.theme_status = Some(status.into());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Asks where to save, then writes `themes` there as one file.
    fn export_themes(&mut self, themes: Vec<Theme>, suggested_name: String, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_new_path(&theme::dialog_folder(), Some(&suggested_name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else { return };
            let result = theme::export(themes, &path);
            this.update(cx, |this, cx| {
                this.theme_status = Some(match result {
                    Ok(()) => format!("Exported to {}", path.display()).into(),
                    Err(error) => format!("Could not export: {error}").into(),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn delete_theme(&mut self, name: &str, cx: &mut Context<Self>) {
        let removed = self.themes.update(cx, |themes, cx| {
            let removed = themes.remove(name);
            cx.notify();
            removed
        });
        self.confirm_delete = None;
        self.theme_status = Some(match removed {
            Ok(()) => format!("Deleted \"{name}\".").into(),
            Err(error) => format!("Could not delete: {error}").into(),
        });
        cx.notify();
    }

    fn theme_options(&self, cx: &mut Context<Self>) -> Div {
        let library = self.themes.read(cx);
        let themes: Vec<(Theme, bool)> = library.all().map(|(theme, built_in)| (theme.clone(), built_in)).collect();
        let custom = library.custom.clone();

        let cards = themes.into_iter().enumerate().map(|(index, (theme, built_in))| {
            let confirming = self.confirm_delete.as_deref() == Some(theme.name.as_str());
            let name = theme.name.clone();
            let (shared, everywhere, exported, deleted) = (theme.clone(), theme.clone(), theme.clone(), theme.name.clone());
            div()
                .w(px(208.))
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(black().opacity(0.1))
                .child(theme::preview(&theme, 182., 96.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .justify_between()
                        .gap_2()
                        .child(div().overflow_hidden().text_ellipsis().child(name))
                        .child(div().text_xs().opacity(0.6).child(if built_in { "Built-in" } else { "Yours" })),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .child(Button::new(("use-shared", index)).small().label("Use as shared").on_click(
                            cx.listener(move |this, _, window, cx| this.use_theme(&shared, false, window, cx)),
                        ))
                        .child(Button::new(("use-all", index)).small().label("Use everywhere").on_click(
                            cx.listener(move |this, _, window, cx| this.confirm_use_everywhere(&everywhere, window, cx)),
                        ))
                        .child(Button::new(("export", index)).small().label("Export…").on_click(cx.listener(
                            move |this, _, _, cx| {
                                let file_name = theme::file_name(&exported.name);
                                this.export_themes(vec![exported.clone()], file_name, cx);
                            },
                        )))
                        .when(!built_in, |buttons| {
                            buttons.child(
                                Button::new(("delete", index))
                                    .small()
                                    .when(confirming, |button| button.danger())
                                    .label(if confirming { "Really delete?" } else { "Delete" })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if this.confirm_delete.as_deref() == Some(deleted.as_str()) {
                                            this.delete_theme(&deleted, cx);
                                        } else {
                                            this.confirm_delete = Some(deleted.clone());
                                            cx.notify();
                                        }
                                    })),
                            )
                        }),
                )
        });

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_xl().child("Themes"))
            .child(div().opacity(0.7).child(
                "A theme is a look: colors, border, corners, glass and font. To make your own, style the \
                 shared style (or a widget) the way you like and use \"Save as theme\" at the top of its \
                 style options. Theme changes are saved right away.",
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(Button::new("import-themes").label("Import…").on_click(
                        cx.listener(|this, _, _, cx| this.import_themes(cx)),
                    ))
                    .child(
                        Button::new("export-mine")
                            .label("Export my themes…")
                            .disabled(custom.is_empty())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let file_name = format!("{} themes.json", crate::APP_NAME);
                                this.export_themes(custom.clone(), file_name, cx);
                            })),
                    )
                    .child(Button::new("new-theme").label("New theme from shared style").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.selected = Selected::SharedStyle;
                            cx.notify();
                        },
                    ))),
            )
            .children(self.theme_status.clone().map(|status| div().text_sm().opacity(0.7).child(status)))
            .child(div().flex().flex_wrap().gap_3().children(cards))
    }
}

/// Bahnschrift comes with Windows 10 and 11; elsewhere GPUI falls back to the default font.
const BRAND_FONT: &str = "Bahnschrift";

/// The app name in the header: the brand mark (a plain black circle), then "DESKLING" in bold
/// capitals, spaced out a little.
fn brand() -> Div {
    let mark = div().size(px(16.)).rounded_full().bg(black());
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_3()
        .child(mark)
        .child(spaced_letters(&crate::APP_NAME.to_uppercase(), 2.).text_xl())
}

/// "DESKTOP WIDGETS" in big, faint letters behind the header, starting just left of the name.
/// The header clips it, and it's drawn first so everything else sits on top.
fn watermark() -> Div {
    div()
        .absolute()
        .left(px(12.))
        .top_0()
        .bottom_0()
        .flex()
        .items_center()
        .text_color(black().opacity(0.05))
        .child(spaced_letters("DESKTOP WIDGETS", 4.).text_size(px(40.)))
}

/// `text` in the brand font, bold, with `gap` pixels between letters (GPUI has no letter
/// spacing, so each letter is its own element; a space becomes a fixed gap).
fn spaced_letters(text: &str, gap: f32) -> Div {
    let letters = text.chars().map(|letter| match letter {
        ' ' => div().w(px(gap * 3.)),
        letter => div().child(letter.to_string()),
    });
    div()
        .flex()
        .flex_none()
        .gap(px(gap))
        .whitespace_nowrap()
        .font_family(BRAND_FONT)
        .font_weight(FontWeight::BOLD)
        .children(letters.collect::<Vec<_>>())
}

/// A titled group of options, with a line above it.
fn section(title: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .pt_4()
        .border_t_1()
        .border_color(black().opacity(0.1))
        .child(div().text_lg().child(title))
}

fn labeled_slider(label: String, slider: &Entity<SliderState>) -> Div {
    div().flex().flex_col().gap_1().child(label).child(Slider::new(slider))
}

impl Render for DashboardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.add_battery_name_boxes(window, cx);
        let options = match self.selected {
            Selected::Widget(index) => self.widget_options(index, cx),
            Selected::General => self.general_options(cx),
            Selected::SharedStyle => self.shared_style_options(),
            Selected::Themes => self.theme_options(cx),
            Selected::Layout => self.layout_options(cx),
            Selected::Animations => self.animation_options(),
        };

        let header = div()
            .relative()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap_4()
            .px_6()
            .py_3()
            .border_b_1()
            .border_color(black().opacity(0.1))
            .child(watermark())
            .child(brand())
            // Takes the free space; a long message is cut off with "…" instead of pushing the
            // buttons past the window edge.
            .child(div().flex_1().min_w_0().flex().justify_end().children(
                self.save_status.clone().map(|status| div().truncate().text_sm().opacity(0.7).child(status)),
            ))
            .when(self.has_tray, |header| {
                header.child(Button::new("quit").label("Quit").on_click(|_, _, cx| cx.quit()))
            })
            .child(Button::new("save").primary().label("Save").on_click(cx.listener(|this, _, _, cx| this.save(cx))));

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    // `min_w_0`: a flex item won't shrink below its content's width otherwise (like
                    // CSS `min-width: auto`), so long text and sliders would run past the window edge.
                    .child(
                        div()
                            .id("options")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .p_6()
                            .child(options.w_full()),
                    ),
            )
    }
}
