use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::*;

mod glass;
mod widget;
mod widget_settings;

use widget::WidgetStyle;
use widget_settings::WidgetSettings;

struct HelloWorld {
    count: i32,
    widget: Option<AnyWindowHandle>,
    widget_style: Entity<WidgetStyle>,
    widget_settings: Entity<WidgetSettings>,
}

impl HelloWorld {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // One shared style: the settings panel writes to it, the widget reads from it.
        let widget_style = cx.new(|_| WidgetStyle::load());
        let widget_settings = cx.new(|cx| WidgetSettings::new(widget_style.clone(), window, cx));

        Self { count: 15, widget: None, widget_style, widget_settings }
    }

    fn toggle_widget(&mut self, cx: &mut App) {
        if let Some(handle) = self.widget.take() {
            handle.update(cx, |_, window, _| window.remove_window()).ok();
        } else {
            self.widget = Some(widget::open(self.widget_style.clone(), cx));
        }
    }
}

impl Render for HelloWorld {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("main")
            .flex()
            .flex_col()
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .child("Hello, World!")
                    .child(
                        Button::new("hello")
                            .primary()
                            .label("Click me")
                            .on_click(cx.listener(|this, event, window, cx| {
                                println!("Clicked! {:?} \n {:?}", event, window.mouse_position());
                                this.count += 1;
                                cx.notify();
                            })),
                    )
                    .child(format!("Count: {}", self.count))
                    .child(
                        Button::new("toggle-widget")
                            .label(if self.widget.is_some() { "Hide widget" } else { "Show widget" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_widget(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(self.widget_settings.clone())
    }
}

fn main() {
    env_logger::init();
    application().with_assets(assets::Assets).run(|cx| {
        init(cx);

        let bounds = Bounds::centered(None, size(px(520.), px(860.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        open_window(options, cx, |window, cx| cx.new(|cx| HelloWorld::new(window, cx)))
            .expect("Failed to open window");
    });
}
