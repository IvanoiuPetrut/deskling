//! What's on the desktop: the list of widgets, each one's settings, and saving it all to disk.

use std::path::PathBuf;

use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::widget::WidgetStyle;

#[derive(Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum WidgetKind {
    #[default]
    Clock,
    Cpu,
    Ram,
    Disk,
}

impl WidgetKind {
    pub const ALL: [WidgetKind; 4] = [WidgetKind::Clock, WidgetKind::Cpu, WidgetKind::Ram, WidgetKind::Disk];

    pub fn name(self) -> &'static str {
        match self {
            WidgetKind::Clock => "Clock",
            WidgetKind::Cpu => "CPU",
            WidgetKind::Ram => "RAM",
            WidgetKind::Disk => "Disk space",
        }
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
    /// The saved dashboard, or a fresh one (with only the clock shown) the first time.
    pub fn load(cx: &mut App) -> Self {
        let file = read_json::<DashboardFile>("dashboard.json").unwrap_or_else(|| DashboardFile {
            // Older versions saved a single widget style; keep it as the shared one.
            shared_style: read_json("widget-style.json").unwrap_or_default(),
            widgets: Vec::new(),
        });

        // Every kind gets exactly one entry, in a fixed order, even if the file lacks some.
        let widgets = WidgetKind::ALL
            .iter()
            .enumerate()
            .map(|(index, &kind)| {
                let saved = file.widgets.iter().find(|widget| widget.kind == kind);
                let default_place = WidgetFile {
                    kind,
                    enabled: kind == WidgetKind::Clock,
                    y: 100. + index as f32 * 130.,
                    ..Default::default()
                };
                let saved = saved.unwrap_or(&default_place);
                let style = cx.new(|_| saved.style.clone());
                cx.new(|_| WidgetConfig {
                    kind,
                    enabled: saved.enabled,
                    use_shared_style: saved.use_shared_style,
                    style,
                    position: point(px(saved.x), px(saved.y)),
                    disk: saved.disk.clone(),
                    show_temperature: saved.show_temperature,
                })
            })
            .collect();

        Self { shared_style: cx.new(|_| file.shared_style), widgets }
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
