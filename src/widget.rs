use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::dashboard::WidgetConfig;
use crate::glass::{self, ScreenRect};
use crate::stats::{HISTORY_LEN, SystemStats};
use crate::widgets::{Content, Graph, widget_type};

/// How often the desktop picture behind the glass is retaken, to catch wallpaper/icon changes.
const GLASS_REFRESH: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BorderKind {
    None,
    Solid,
    Dashed,
}

impl BorderKind {
    pub const ALL: [BorderKind; 3] = [BorderKind::None, BorderKind::Solid, BorderKind::Dashed];
}

/// Everything about how the widget looks. The main panel edits it, the widget redraws from it.
#[derive(Clone, Serialize, Deserialize)]
// Fields missing from an older saved file get their default value instead of failing to load.
#[serde(default)]
pub struct WidgetStyle {
    pub width: f32,
    pub height: f32,
    pub padding: f32,
    pub corner_radius: f32,
    pub font_size: f32,
    pub text_color: Hsla,
    pub background: Hsla,
    /// 0.0 = fully see-through, 1.0 = solid. Applied to `background` only.
    pub opacity: f32,
    pub border: BorderKind,
    pub border_width: f32,
    pub border_color: Hsla,
    /// Blur of the desktop behind the widget, in pixels. 0 = no glass, plain see-through.
    pub blur: f32,
}

impl Default for WidgetStyle {
    fn default() -> Self {
        Self {
            width: 200.,
            height: 100.,
            padding: 8.,
            corner_radius: 12.,
            font_size: 24.,
            text_color: white(),
            background: black(),
            opacity: 0.6,
            border: BorderKind::None,
            border_width: 1.,
            border_color: white().opacity(0.3),
            blur: 0.,
        }
    }
}

thread_local! {
    /// One desktop picture shared by all widgets (it's ~50 MB on big screens).
    static DESKTOP: RefCell<Option<(Instant, Weak<glass::Snapshot>)>> = const { RefCell::new(None) };
}

/// The shared desktop picture. Retaken when it's getting old or no widget holds it any more.
fn desktop_snapshot() -> Option<Rc<glass::Snapshot>> {
    DESKTOP.with_borrow_mut(|cached| {
        if let Some((taken, weak)) = cached
            && taken.elapsed() < GLASS_REFRESH / 2
            && let Some(snapshot) = weak.upgrade()
        {
            return Some(snapshot);
        }
        let snapshot = Rc::new(win32::capture_desktop()?);
        *cached = Some((Instant::now(), Rc::downgrade(&snapshot)));
        Some(snapshot)
    })
}

/// One widget window on the desktop: a clock, CPU, RAM or disk, depending on its config.
struct DesktopWidget {
    config: Entity<WidgetConfig>,
    shared_style: Entity<WidgetStyle>,
    stats: Entity<SystemStats>,
    /// Picture of the desktop, kept so moving the widget only needs a new crop + blur.
    desktop: Option<Rc<glass::Snapshot>>,
    /// The blurred background currently shown, if the glass effect is on.
    glass: Option<Arc<RenderImage>>,
    _subscriptions: Vec<Subscription>,
    _refresh: Task<()>,
}

