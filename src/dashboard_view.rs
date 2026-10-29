//! The main window: the list of desktop widgets on the left, the selected one's options on the right.

use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::dashboard::{Dashboard, DateFormat, WidgetConfig, WidgetKind};
use crate::stats::SystemStats;
use crate::widget;
use crate::widget_settings::WidgetSettings;
use crate::widgets::{gigabytes, utc_name, widget_type};

/// Which item of the list on the left is open on the right.
#[derive(Clone, Copy, PartialEq)]
enum Selected {
    Widget(usize),
    SharedStyle,
    Layout,
}

/// The editing state of one widget's options page (text box, sliders, style editor).
struct WidgetPanel {
    style: Entity<WidgetSettings>,
    label: Entity<InputState>,
    warning: Entity<SliderState>,
    critical: Entity<SliderState>,
    utc_offset: Entity<SliderState>,
    // Dropped together with the panel when the widget is removed.
    _subscriptions: Vec<Subscription>,
}

impl WidgetPanel {
    fn new(config: &Entity<WidgetConfig>, window: &mut Window, cx: &mut Context<DashboardView>) -> Self {
        let current = config.read(cx);
        let (style, label_text, thresholds, clock) =
            (current.style.clone(), current.parts.label.clone(), current.thresholds, current.clock);

        let style = cx.new(|cx| WidgetSettings::new(style, window, cx));
        let label = cx.new(|cx| InputState::new(window, cx).placeholder("Automatic").default_value(label_text));
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
            Self::on_slide(&warning, config, |config, value| config.thresholds.warning = value, cx),
            Self::on_slide(&critical, config, |config, value| config.thresholds.critical = value, cx),
            Self::on_slide(&utc_offset, config, |config, value| config.clock.utc_offset = Some(value), cx),
        ];

        Self { style, label, warning, critical, utc_offset, _subscriptions: subscriptions }
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
    /// Result of the last "Save" (or "Start with Windows") click.
    save_status: Option<SharedString>,
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
        let shared_style = dashboard.shared_style.clone();
        let shared_panel = cx.new(|cx| WidgetSettings::new(shared_style, window, cx));
        let widget_panels = dashboard.widgets.iter().map(|config| WidgetPanel::new(config, window, cx)).collect();
        let grid_size =
            cx.new(|_| SliderState::new().min(5.).max(100.).step(5.).default_value(dashboard.grid_size));

        let subscriptions = vec![
            // The Disk widget's drive list comes from the stats, so redraw when they change
            // (only while that widget is open on the right, to avoid redrawing every second).
            cx.observe(&stats, |this: &mut Self, _, cx| {
                if this.selected_kind(cx) == Some(WidgetKind::Disk) {
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
            save_status: None,
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
            Selected::SharedStyle | Selected::Layout => None,
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
                self.windows[index] = Some(widget::open(config, shared_style, self.stats.clone(), cx));
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
        self.widget_panels.push(WidgetPanel::new(&config, window, cx));
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
            .child(self.list_item("shared-style", "Shared style", Selected::SharedStyle, cx))
            .child(self.list_item("layout", "Layout", Selected::Layout, cx))
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
        let (parts, thresholds, clock) = (config.parts.clone(), config.thresholds, config.clock);
        let panel = &self.widget_panels[index];
        let has_graph = widget_type(kind).has_graph();
        // Widgets with a usage % (shown as their bar) can be colored by it.
        let has_usage = matches!(kind, WidgetKind::Cpu | WidgetKind::Ram | WidgetKind::Disk);

        // --- Content ---
        let content = section("Content")
            .child(div().flex().flex_col().gap_1().child("Label").child(Input::new(&panel.label)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .child(self.checkbox("show-label", "Show label", parts.show_label, index, |c, v| c.parts.show_label = v, cx))
                    .child(self.checkbox("show-value", "Show value", parts.show_value, index, |c, v| c.parts.show_value = v, cx))
                    .when(kind != WidgetKind::Clock, |row| {
                        row.child(self.checkbox(
                            "show-bar",
                            "Show bar / graph",
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

        // --- Color by usage ---
        let usage_colors = has_usage.then(|| {
            section("Color by usage")
                .child(self.checkbox(
                    "thresholds",
                    "Change color when usage is high",
                    thresholds.enabled,
                    index,
                    |c, v| c.thresholds.enabled = v,
                    cx,
                ))
                .when(thresholds.enabled, |colors| {
                    colors
                        .child(labeled_slider(format!("Warning from {:.0}%", thresholds.warning), &panel.warning))
                        .child(labeled_slider(format!("Critical from {:.0}%", thresholds.critical), &panel.critical))
                        .child(div().text_sm().opacity(0.7).child(
                            "Recolors the value, bar and graph. The colors are \"Warning\" and \"Critical\" \
                             in the style. For disks this is the used space.",
                        ))
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
            .children(usage_colors)
            .child(style)
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

    fn set_starts_with_windows(&mut self, enabled: bool) {
        #[cfg(windows)]
        match crate::tray::set_starts_with_windows(enabled) {
            Ok(()) => self.starts_with_windows = enabled,
            Err(error) => self.save_status = Some(format!("Could not change Start with Windows: {error}").into()),
        }
        #[cfg(not(windows))]
        let _ = enabled;
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = match self.selected {
            Selected::Widget(index) => self.widget_options(index, cx),
            Selected::SharedStyle => self.shared_style_options(),
            Selected::Layout => self.layout_options(cx),
        };

        let header = div()
            .flex()
            .items_center()
            .gap_4()
            .px_6()
            .py_3()
            .border_b_1()
            .border_color(black().opacity(0.1))
            .child(div().text_xl().child("Desktop widgets"))
            .child(div().flex_1())
            .children(self.save_status.clone().map(|status| div().text_xs().opacity(0.7).child(status)))
            .when(cfg!(windows), |header| {
                header.child(
                    Switch::new("start-with-windows")
                        .checked(self.starts_with_windows)
                        .label("Start with Windows")
                        .on_click(cx.listener(|this, enabled: &bool, _, cx| {
                            this.set_starts_with_windows(*enabled);
                            cx.notify();
                        })),
                )
            })
            .when(self.has_tray, |header| {
                header.child(Button::new("quit").label("Quit").on_click(|_, _, cx| cx.quit()))
            })
            .child(Button::new("save").primary().label("Save").on_click(cx.listener(|this, _, _, cx| {
                this.save_status = Some(match this.dashboard.save(cx) {
                    Ok(path) => format!("Saved to {}", path.display()).into(),
                    Err(error) => format!("Could not save: {error}").into(),
                });
                cx.notify();
            })));

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
