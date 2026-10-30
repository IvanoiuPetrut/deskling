use gpui_kit::component::button::Button;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::IndexPath;
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::theme::{Theme, ThemeLibrary};
use crate::widget::{BorderKind, WidgetStyle};

/// First entry of the font list; picking it means "the default font" (an empty `font_family`).
const DEFAULT_FONT: &str = "Default";

type FontList = SearchableVec<SharedString>;

/// One slider row: its label, its UI state, and how to read its value from the style.
struct NumberSetting {
    label: &'static str,
    unit: &'static str,
    slider: Entity<SliderState>,
    get: fn(&WidgetStyle) -> f32,
}

/// The style editor in the dashboard (theme, sliders, border type, colors) for one `WidgetStyle`:
/// the shared style or one widget's own. Every change is written straight into it, and the
/// widgets using it redraw themselves.
pub struct WidgetSettings {
    style: Entity<WidgetStyle>,
    themes: Entity<ThemeLibrary>,
    /// Name for "Save as theme".
    theme_name: Entity<InputState>,
    /// Result of the last "Save as theme" click.
    theme_status: Option<SharedString>,
    numbers: Vec<NumberSetting>,
    text_color: Entity<ColorPickerState>,
    background: Entity<ColorPickerState>,
    border_color: Entity<ColorPickerState>,
    accent_color: Entity<ColorPickerState>,
    warning_color: Entity<ColorPickerState>,
    critical_color: Entity<ColorPickerState>,
    font: Entity<SelectState<FontList>>,
    _subscriptions: Vec<Subscription>,
}

impl WidgetSettings {
    pub fn new(
        style: Entity<WidgetStyle>,
        themes: Entity<ThemeLibrary>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Redraw the theme list when themes are added, imported or deleted.
        let mut subs = vec![cx.observe(&themes, |_, _, cx| cx.notify())];

        #[rustfmt::skip]
        let numbers = vec![
            Self::number(&style, "Width", "px", (80., 600., 1.), |s| s.width, |s, v| s.width = v, &mut subs, cx),
            Self::number(&style, "Height", "px", (40., 400., 1.), |s| s.height, |s, v| s.height = v, &mut subs, cx),
            Self::number(&style, "Padding", "px", (0., 48., 1.), |s| s.padding, |s, v| s.padding = v, &mut subs, cx),
            Self::number(&style, "Corner radius", "px", (0., 60., 1.), |s| s.corner_radius, |s, v| s.corner_radius = v, &mut subs, cx),
            Self::number(&style, "Font size", "px", (8., 96., 1.), |s| s.font_size, |s, v| s.font_size = v, &mut subs, cx),
            Self::number(&style, "Background opacity", "%", (0., 100., 1.), |s| s.opacity * 100., |s, v| s.opacity = v / 100., &mut subs, cx),
            Self::number(&style, "Border width", "px", (1., 12., 1.), |s| s.border_width, |s, v| s.border_width = v, &mut subs, cx),
            Self::number(&style, "Glass blur", "px", (0., 40., 1.), |s| s.blur, |s, v| s.blur = v, &mut subs, cx),
            Self::number(&style, "Font weight", "", (100., 900., 100.), |s| s.font_weight, |s, v| s.font_weight = v, &mut subs, cx),
            Self::number(&style, "Label size", "% of font size", (20., 100., 1.), |s| s.label_size, |s, v| s.label_size = v, &mut subs, cx),
        ];

        let current = style.read(cx).clone();
        let text_color = Self::color(&style, current.text_color, |s, c| s.text_color = c, &mut subs, window, cx);
        let background = Self::color(&style, current.background, |s, c| s.background = c, &mut subs, window, cx);
        let border_color = Self::color(&style, current.border_color, |s, c| s.border_color = c, &mut subs, window, cx);
        let accent_color = Self::color(&style, current.accent_color, |s, c| s.accent_color = c, &mut subs, window, cx);
        let warning_color = Self::color(&style, current.warning_color, |s, c| s.warning_color = c, &mut subs, window, cx);
        let critical_color = Self::color(&style, current.critical_color, |s, c| s.critical_color = c, &mut subs, window, cx);
        let font = Self::font_picker(&style, &current.font_family, &mut subs, window, cx);
        let theme_name = cx.new(|cx| InputState::new(window, cx).placeholder("Theme name"));

        Self {
            style,
            themes,
            theme_name,
            theme_status: None,
            numbers,
            text_color,
            background,
            border_color,
            accent_color,
            warning_color,
            critical_color,
            font,
            _subscriptions: subs,
        }
    }

