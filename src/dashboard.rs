//! What's on the desktop: the list of widgets, each one's settings, and saving it all to disk.

use std::path::PathBuf;

use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::widget::WidgetStyle;
use crate::widgets::widget_type;

#[derive(Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum WidgetKind {
    #[default]
    Clock,
    Cpu,
    Ram,
    Disk,
    Network,
}

impl WidgetKind {
    pub const ALL: [WidgetKind; 5] =
        [WidgetKind::Clock, WidgetKind::Cpu, WidgetKind::Ram, WidgetKind::Disk, WidgetKind::Network];

    pub fn name(self) -> &'static str {
        widget_type(self).name()
    }
}

/// One desktop widget's settings. Lives in an `Entity` so its window can watch it.
pub struct WidgetConfig {
    pub kind: WidgetKind,
    /// Shown on the desktop.
    pub enabled: bool,
    /// Use the dashboard's shared style instead of `style`.
    pub use_shared_style: bool,
    pub style: Entity<WidgetStyle>,
    /// Top-left corner on screen, in logical pixels. Updated when the widget is dragged.
    pub position: Point<Pixels>,
    /// Disk widget: which drive, e.g. `C:\`.
    pub disk: String,
    /// CPU widget: also show the temperature.
    pub show_temperature: bool,
    /// Widgets that have one (CPU, RAM, network): draw the last minute as a graph instead of a bar.
    pub show_graph: bool,
}

pub struct Dashboard {
    pub shared_style: Entity<WidgetStyle>,
    pub widgets: Vec<Entity<WidgetConfig>>,
}

// What actually goes into the file: plain values, no entities.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct DashboardFile {
    shared_style: WidgetStyle,
    widgets: Vec<WidgetFile>,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct WidgetFile {
    kind: WidgetKind,
    enabled: bool,
    use_shared_style: bool,
    style: WidgetStyle,
    x: f32,
    y: f32,
    disk: String,
    show_temperature: bool,
    show_graph: bool,
}

impl Default for WidgetFile {
    fn default() -> Self {
        Self {
            kind: WidgetKind::Clock,
            enabled: false,
            use_shared_style: true,
            style: WidgetStyle::default(),
            x: 100.,
            y: 100.,
            disk: "C:\\".into(),
            show_temperature: true,
            show_graph: true,
        }
    }
}

impl WidgetFile {
    fn into_config(self, cx: &mut App) -> WidgetConfig {
        WidgetConfig {
            kind: self.kind,
            enabled: self.enabled,
            use_shared_style: self.use_shared_style,
            style: cx.new(|_| self.style),
            position: point(px(self.x), px(self.y)),
            disk: self.disk,
            show_temperature: self.show_temperature,
            show_graph: self.show_graph,
        }
    }
}

/// `%APPDATA%\gpui-hello\<name>` on Windows (`~/.config/gpui-hello/<name>` elsewhere).
fn config_file(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("gpui-hello").join(name))
}

fn read_json<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    let path = config_file(name)?;
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text)
        .inspect_err(|error| log::warn!("Ignoring {}: {error}", path.display()))
        .ok()
}

impl Dashboard {
    /// The saved dashboard, or a fresh one (with just a clock) the first time.
    pub fn load(cx: &mut App) -> Self {
        let file = read_json::<DashboardFile>("dashboard.json").unwrap_or_else(|| DashboardFile {
            // Older versions saved a single widget style; keep it as the shared one.
            shared_style: read_json("widget-style.json").unwrap_or_default(),
            widgets: vec![WidgetFile { enabled: true, ..Default::default() }],
        });
        let widgets = file.widgets.into_iter().map(|widget| cx.new(|cx| widget.into_config(cx))).collect();
        Self { shared_style: cx.new(|_| file.shared_style), widgets }
    }

    /// Adds a new widget of `kind`, shown, placed below the others so they don't overlap.
    pub fn add(&mut self, kind: WidgetKind, cx: &mut App) -> Entity<WidgetConfig> {
        let count = self.widgets.len() as f32;
        let widget = WidgetFile {
            kind,
            enabled: true,
            x: 100. + (count / 6.).floor() * 260.,
            y: 100. + (count % 6.) * 130.,
            ..Default::default()
        };
        let config = cx.new(|cx| widget.into_config(cx));
        self.widgets.push(config.clone());
        config
    }

    /// Writes everything to `dashboard.json` and returns where it went.
    pub fn save(&self, cx: &App) -> std::io::Result<PathBuf> {
        let file = DashboardFile {
            shared_style: self.shared_style.read(cx).clone(),
            widgets: self
                .widgets
                .iter()
                .map(|widget| {
                    let widget = widget.read(cx);
                    WidgetFile {
                        kind: widget.kind,
                        enabled: widget.enabled,
                        use_shared_style: widget.use_shared_style,
                        style: widget.style.read(cx).clone(),
                        x: widget.position.x.into(),
                        y: widget.position.y.into(),
                        disk: widget.disk.clone(),
                        show_temperature: widget.show_temperature,
                        show_graph: widget.show_graph,
                    }
                })
                .collect(),
        };

        let path = config_file("dashboard.json").ok_or(std::io::ErrorKind::NotFound)?;
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(&file)?)?;
        Ok(path)
    }
}
