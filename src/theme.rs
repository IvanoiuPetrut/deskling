//! Themes: named looks (colors, border, corners, glass, font) that can be applied to any widget
//! style. A few come with the app; the user's own are kept in `themes.json` and can be shared
//! as files (export / import).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::dashboard::{read_json, write_json};
use crate::widget::{BorderKind, WidgetStyle};

/// The look part of a `WidgetStyle`. Size, padding and font size stay as they are when a theme
/// is applied, so a theme can't break a layout.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    pub text_color: Hsla,
    pub background: Hsla,
    pub opacity: f32,
    pub border: BorderKind,
    pub border_width: f32,
    pub border_color: Hsla,
    pub corner_radius: f32,
    pub blur: f32,
    pub font_family: String,
    pub font_weight: f32,
    pub accent_color: Hsla,
    pub warning_color: Hsla,
    pub critical_color: Hsla,
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_style("", &WidgetStyle::default())
    }
}

impl Theme {
    /// The look of `style`, under `name`.
    pub fn from_style(name: &str, style: &WidgetStyle) -> Self {
        Self {
            name: name.into(),
            text_color: style.text_color,
            background: style.background,
            opacity: style.opacity,
            border: style.border,
            border_width: style.border_width,
            border_color: style.border_color,
            corner_radius: style.corner_radius,
            blur: style.blur,
            font_family: style.font_family.clone(),
            font_weight: style.font_weight,
            accent_color: style.accent_color,
            warning_color: style.warning_color,
            critical_color: style.critical_color,
        }
    }

    /// Gives `style` this theme's look.
    pub fn apply(&self, style: &mut WidgetStyle) {
        style.text_color = self.text_color;
        style.background = self.background;
        style.opacity = self.opacity;
        style.border = self.border;
        style.border_width = self.border_width;
        style.border_color = self.border_color;
        style.corner_radius = self.corner_radius;
        style.blur = self.blur;
        style.font_family = self.font_family.clone();
        style.font_weight = self.font_weight;
        style.accent_color = self.accent_color;
        style.warning_color = self.warning_color;
        style.critical_color = self.critical_color;
    }

    /// Whether `style` currently looks like this theme.
    pub fn matches(&self, style: &WidgetStyle) -> bool {
        Theme::from_style(&self.name, style).same_look(self)
    }

    /// Same look, ignoring the name. Compared as saved: colors are stored as `#rrggbbaa`, so a
    /// theme that went through a file differs a tiny bit from the original in memory.
    fn same_look(&self, other: &Theme) -> bool {
        let saved = |theme: &Theme| serde_json::to_value(Theme { name: String::new(), ..theme.clone() }).ok();
        saved(self) == saved(other)
    }
}

fn hex(color: u32) -> Hsla {
    rgb(color).into()
}

/// The themes that come with the app. They can be applied and exported, not changed or deleted.
pub fn built_in() -> &'static [Theme] {
    static THEMES: LazyLock<Vec<Theme>> = LazyLock::new(|| {
        let base = Theme::default();
        vec![
            Theme { name: "Classic".into(), ..base.clone() },
            Theme {
                name: "Frosted glass".into(),
                background: white(),
                opacity: 0.12,
                border: BorderKind::Solid,
                border_color: white().opacity(0.35),
                corner_radius: 16.,
                blur: 20.,
                ..base.clone()
            },
            Theme {
                name: "Light".into(),
                text_color: hex(0x1f2328),
                background: hex(0xf6f8fa),
                opacity: 0.9,
                border: BorderKind::Solid,
                border_color: hex(0xd0d7de),
                accent_color: hex(0x0969da),
                warning_color: hex(0xbf8700),
                critical_color: hex(0xcf222e),
                ..base.clone()
            },
            Theme {
                name: "Midnight".into(),
                text_color: hex(0xe6edf3),
                background: hex(0x0d1b2a),
                opacity: 0.85,
                accent_color: hex(0x4cc9f0),
                ..base.clone()
            },
            Theme {
                name: "Nord".into(),
                text_color: hex(0xeceff4),
                background: hex(0x2e3440),
                opacity: 0.85,
                border: BorderKind::Solid,
                border_color: hex(0x4c566a),
                accent_color: hex(0x88c0d0),
                warning_color: hex(0xebcb8b),
                critical_color: hex(0xbf616a),
                ..base.clone()
            },
            Theme {
                name: "Solarized".into(),
                text_color: hex(0x93a1a1),
                background: hex(0x002b36),
                opacity: 0.9,
                corner_radius: 6.,
                accent_color: hex(0x268bd2),
                warning_color: hex(0xb58900),
                critical_color: hex(0xdc322f),
                ..base.clone()
            },
            Theme {
                name: "Terminal".into(),
                text_color: hex(0x33ff66),
                background: black(),
                opacity: 0.85,
                border: BorderKind::Solid,
                border_color: hex(0x33ff66).opacity(0.5),
                corner_radius: 0.,
                font_family: "Consolas".into(),
                accent_color: hex(0x33ff66),
                warning_color: hex(0xffcc00),
                critical_color: hex(0xff3333),
                ..base.clone()
            },
            Theme {
                name: "Neon".into(),
                text_color: hex(0xf8f8ff),
                background: hex(0x14001f),
                opacity: 0.8,
                border: BorderKind::Solid,
                border_width: 2.,
                border_color: hex(0xff2bd6),
                corner_radius: 14.,
                font_weight: 600.,
                accent_color: hex(0x00f0ff),
                warning_color: hex(0xffe600),
                critical_color: hex(0xff2b4e),
                ..base.clone()
            },
            Theme {
                name: "Sunset".into(),
                text_color: hex(0xfff4e6),
                background: hex(0x3d1a2e),
                opacity: 0.8,
                corner_radius: 20.,
                accent_color: hex(0xff8c42),
                warning_color: hex(0xffd166),
                critical_color: hex(0xef476f),
                ..base.clone()
            },
            Theme {
                name: "Minimal".into(),
                opacity: 0.,
                border: BorderKind::Dashed,
                border_color: white().opacity(0.4),
                font_weight: 300.,
                ..base
            },
        ]
    });
    &THEMES
}