impl DesktopWidget {
    fn new(
        config: Entity<WidgetConfig>,
        shared_style: Entity<WidgetStyle>,
        stats: Entity<SystemStats>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let own_style = config.read(cx).style.clone();
        let subscriptions = vec![
            // Any setting or either style changed: resize if needed and redraw.
            cx.observe_in(&config, window, |this, _, window, cx| this.restyle(window, cx)),
            cx.observe_in(&own_style, window, |this, _, window, cx| this.restyle(window, cx)),
            cx.observe_in(&shared_style, window, |this, _, window, cx| this.restyle(window, cx)),
            // New numbers every second (this also keeps the clock ticking).
            cx.observe(&stats, |_, _, cx| cx.notify()),
            // Dragged or resized: remember where, and what's behind it changed.
            cx.observe_window_bounds(window, |this, window, cx| {
                let origin = window.bounds().origin;
                // No `cx.notify()` in here: the position needs saving, not redrawing.
                this.config.update(cx, |config, _| config.position = origin);
                this.update_glass(window, cx);
            }),
        ];

        let refresh = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(GLASS_REFRESH).await;
                let updated = this.update_in(cx, |this, window, cx| {
                    this.desktop = None;
                    this.update_glass(window, cx);
                });
                if updated.is_err() {
                    break; // the widget was closed
                }
            }
        });

        let mut this = Self {
            config,
            shared_style,
            stats,
            desktop: None,
            glass: None,
            _subscriptions: subscriptions,
            _refresh: refresh,
        };
        this.update_glass(window, cx);
        this
    }

    /// The style in use: the dashboard's shared one, or this widget's own.
    fn style<'a>(&self, cx: &'a App) -> &'a WidgetStyle {
        let config = self.config.read(cx);
        if config.use_shared_style { self.shared_style.read(cx) } else { config.style.read(cx) }
    }

    fn restyle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let style = self.style(cx);
        let wanted = size(px(style.width), px(style.height));
        if window.viewport_size() != wanted {
            window.resize(wanted);
        }
        self.update_glass(window, cx);
    }

    /// Rebuilds the blurred background for the widget's current position, size and style.
    fn update_glass(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let style = self.style(cx);
        let (blur, border, border_width) = (style.blur, style.border, style.border_width);

        let new_glass = if blur > 0. {
            if self.desktop.is_none() {
                self.desktop = desktop_snapshot();
            }
            let scale = window.scale_factor();
            // With a solid border the glass only fills the inside, so skip the border strip.
            let inset = if border == BorderKind::Solid { (border_width * scale).round() as i32 } else { 0 };
            self.desktop.as_ref().zip(win32::window_screen_rect(window)).and_then(|(desktop, rect)| {
                let inside = ScreenRect {
                    x: rect.x + inset,
                    y: rect.y + inset,
                    width: rect.width - 2 * inset,
                    height: rect.height - 2 * inset,
                };
                glass::blurred_region(desktop, inside, (blur * scale).round() as u32)
            })
        } else {
            self.desktop = None; // glass is off: let go of the big picture
            None
        };

        // Free the old image's GPU memory.
        if let Some(old) = std::mem::replace(&mut self.glass, new_glass) {
            window.drop_image(old).ok();
        }
        cx.notify();
    }

    /// What the widget shows right now, from its widget type (see `src/widgets/`).
    fn content(&self, cx: &App) -> Content {
        let config = self.config.read(cx);
        widget_type(config.kind).content(config, self.stats.read(cx))
    }
}

/// A small area chart of `graph`, newest value at the right edge, drawn with GPUI paths.
fn history_graph(graph: Graph, color: Hsla) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let points = graph.points;
            if points.len() < 2 {
                return;
            }
            let step = bounds.size.width / (HISTORY_LEN - 1) as f32;
            let first_slot = HISTORY_LEN - points.len();
            let x = |i: usize| bounds.left() + step * (first_slot + i) as f32;
            let y = |value: f32| bounds.bottom() - bounds.size.height * (value / graph.max).clamp(0., 1.);

            // Filled area under the line, then the line on top.
            let mut area = PathBuilder::fill();
            area.move_to(point(x(0), bounds.bottom()));
            for (i, value) in points.iter().enumerate() {
                area.line_to(point(x(i), y(*value)));
            }
            area.line_to(point(x(points.len() - 1), bounds.bottom()));
            area.close();
            if let Ok(path) = area.build() {
                window.paint_path(path, color.opacity(0.25));
            }

            let mut line = PathBuilder::stroke(px(1.5));
            line.move_to(point(x(0), y(points[0])));
            for (i, value) in points.iter().enumerate().skip(1) {
                line.line_to(point(x(i), y(*value)));
            }
            if let Ok(path) = line.build() {
                window.paint_path(path, color);
            }
        },
    )
}

impl Render for DesktopWidget {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Content { label, value, bar, graph } = self.content(cx);
        let show_graph = self.config.read(cx).show_graph;
        let style = self.style(cx).clone();

        // The graph replaces the bar when it's turned on and the widget has one.
        let (bar, graph) = match graph {
            Some(graph) if show_graph => (None, Some(graph)),
            _ => (bar, None),
        };

        let text = div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .child(div().text_size(px(style.font_size * 0.45)).opacity(0.8).child(label))
            .child(div().child(value))
            .children(bar.map(|fill| {
                div()
                    .w(relative(0.8))
                    .h(px(4.))
                    .rounded_full()
                    .bg(style.text_color.opacity(0.2))
                    .child(div().h_full().w(relative(fill.clamp(0., 1.))).rounded_full().bg(style.text_color))
            }))
            .children(graph.map(|graph| {
                div()
                    .w(relative(0.85))
                    .h(px(style.font_size * 1.1))
                    .child(history_graph(graph, style.text_color).size_full())
            }));

