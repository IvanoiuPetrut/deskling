use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::*;

use crate::widget::{BorderKind, WidgetStyle};

/// One slider row: its label, its UI state, and how to read its value from the style.
struct NumberSetting {
    label: &'static str,
    unit: &'static str,
    slider: Entity<SliderState>,
    get: fn(&WidgetStyle) -> f32,
}

/// The style editor in the dashboard (sliders, border type, colors) for one `WidgetStyle`:
/// the shared style or one widget's own. Every change is written straight into it, and the
/// widgets using it redraw themselves.
pub struct WidgetSettings {
    style: Entity<WidgetStyle>,
    numbers: Vec<NumberSetting>,
    text_color: Entity<ColorPickerState>,
    background: Entity<ColorPickerState>,
    border_color: Entity<ColorPickerState>,
    _subscriptions: Vec<Subscription>,
}

impl WidgetSettings {
    pub fn new(style: Entity<WidgetStyle>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subs = Vec::new();

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
        ];

        let current = style.read(cx).clone();
        let text_color = Self::color(&style, current.text_color, |s, c| s.text_color = c, &mut subs, window, cx);
        let background = Self::color(&style, current.background, |s, c| s.background = c, &mut subs, window, cx);
        let border_color = Self::color(&style, current.border_color, |s, c| s.border_color = c, &mut subs, window, cx);

        Self { style, numbers, text_color, background, border_color, _subscriptions: subs }
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
    }
}