/// What `themes.json` and exported files hold. Import also takes a bare theme or a bare list.
#[derive(Serialize, Deserialize)]
struct ThemeFile {
    #[serde(default)]
    app: String,
    themes: Vec<Theme>,
}

impl ThemeFile {
    fn new(themes: Vec<Theme>) -> Self {
        Self { app: crate::APP_NAME.into(), themes }
    }
}

/// The user's own themes. Changes are written to `themes.json` right away.
#[derive(Default)]
pub struct ThemeLibrary {
    pub custom: Vec<Theme>,
}

impl ThemeLibrary {
    pub fn load() -> Self {
        Self { custom: read_json::<ThemeFile>("themes.json").map(|file| file.themes).unwrap_or_default() }
    }

    /// Built-in themes first, then the user's. The flag is true for built-in ones.
    pub fn all(&self) -> impl Iterator<Item = (&Theme, bool)> {
        built_in().iter().map(|theme| (theme, true)).chain(self.custom.iter().map(|theme| (theme, false)))
    }

    fn exists(&self, name: &str) -> bool {
        self.all().any(|(theme, _)| theme.name.eq_ignore_ascii_case(name))
    }

    /// `name`, or `name 2`, `name 3`… if that's taken.
    fn unique_name(&self, name: &str) -> String {
        if !self.exists(name) {
            return name.into();
        }
        (2..).map(|n| format!("{name} {n}")).find(|candidate| !self.exists(candidate)).unwrap()
    }

    /// Adds a theme, or replaces the user's theme of the same name. A built-in name gets a
    /// number added instead. Returns the name it was saved under.
    pub fn save_theme(&mut self, mut theme: Theme) -> std::io::Result<String> {
        if let Some(existing) = self.custom.iter_mut().find(|t| t.name.eq_ignore_ascii_case(&theme.name)) {
            theme.name = existing.name.clone();
            *existing = theme.clone();
        } else {
            theme.name = self.unique_name(&theme.name);
            self.custom.push(theme.clone());
        }
        self.write()?;
        Ok(theme.name)
    }

    pub fn remove(&mut self, name: &str) -> std::io::Result<()> {
        self.custom.retain(|theme| theme.name != name);
        self.write()
    }

    /// Adds the themes in a file. Ones already here unchanged are skipped; a name that's taken
    /// by a different theme gets a number added. Returns how many were added.
    pub fn import(&mut self, path: &Path) -> Result<usize, String> {
        let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        let themes = parse(&text)?;

        let mut added = 0;
        for mut theme in themes {
            theme.name = theme.name.trim().to_string();
            if theme.name.is_empty() {
                theme.name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            }
            if self.all().any(|(existing, _)| existing.name == theme.name && existing.same_look(&theme)) {
                continue;
            }
            theme.name = self.unique_name(&theme.name);
            self.custom.push(theme);
            added += 1;
        }
        if added > 0 {
            self.write().map_err(|error| error.to_string())?;
        }
        Ok(added)
    }

    fn write(&self) -> std::io::Result<()> {
        write_json("themes.json", &ThemeFile::new(self.custom.clone())).map(|_| ())
    }
}