    /// Gives the style `theme`'s look, and moves the sliders and pickers to match.
    pub fn apply_theme(&mut self, theme: &Theme, window: &mut Window, cx: &mut Context<Self>) {
        self.style.update(cx, |style, cx| {
            theme.apply(style);
            cx.notify();
        });
        self.theme_status = None;

        // `set_value` doesn't fire change events, so this doesn't write back into the style.
        let style = self.style.read(cx).clone();
        for number in &self.numbers {
            let value = (number.get)(&style);
            number.slider.update(cx, |slider, cx| slider.set_value(value, window, cx));
        }
        let colors = [
            (&self.text_color, style.text_color),
            (&self.background, style.background),
            (&self.border_color, style.border_color),
            (&self.accent_color, style.accent_color),
            (&self.warning_color, style.warning_color),
            (&self.critical_color, style.critical_color),
        ];
        for (picker, color) in colors {
            picker.update(cx, |picker, cx| picker.set_value(color, window, cx));
        }
        let font: SharedString =
            if style.font_family.is_empty() { DEFAULT_FONT.into() } else { style.font_family.into() };
        self.font.update(cx, |picker, cx| picker.set_selected_value(&font, window, cx));
        cx.notify();
    }

    /// Saves the current look as a theme under the typed name.
    fn save_as_theme(&mut self, cx: &mut Context<Self>) {
        let name = self.theme_name.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.theme_status = Some("Type a name for the theme first.".into());
            cx.notify();
            return;
        }
        let theme = Theme::from_style(&name, self.style.read(cx));
        let saved = self.themes.update(cx, |themes, cx| {
            let saved = themes.save_theme(theme);
            cx.notify();
            saved
        });
        self.theme_status = Some(match saved {
            Ok(name) => format!("Saved as theme \"{name}\".").into(),
            Err(error) => format!("Could not save the theme: {error}").into(),
        });
        cx.notify();
    }

    /// One button per theme (the one in use is highlighted), then "Save as theme".
    fn theme_picker(&self, style: &WidgetStyle, cx: &mut Context<Self>) -> Div {
        let themes: Vec<Theme> = self.themes.read(cx).all().map(|(theme, _)| theme.clone()).collect();
        let chips = themes.into_iter().enumerate().map(|(index, theme)| {
            let active = theme.matches(style);
            let swatch = div()
                .size(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(black().opacity(0.2))
                .bg(theme.background)
                .child(div().size(px(6.)).rounded_full().bg(theme.accent_color));
            div()
                .id(("theme", index))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(black().opacity(if active { 0.5 } else { 0.1 }))
                .when(active, |chip| chip.bg(black().opacity(0.07)))
                .hover(|chip| chip.bg(black().opacity(0.04)))
                .cursor_pointer()
                .text_sm()
                .child(swatch)
                .child(theme.name.clone())
                .on_click(cx.listener(move |this, _, window, cx| this.apply_theme(&theme, window, cx)))
        });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child("Theme")
            .child(div().flex().flex_wrap().gap_2().children(chips))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(Input::new(&self.theme_name)))
                    .child(
                        Button::new("save-theme")
                            .label("Save as theme")
                            .on_click(cx.listener(|this, _, _, cx| this.save_as_theme(cx))),
                    ),
            )
            .children(self.theme_status.clone().map(|status| div().text_sm().opacity(0.7).child(status)))
            .child(div().text_sm().opacity(0.7).child(
                "A theme sets colors, border, corners, glass and font, not the size. Saving under the \
                 name of one of your own themes updates it.",
            ))
    }

    /// Makes a slider that edits one number in the style.
    #[allow(clippy::too_many_arguments)]
    fn number(
        style: &Entity<WidgetStyle>,
        label: &'static str,
        unit: &'static str,
        (min, max, step): (f32, f32, f32),
        get: fn(&WidgetStyle) -> f32,
        set: fn(&mut WidgetStyle, f32),
        subs: &mut Vec<Subscription>,
        cx: &mut Context<Self>,
    ) -> NumberSetting {
        let value = get(style.read(cx));
        let slider = cx.new(|_| SliderState::new().min(min).max(max).step(step).default_value(value));

        let style = style.clone();
        subs.push(cx.subscribe(&slider, move |_, _, event: &SliderEvent, cx| {
            if let SliderEvent::Change(value) = event {
                style.update(cx, |style, cx| {
                    set(style, value.start());
                    cx.notify();
                });
                cx.notify(); // redraw this panel so the label shows the new value
            }
        }));

        NumberSetting { label, unit, slider, get }
    }

    /// A searchable list of the fonts installed on this PC.
    fn font_picker(
        style: &Entity<WidgetStyle>,
        current: &str,
        subs: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<FontList>> {
        let mut fonts: Vec<SharedString> = cx.text_system().all_font_names().into_iter().map(Into::into).collect();
        fonts.sort();
        fonts.dedup();
        fonts.insert(0, DEFAULT_FONT.into());
        let current = if current.is_empty() { DEFAULT_FONT } else { current };
        let selected = fonts.iter().position(|font| font == current).map(IndexPath::new);
        let picker = cx.new(|cx| SelectState::new(SearchableVec::new(fonts), selected, window, cx).searchable(true));

        let style = style.clone();
        subs.push(cx.subscribe(&picker, move |_, _, event: &SelectEvent<FontList>, cx| {
            if let SelectEvent::Confirm(Some(font)) = event {
                let family = if font == DEFAULT_FONT { String::new() } else { font.to_string() };
                style.update(cx, |style, cx| {
                    style.font_family = family;
                    cx.notify();
                });
            }
        }));
        picker
    }

    /// Makes a color picker that edits one color in the style.
    fn color(
        style: &Entity<WidgetStyle>,
        initial: Hsla,
        set: fn(&mut WidgetStyle, Hsla),
        subs: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ColorPickerState> {
        let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(initial));

        let style = style.clone();
        subs.push(cx.subscribe(&picker, move |_, _, event: &ColorPickerEvent, cx| {
            if let ColorPickerEvent::Change(Some(color)) = event {
                style.update(cx, |style, cx| {
                    set(style, *color);
                    cx.notify();
                });
            }
        }));

        picker
    }
}