        // Layers, back to front: blurred desktop, tint (background color + opacity), text.
        let content = |radius: f32| {
            div()
                .relative()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(style.padding))
                .rounded(px(radius))
                .text_color(style.text_color)
                .text_size(px(style.font_size))
                .children(self.glass.clone().map(|glass| {
                    img(glass)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Fill)
                        .rounded(px(radius))
                }))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(radius))
                        .bg(Hsla { a: style.opacity, ..style.background }),
                )
                .child(text)
        };

        let frame = div()
            .size_full()
            .rounded(px(style.corner_radius))
            .window_control_area(WindowControlArea::Drag);

        match style.border {
            BorderKind::None => frame.child(content(style.corner_radius)),
            // Drawn as two filled shapes (border color behind, fill on top) instead of GPUI's
            // `.border()`: on a transparent window that leaves dark pixels on the outer edge.
            BorderKind::Solid => frame
                .bg(style.border_color)
                .p(px(style.border_width))
                .child(content((style.corner_radius - style.border_width).max(0.))),
            // Dashes need GPUI's own border drawing, so this one keeps `.border()`.
            BorderKind::Dashed => frame.child(
                content(style.corner_radius)
                    .border(px(style.border_width))
                    .border_color(style.border_color)
                    .border_dashed(),
            ),
        }
    }
}

/// Opens one widget window and returns its handle, so it can be closed later.
pub fn open(
    config: Entity<WidgetConfig>,
    shared_style: Entity<WidgetStyle>,
    stats: Entity<SystemStats>,
    cx: &mut App,
) -> AnyWindowHandle {
    let current = config.read(cx);
    let style = if current.use_shared_style { shared_style.read(cx) } else { current.style.read(cx) };
    let bounds = Bounds::new(current.position, size(px(style.width), px(style.height)));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        kind: WindowKind::PopUp,
        window_background: WindowBackgroundAppearance::Transparent,
        is_resizable: false,
        focus: false,
        ..Default::default()
    };

    // GPUI's own `cx.open_window`, not gpui-kit's `open_window`: gpui-kit wraps the window
    // in its `Root`, which paints the theme background and would hide the transparency.
    let handle = cx
        .open_window(options, |window, cx| {
            cx.new(|cx| DesktopWidget::new(config, shared_style, stats, window, cx))
        })
        .expect("Failed to open widget");
    // The Win32 calls make Windows send the window messages right away. If GPUI is in the middle
    // of something (creating or updating a window) it can't handle them and logs "RefCell already
    // borrowed", so they run in a task, after GPUI is done with this window.
    #[cfg(windows)]
    if let Ok(Some(hwnd)) = handle.update(cx, |_, window, _| win32::raw_hwnd(window)) {
        cx.spawn(async move |_| win32::pin_to_desktop(hwnd)).detach();
    }
    handle.into()
}

/// Keeps the widget on the desktop layer, like a Rainmeter skin set to "On desktop":
/// below every normal window, but always directly above the desktop itself, so it
/// also stays visible after Win+D (which raises the desktop over everything).
#[cfg(windows)]
mod win32 {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