/// The themes in a theme file: an exported file, a bare list of themes, or a single theme.
fn parse(text: &str) -> Result<Vec<Theme>, String> {
    let json: serde_json::Value = serde_json::from_str(text).map_err(|error| format!("not JSON ({error})"))?;
    match &json {
        serde_json::Value::Object(map) if map.contains_key("themes") => {
            serde_json::from_value::<ThemeFile>(json).map(|file| file.themes)
        }
        serde_json::Value::Array(_) => serde_json::from_value(json),
        // A bare theme needs at least its name, so any other JSON object isn't taken for one.
        serde_json::Value::Object(map) if map.contains_key("name") => {
            serde_json::from_value(json).map(|theme| vec![theme])
        }
        _ => return Err("not a theme file".into()),
    }
    .map_err(|error| format!("not a theme file ({error})"))
}

/// Writes `themes` to a file that `ThemeLibrary::import` can read back.
pub fn export(themes: Vec<Theme>, path: &Path) -> std::io::Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(&ThemeFile::new(themes))?)
}

/// A file name for exporting `name`: letters, digits, spaces and dashes only.
pub fn file_name(name: &str) -> String {
    let clean: String =
        name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    format!("{}.json", clean.trim())
}

/// Where export / import dialogs start: the Documents folder if there is one.
pub fn dialog_folder() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join("Documents"))
        .filter(|folder| folder.is_dir())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A small mock widget drawn in `theme`'s look, on a dark "desktop" so see-through themes show.
pub fn preview(theme: &Theme, width: f32, height: f32) -> Div {
    let font_size = height * 0.2;
    let bar = |fill: f32, color: Hsla| {
        div().w(relative(0.8)).h(px(3.)).rounded_full().bg(color.opacity(0.25)).child(
            div().h_full().w(relative(fill)).rounded_full().bg(color),
        )
    };
    let card = div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(3.))
        .rounded(px(theme.corner_radius.min(height / 2.)))
        .bg(Hsla { a: theme.opacity, ..theme.background })
        .text_color(theme.text_color)
        .text_size(px(font_size))
        .font_weight(FontWeight(theme.font_weight))
        .when(!theme.font_family.is_empty(), |card| card.font_family(theme.font_family.clone()))
        .when(theme.border != BorderKind::None, |card| {
            card.border(px(theme.border_width)).border_color(theme.border_color)
        })
        .when(theme.border == BorderKind::Dashed, |card| card.border_dashed())
        .child(div().text_size(px(font_size * 0.45)).opacity(0.8).child("CPU"))
        .child("42%")
        .child(bar(0.42, theme.accent_color))
        .child(
            div()
                .flex()
                .gap_1()
                .child(div().size(px(6.)).rounded_full().bg(theme.warning_color))
                .child(div().size(px(6.)).rounded_full().bg(theme.critical_color)),
        );

    div()
        .w(px(width))
        .h(px(height))
        .flex_none()
        .p(px(height * 0.1))
        .rounded_md()
        .bg(linear_gradient(
            135.,
            linear_color_stop(hex(0x3a4a6b), 0.),
            linear_color_stop(hex(0x6b3a5a), 1.),
        ))
        .child(card)
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings in GPUI's own `test` attribute.
    use super::{Theme, ThemeFile, ThemeLibrary, built_in, parse};

    #[test]
    fn exported_file_reads_back() {
        let themes = built_in().to_vec();
        let text = serde_json::to_string_pretty(&ThemeFile::new(themes.clone())).unwrap();
        let read = parse(&text).unwrap();
        assert_eq!(read.len(), themes.len());
        for (read, original) in read.iter().zip(&themes) {
            assert_eq!(read.name, original.name);
            assert!(read.same_look(original));
        }
    }

    #[test]
    fn reads_a_bare_theme_or_list() {
        let theme = serde_json::to_string(&built_in()[3]).unwrap();
        assert_eq!(parse(&theme).unwrap()[0].name, "Midnight");
        assert_eq!(parse(&format!("[{theme}, {theme}]")).unwrap().len(), 2);
        // Missing fields fall back to the default look.
        assert_eq!(parse(r#"{"name": "Mine", "opacity": 0.3}"#).unwrap()[0].opacity, 0.3);
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"widgets": []}"#).is_err());
        assert!(parse("42").is_err());
    }

    #[test]
    fn names_stay_unique() {
        let mut library = ThemeLibrary::default();
        library.custom.push(Theme { name: "Mine".into(), ..Theme::default() });
        assert_eq!(library.unique_name("Neon"), "Neon 2");
        assert_eq!(library.unique_name("mine"), "mine 2");
        assert_eq!(library.unique_name("Fresh"), "Fresh");
    }
}
