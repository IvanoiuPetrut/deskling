use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::*;

struct HelloWorld {
    count: i32,
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
    }
}

fn main() {
    env_logger::init();
    application().with_assets(assets::Assets).run(|cx| {
        init(cx);

        open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|_| HelloWorld { count: 15 })
        })
        .expect("Failed to open window");
    });
}
