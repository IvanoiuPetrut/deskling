use gpui_kit::*;

mod dashboard;
mod dashboard_view;
mod glass;
mod stats;
mod widget;
mod widget_settings;

use dashboard::Dashboard;
use dashboard_view::DashboardView;

fn main() {
    env_logger::init();
    application().with_assets(assets::Assets).run(|cx| {
        init(cx);

        // Read CPU/RAM/disk numbers in the background; every widget watches these.
        let stats = stats::start(cx);
        let dashboard = Dashboard::load(cx);

        let bounds = Bounds::centered(None, size(px(860.), px(780.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("Desktop widgets".into()), ..Default::default() }),
            ..Default::default()
        };
        open_window(options, cx, |window, cx| cx.new(|cx| DashboardView::new(dashboard, stats, window, cx)))
            .expect("Failed to open window");
    });
}
