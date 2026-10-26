use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::*;

mod widget;

struct HelloWorld {
    count: i32,
    widget: Option<AnyWindowHandle>,
}

impl HelloWorld {
    fn toggle_widget(&mut self, cx: &mut App) {
        if let Some(handle) = self.widget.take() {
            handle.update(cx, |_, window, _| window.remove_window()).ok();
        } else {
            self.widget = Some(widget::open(cx));
        }
    }
}

impl Render for HelloWorld {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
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
            )
    }
}

fn main() {
    env_logger::init();
    application().with_assets(assets::Assets).run(|cx| {
        init(cx);

        open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|_| HelloWorld { count: 15, widget: None })
        })
        .expect("Failed to open window");
    });
}
