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

/// Which parts of a widget are drawn, and an optional custom label.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Parts {
    /// Replaces the automatic label (e.g. "CPU · 52 °C") when not empty.
    pub label: String,
    pub show_label: bool,
    pub show_value: bool,
    /// The usage bar, or the history graph when that's on.
    pub show_bar: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Self { label: String::new(), show_label: true, show_value: true, show_bar: true }
    }
}

/// Recolors the value, bar and graph when usage (in %) gets high.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct Thresholds {
    pub enabled: bool,
    pub warning: f32,
    pub critical: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self { enabled: false, warning: 70., critical: 90. }
    }
}

#[derive(Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum DateFormat {
    /// Thu 29 Oct
    #[default]
    Short,
    /// 29.10.2026
    Numeric,
    /// 2026-10-29
    Iso,
    /// Thursday, 29 October
    Long,
}

impl DateFormat {
    pub const ALL: [DateFormat; 4] = [DateFormat::Short, DateFormat::Numeric, DateFormat::Iso, DateFormat::Long];

    /// The chrono format string, see https://docs.rs/chrono/latest/chrono/format/strftime/
    pub fn pattern(self) -> &'static str {
        match self {
            DateFormat::Short => "%a %d %b",
            DateFormat::Numeric => "%d.%m.%Y",
            DateFormat::Iso => "%Y-%m-%d",
            DateFormat::Long => "%A, %d %B",
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct ClockOptions {
    /// 14:05 instead of 2:05 PM.
    pub hour24: bool,
    pub seconds: bool,
    pub date: DateFormat,
    /// Hours from UTC for another time zone (e.g. 9 for Tokyo); `None` = this PC's time zone.
    pub utc_offset: Option<f32>,
}

impl Default for ClockOptions {
    fn default() -> Self {
        Self { hour24: true, seconds: false, date: DateFormat::Short, utc_offset: None }
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
    pub parts: Parts,
    pub thresholds: Thresholds,
    pub clock: ClockOptions,
    /// Can't be dragged.
    pub locked: bool,
}

pub struct Dashboard {
    pub shared_style: Entity<WidgetStyle>,
    pub widgets: Vec<Entity<WidgetConfig>>,
    /// Widgets snap to a grid of `grid_size` (logical pixels) while dragged.
    pub snap_to_grid: bool,
    pub grid_size: f32,
}

// What actually goes into the file: plain values, no entities.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct DashboardFile {
    shared_style: WidgetStyle,
    widgets: Vec<WidgetFile>,
    snap_to_grid: bool,
    grid_size: f32,
}

impl Default for DashboardFile {
    fn default() -> Self {
        Self { shared_style: WidgetStyle::default(), widgets: Vec::new(), snap_to_grid: false, grid_size: 20. }
    }
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
    parts: Parts,
    thresholds: Thresholds,
    clock: ClockOptions,
    locked: bool,
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
            parts: Parts::default(),
            thresholds: Thresholds::default(),
            clock: ClockOptions::default(),
            locked: false,
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
            parts: self.parts,
            thresholds: self.thresholds,
            clock: self.clock,
            locked: self.locked,
        }
    }
}

/// The settings folder from before the app was renamed to Deskling; still read if nothing's
/// been saved under the new name yet, so existing setups carry over.
const OLD_FOLDER: &str = "gpui-hello";

/// `%APPDATA%` on Windows (`~/.config` elsewhere).
fn config_base() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

/// `%APPDATA%\Deskling\<name>` on Windows (`~/.config/Deskling/<name>` elsewhere).
fn config_file(name: &str) -> Option<PathBuf> {
    Some(config_base()?.join(crate::APP_NAME).join(name))
}

pub fn read_json<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    let new_path = config_file(name)?;
    let path = if new_path.exists() { new_path } else { config_base()?.join(OLD_FOLDER).join(name) };
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text)
        .inspect_err(|error| log::warn!("Ignoring {}: {error}", path.display()))
        .ok()
}

/// Writes `value` to the settings folder as `name` and returns where it went.
pub fn write_json<T: Serialize>(name: &str, value: &T) -> std::io::Result<PathBuf> {
    let path = config_file(name).ok_or(std::io::ErrorKind::NotFound)?;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(value)?)?;
    Ok(path)
}

impl Dashboard {
    /// The saved dashboard, or a fresh one (with just a clock) the first time.
    pub fn load(cx: &mut App) -> Self {
        let file = read_json::<DashboardFile>("dashboard.json").unwrap_or_else(|| DashboardFile {
            // Older versions saved a single widget style; keep it as the shared one.
            shared_style: read_json("widget-style.json").unwrap_or_default(),
            widgets: vec![WidgetFile { enabled: true, ..Default::default() }],
            ..Default::default()
        });
        let widgets = file.widgets.into_iter().map(|widget| cx.new(|cx| widget.into_config(cx))).collect();
        Self {
            shared_style: cx.new(|_| file.shared_style),
            widgets,
            snap_to_grid: file.snap_to_grid,
            grid_size: file.grid_size,
        }
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
                        parts: widget.parts.clone(),
                        thresholds: widget.thresholds,
                        clock: widget.clock,
                        locked: widget.locked,
                    }
                })
                .collect(),
            snap_to_grid: self.snap_to_grid,
            grid_size: self.grid_size,
        };
        write_json("dashboard.json", &file)
    }
}