    use gpui_kit::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};

    use crate::glass::{self, ScreenRect};
    use windows::Win32::Graphics::Dwm::{
        DWM_WINDOW_CORNER_PREFERENCE, DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR,
        DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_DONOTROUND, DwmSetWindowAttribute,
    };
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_SYSTEM_FOREGROUND, FindWindowW, GW_HWNDPREV, GWL_EXSTYLE, GetWindow, GetWindowLongW,
        GetWindowRect,
        HWND_BOTTOM, HWND_TOP, KillTimer, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SetTimer, SetWindowPos, WINDOWPOS, WINEVENT_OUTOFCONTEXT, WM_NCCALCSIZE,
        WM_NCDESTROY, WM_TIMER, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING, WS_EX_TOPMOST,
    };
    use windows::core::{PCWSTR, w};

    const SUBCLASS_ID: usize = 1;
    // Not in the `windows` crate: makes PrintWindow return what's actually on screen (incl. DWM content).
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);
    // Unusual id so it can't clash with GPUI's own timers on this window.
    const RECHECK_TIMER_ID: usize = 0x5EED;
    const RECHECK_TICKS: u32 = 10;
    const RECHECK_INTERVAL_MS: u32 = 100;

    // The hooks are plain functions with no `self`, so they find the widgets through these.
    // Each entry: (widget window, how many re-checks its timer has done).
    static WIDGETS: Mutex<Vec<(isize, u32)>> = Mutex::new(Vec::new());
    static FOREGROUND_HOOK: AtomicIsize = AtomicIsize::new(0);
    static PLACING: AtomicBool = AtomicBool::new(false);

    fn widget_windows() -> Vec<HWND> {
        WIDGETS.lock().unwrap().iter().map(|(hwnd, _)| HWND(*hwnd as *mut _)).collect()
    }

    fn hwnd_of(window: &Window) -> Option<HWND> {
        // GPUI's Window has its own `window_handle()`, so call the raw-window-handle one by name.
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::Win32(win32) = handle.as_raw() else { return None };
        Some(HWND(win32.hwnd.get() as *mut _))
    }

    /// The window's position and size on screen, in physical pixels.
    pub fn window_screen_rect(window: &Window) -> Option<ScreenRect> {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd_of(window)?, &mut rect) }.ok()?;
        Some(ScreenRect {
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        })
    }

    /// Takes a picture of the desktop (wallpaper + icons) without any windows on it,
    /// by asking the desktop window to draw itself into an off-screen bitmap.
    pub fn capture_desktop() -> Option<glass::Snapshot> {
        unsafe {
            let desktop = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
            let mut rect = RECT::default();
            GetWindowRect(desktop, &mut rect).ok()?;
            let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);

            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // negative = rows go top to bottom
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let snapshot = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) => {
                    let previous = SelectObject(dc, bitmap.into());
                    let drawn = PrintWindow(desktop, dc, PW_RENDERFULLCONTENT).as_bool();
                    let snapshot = drawn.then(|| glass::Snapshot {
                        left: rect.left,
                        top: rect.top,
                        width,
                        height,
                        bgra: std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize)
                            .to_vec(),
                    });
                    SelectObject(dc, previous);
                    let _ = DeleteObject(bitmap.into());
                    snapshot
                }
                Err(_) => None,
            };
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            snapshot
        }
    }

    /// The window's Win32 handle as a plain number, so it can be used outside a GPUI update.
    pub fn raw_hwnd(window: &Window) -> Option<isize> {
        hwnd_of(window).map(|hwnd| hwnd.0 as isize)
    }

    pub fn pin_to_desktop(raw_hwnd: isize) {
        let hwnd = HWND(raw_hwnd as *mut _);
        WIDGETS.lock().unwrap().push((raw_hwnd, 0));

        unsafe {
            // Windows 11 draws a thin border around every window; turn it off (no-op on Windows 10).
            let no_border = DWMWA_COLOR_NONE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &no_border as *const u32 as *const _,
                size_of::<u32>() as u32,
            );

            // Windows 11 also rounds window corners on its own; turn that off so the
            // widget's own corner radius (even 0) decides the shape.
            let no_rounding = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &no_rounding as *const DWM_WINDOW_CORNER_PREFERENCE as *const _,
                size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );

            // No window frame drawing at all: this removes the drop shadow (which outlines the
            // window's square, not the rounded widget) and the accent-colored line on top.
            let no_frame = DWMNCRP_DISABLED;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_NCRENDERING_POLICY,
                &no_frame as *const DWMNCRENDERINGPOLICY as *const _,
                size_of::<DWMNCRENDERINGPOLICY>() as u32,
            );

            // Windows asks before every move/raise; this hook answers "stay above the desktop".
            // It also answers WM_NCCALCSIZE with "the whole window is content, no frame".
            let _ = SetWindowSubclass(hwnd, Some(keep_above_desktop), SUBCLASS_ID, 0);

            // Make Windows ask WM_NCCALCSIZE again now that the hook is in place.
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );

            // Win+D changes the foreground window to the desktop, so re-place the widgets then.
            // One hook for all widgets: only the first widget installs it.
            if FOREGROUND_HOOK.load(Ordering::Relaxed) != 0 {
                place(hwnd);
                return;
            }
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground_change),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            FOREGROUND_HOOK.store(hook.0 as isize, Ordering::Relaxed);
        }

        // Place it right now (this also drops PopUp's always-on-top).
        place(hwnd);
    }

    /// The `hwndInsertAfter` that puts `hwnd` in the group of widgets directly above the
    /// desktop, or `None` when it's already in that group.
    fn slot_above_desktop(hwnd: HWND) -> Option<HWND> {
        let Ok(desktop) = (unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }) else {
            return Some(HWND_BOTTOM);
        };
        // Walk up from the desktop over the other widgets, so widgets don't keep pushing
        // each other out of the spot right above the desktop.
        let widgets = widget_windows();
        let mut above = unsafe { GetWindow(desktop, GW_HWNDPREV) };
        while let Ok(window) = above {
            if window == hwnd {
                return None;
            }
            if !widgets.contains(&window) {
                break;
            }
            above = unsafe { GetWindow(window, GW_HWNDPREV) };
        }
        match above {
            Ok(window) if !is_topmost(window) => Some(window),
            // Nothing normal above the desktop (e.g. after Win+D): go to the top of the normal windows.
            _ => Some(HWND_TOP),
        }
    }

    fn is_topmost(hwnd: HWND) -> bool {
        let style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
        style & WS_EX_TOPMOST.0 != 0
    }

    fn place(hwnd: HWND) {
        let Some(after) = slot_above_desktop(hwnd) else { return };
        PLACING.store(true, Ordering::Relaxed);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        PLACING.store(false, Ordering::Relaxed);
    }

    unsafe extern "system" fn on_foreground_change(
        _hook: HWINEVENTHOOK,
        _event: u32,
        _hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        // The event fires before the desktop actually moves (Win+D raising it, or dropping it
        // back on restore), so keep re-checking for a short moment afterwards.
        WIDGETS.lock().unwrap().iter_mut().for_each(|(_, rechecks)| *rechecks = 0);
        for widget in widget_windows() {
            place(widget);
            unsafe { SetTimer(Some(widget), RECHECK_TIMER_ID, RECHECK_INTERVAL_MS, None) };
        }
    }

    unsafe extern "system" fn keep_above_desktop(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            // "The client area is the whole window": no frame left for Windows to draw.
            // Returned before GPUI sees it, because GPUI keeps the side and bottom frame.
            WM_NCCALCSIZE if wparam.0 != 0 => return LRESULT(0),
            WM_WINDOWPOSCHANGING => {
                let pos = unsafe { &mut *(lparam.0 as *mut WINDOWPOS) };
                match slot_above_desktop(hwnd) {
                    Some(after) => {
                        pos.hwndInsertAfter = after;
                        pos.flags &= !SWP_NOZORDER;
                    }
                    None => pos.flags |= SWP_NOZORDER,
                }
            }
            // Activation (e.g. clicking the widget) ignores the answer above, so put it back.
            WM_WINDOWPOSCHANGED => {
                let pos = unsafe { &*(lparam.0 as *const WINDOWPOS) };
                if !pos.flags.contains(SWP_NOZORDER) && !PLACING.load(Ordering::Relaxed) {
                    place(hwnd);
                }
            }
            WM_TIMER if wparam.0 == RECHECK_TIMER_ID => {
                place(hwnd);
                let done = {
                    let mut widgets = WIDGETS.lock().unwrap();
                    let entry = widgets.iter_mut().find(|(widget, _)| *widget == hwnd.0 as isize);
                    entry.map_or(RECHECK_TICKS, |(_, rechecks)| {
                        *rechecks += 1;
                        *rechecks
                    })
                };
                if done >= RECHECK_TICKS {
                    let _ = unsafe { KillTimer(Some(hwnd), RECHECK_TIMER_ID) };
                }
                return LRESULT(0);
            }
            WM_NCDESTROY => unsafe {
                let _ = KillTimer(Some(hwnd), RECHECK_TIMER_ID);
                let _ = RemoveWindowSubclass(hwnd, Some(keep_above_desktop), SUBCLASS_ID);
                let none_left = {
                    let mut widgets = WIDGETS.lock().unwrap();
                    widgets.retain(|(widget, _)| *widget != hwnd.0 as isize);
                    widgets.is_empty()
                };
                // Last widget gone: stop listening for foreground changes.
                if none_left {
                    let hook = FOREGROUND_HOOK.swap(0, Ordering::Relaxed);
                    if hook != 0 {
                        let _ = UnhookWinEvent(HWINEVENTHOOK(hook as *mut _));
                    }
                }
            },
            _ => {}
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}

/// Other platforms: no desktop pinning and no glass (there's no desktop picture to blur).
#[cfg(not(windows))]
mod win32 {
    use gpui_kit::Window;

    use crate::glass::{Snapshot, ScreenRect};

    pub fn capture_desktop() -> Option<Snapshot> {
        None
    }

    pub fn window_screen_rect(_: &Window) -> Option<ScreenRect> {
        None
    }
}
