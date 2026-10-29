//! The main window: the list of desktop widgets on the left, the selected one's options on the right.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::Sizable;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::dashboard::{Dashboard, WidgetConfig, WidgetKind};
use crate::stats::SystemStats;
use crate::widget;
use crate::widget_settings::WidgetSettings;
use crate::widgets::{gigabytes, widget_type};

/// Which item of the list on the left is open on the right.
#[derive(Clone, Copy, PartialEq)]
enum Selected {
    SharedStyle,
    Widget(usize),
}

pub struct DashboardView {
    dashboard: Dashboard,
    stats: Entity<SystemStats>,
    /// The open desktop window of each widget, in the same order as `dashboard.widgets`.
    windows: Vec<Option<AnyWindowHandle>>,
    selected: Selected,
    shared_panel: Entity<WidgetSettings>,
    /// Each widget's own style editor, in the same order as `dashboard.widgets`.
    widget_panels: Vec<Entity<WidgetSettings>>,
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
        let widget_panels = dashboard
            .widgets
            .iter()
            .map(|config| {
                let style = config.read(cx).style.clone();
                cx.new(|cx| WidgetSettings::new(style, window, cx))
            })
            .collect();

        // The Disk widget's drive list comes from the stats, so redraw when they change
        // (only while that widget is open on the right, to avoid redrawing every second).
        let subscriptions = vec![cx.observe(&stats, |this: &mut Self, _, cx| {
            if this.selected_kind(cx) == Some(WidgetKind::Disk) {
                cx.notify();
            }
        })];

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
        Self {
            dashboard,
            stats,
            windows: vec![None; count],
            selected: if count > 0 { Selected::Widget(0) } else { Selected::SharedStyle },
            shared_panel,
            widget_panels,
            save_status: None,
            has_tray,
            #[cfg(windows)]
            starts_with_windows: crate::tray::starts_with_windows(),
            #[cfg(not(windows))]
            starts_with_windows: false,
            _subscriptions: subscriptions,
        }
    }

    fn selected_kind(&self, cx: &App) -> Option<WidgetKind> {
        match self.selected {
            Selected::Widget(index) => Some(self.dashboard.widgets[index].read(cx).kind),
            Selected::SharedStyle => None,
        }
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
        let style = config.read(cx).style.clone();
        self.widget_panels.push(cx.new(|cx| WidgetSettings::new(style, window, cx)));
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

    /// The name shown in the list, e.g. "CPU" or "Disk space (D:\)".
    fn widget_label(config: &WidgetConfig) -> SharedString {
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
            .child(label.into())
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

        div()
            .w(px(220.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .border_r_1()
            .border_color(black().opacity(0.1))
            .child(div().px_3().pb_1().text_sm().opacity(0.6).child("Widgets"))
            .children(widgets)
            .child(div().px_3().pt_4().pb_1().text_sm().opacity(0.6).child("Add widget"))
            .child(div().flex().flex_wrap().gap_1().px_2().children(WidgetKind::ALL.map(|kind| {
                Button::new(("add", kind as usize)).small().label(format!("+ {}", kind.name())).on_click(
                    cx.listener(move |this, _, window, cx| this.add_widget(kind, window, cx)),
                )
            })))
            .child(div().px_3().pt_4().pb_1().text_sm().opacity(0.6).child("Style"))
            .child(self.list_item("shared-style", "Shared style", Selected::SharedStyle, cx))
    }

    fn widget_options(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let config = self.dashboard.widgets[index].read(cx);
        let (kind, enabled, use_shared_style) = (config.kind, config.enabled, config.use_shared_style);
        let (show_temperature, show_graph, disk) = (config.show_temperature, config.show_graph, config.disk.clone());
        let has_graph = widget_type(kind).has_graph();

        let kind_options = match kind {
            WidgetKind::Cpu => Some(
                div().child(
                    Checkbox::new("show-temperature")
                        .checked(show_temperature)
                        .label("Show temperature (most PCs only report it to apps run as admin)")
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            this.update_widget(index, cx, |config| config.show_temperature = *checked);
                        })),
                ),
            ),
            WidgetKind::Disk => {
                let disks = self.stats.read(cx).disks.clone();
                let selected = disks.iter().position(|d| d.mount == disk);
                let labels: Vec<String> = disks
                    .iter()
                    .map(|d| {
                        let name = if d.name.is_empty() { String::new() } else { format!(" ({})", d.name) };
                        format!("{}{name}  {} free of {}", d.mount, gigabytes(d.free), gigabytes(d.total))
                    })
                    .collect();
                let mounts: Vec<String> = disks.into_iter().map(|d| d.mount).collect();
                Some(
                    div().flex().flex_col().gap_2().child("Drive").child(
                        RadioGroup::vertical("drive")
                            .children(labels)
                            .selected_index(selected)
                            .on_click(cx.listener(move |this, picked: &usize, _, cx| {
                                let mount = mounts[*picked].clone();
                                this.update_widget(index, cx, |config| config.disk = mount);
                            })),
                    ),
                )
            }
            WidgetKind::Clock | WidgetKind::Ram | WidgetKind::Network => None,
        };
        let graph_option = has_graph.then(|| {
            Checkbox::new("show-graph").checked(show_graph).label("Show history graph (last minute)").on_click(
                cx.listener(move |this, checked: &bool, _, cx| {
                    this.update_widget(index, cx, |config| config.show_graph = *checked);
                }),
            )
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
                Switch::new("shown").checked(enabled).label("Show on desktop").on_click(cx.listener(
                    move |this, shown: &bool, _, cx| {
                        this.set_shown(index, *shown, cx);
                        cx.notify();
                    },
                )),
            )
            .children(kind_options)
            .children(graph_option)
            .child(div().h(px(1.)).bg(black().opacity(0.1)))
            .child(
                Switch::new("use-shared-style")
                    .checked(use_shared_style)
                    .label("Use shared style")
                    .on_click(cx.listener(move |this, shared: &bool, _, cx| {
                        this.update_widget(index, cx, |config| config.use_shared_style = *shared);
                        cx.notify();
                    })),
            )
            .map(|options| {
                if use_shared_style {
                    options.child(
                        div()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap_2()
                            .child(
                                div().opacity(0.7).child(
                                    "This widget uses the shared style. Turn off \"Use shared style\" \
                                     to give it its own look.",
                                ),
                            )
                            .child(Button::new("edit-shared").label("Edit shared style").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.selected = Selected::SharedStyle;
                                    cx.notify();
                                }),
                            )),
                    )
                } else {
                    options.child(self.widget_panels[index].clone())
                }
            })
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

impl Render for DashboardView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = match self.selected {
            Selected::Widget(index) => self.widget_options(index, cx),
            Selected::SharedStyle => self.shared_style_options(),
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
                    .child(div().id("options").flex_1().overflow_y_scroll().p_6().child(options)),
            )
    }
}
