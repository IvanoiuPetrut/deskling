// Release builds are window programs: Windows opens no console window next to the app.
// Debug builds stay console programs, so logs still show while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpui_kit::*;

mod dashboard;
mod dashboard_view;
mod glass;
mod stats;
#[cfg(windows)]
mod tray;
mod widget;
mod widget_settings;
mod widgets;

use dashboard::Dashboard;
use dashboard_view::DashboardView;

/// Shown in the window title, the dashboard header and the tray icon, and used for the settings folder.
pub const APP_NAME: &str = "Deskling";

fn main() {
    env_logger::init();
    // `--hidden` (used by "Start with Windows"): start with only the widgets and the tray icon.
    let hidden = std::env::args().any(|arg| arg == "--hidden");

    application().with_assets(assets::Assets).run(move |cx| {
        init(cx);

        // Read CPU/RAM/disk numbers in the background; every widget watches these.
        let stats = stats::start(cx);
        let dashboard = Dashboard::load(cx);

        let bounds = Bounds::centered(None, size(px(860.), px(780.)), cx);
        #[cfg(windows)]
        let tray = tray::Tray::new();
        #[cfg(windows)]
        let has_tray = tray.is_some();
        #[cfg(not(windows))]
        let has_tray = false;

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some(APP_NAME.into()), ..Default::default() }),
            // Without a tray icon there'd be no way to bring a hidden dashboard back.
            show: !(hidden && has_tray),
            ..Default::default()
        };
        let (dashboard_window, _) = open_window(options, cx, |window, cx| {
            cx.new(|cx| DashboardView::new(dashboard, stats, has_tray, window, cx))
        })
        .expect("Failed to open window");

        #[cfg(windows)]
        if let Some(tray) = tray {
            let dashboard_hwnd = dashboard_window.update(cx, |_, window, _| tray::raw_hwnd(window)).ok().flatten();
            listen_to_tray(tray, dashboard_hwnd, cx);
        }
        #[cfg(not(windows))]
        let _ = dashboard_window;
    });
}

/// Checks a few times a second whether the tray icon was clicked, and acts on it.
#[cfg(windows)]
fn listen_to_tray(tray: tray::Tray, dashboard_hwnd: Option<isize>, cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
            while let Ok(command) = tray.commands.try_recv() {
                match command {
                    tray::TrayCommand::OpenDashboard => {
                        if let Some(hwnd) = dashboard_hwnd {
                            tray::set_window_visible(hwnd, true);
                        }
                    }
                    tray::TrayCommand::Quit => {
                        drop(tray); // removes the icon right away
                        cx.update(|cx| cx.quit());
                        return;
                    }
                }
            }
        }
    })
    .detach();
}