impl Render for WidgetSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.style.read(cx).clone();
        let border_index = BorderKind::ALL.iter().position(|kind| *kind == style.border);

        div()
            .flex()
            .flex_col()
            .gap_4()
            .w_full()
            .child(self.theme_picker(&style, cx))
            .child(div().flex().flex_col().gap_1().child("Font").child(Select::new(&self.font)))
            .children(self.numbers.iter().map(|number| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(format!("{}: {:.0}{}", number.label, (number.get)(&style), number.unit))
                    .child(Slider::new(&number.slider))
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child("Border")
                    .child(
                        RadioGroup::horizontal("border-kind")
                            .children(["None", "Solid", "Dashed"])
                            .selected_index(border_index)
                            .on_click(cx.listener(|this, index: &usize, _, cx| {
                                this.style.update(cx, |style, cx| {
                                    style.border = BorderKind::ALL[*index];
                                    cx.notify();
                                });
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .child(ColorPicker::new(&self.text_color).label("Text"))
                    .child(ColorPicker::new(&self.background).label("Background"))
                    .child(ColorPicker::new(&self.border_color).label("Border")),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .child(ColorPicker::new(&self.accent_color).label("Bars & graphs"))
                    .child(ColorPicker::new(&self.warning_color).label("Warning"))
                    .child(ColorPicker::new(&self.critical_color).label("Critical")),
            )
    }
}
