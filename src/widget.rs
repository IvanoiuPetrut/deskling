use gpui_kit::*;

#[derive(Clone, Copy, PartialEq)]
pub enum BorderKind {
    None,
    Solid,
    Dashed,
}

impl BorderKind {
    pub const ALL: [BorderKind; 3] = [BorderKind::None, BorderKind::Solid, BorderKind::Dashed];
}

/// Everything about how the widget looks. The main panel edits it, the widget redraws from it.
#[derive(Clone)]
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
        }
    }
}

struct ClockWidget {
    style: Entity<WidgetStyle>,
}

impl Render for ClockWidget {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.style.read(cx);

        let mut clock = div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p(px(style.padding))
            .rounded(px(style.corner_radius))
            .bg(Hsla { a: style.opacity, ..style.background })
            .text_color(style.text_color)
            .text_size(px(style.font_size))
            .window_control_area(WindowControlArea::Drag)
            .child("12:34");

        if style.border != BorderKind::None {
            clock = clock.border(px(style.border_width)).border_color(style.border_color);
        }
        if style.border == BorderKind::Dashed {
            clock = clock.border_dashed();
        }
        clock
    }
}

/// Opens the widget window and returns its handle, so it can be closed later.
pub fn open(style: Entity<WidgetStyle>, cx: &mut App) -> AnyWindowHandle {
    let current = style.read(cx);
    let bounds = Bounds::new(
        point(px(100.), px(100.)),
        size(px(current.width), px(current.height)),
    );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        kind: WindowKind::PopUp,
        window_background: WindowBackgroundAppearance::Transparent,
        is_resizable: false,
        ..Default::default()
    };

    // GPUI's own `cx.open_window`, not gpui-kit's `open_window`: gpui-kit wraps the window
    // in its `Root`, which paints the theme background and would hide the transparency.
    let handle = cx
        .open_window(options, |window, cx| {
            #[cfg(windows)]
            win32::pin_to_desktop(window);
            cx.new(|cx| {
                // Whenever the style changes: resize the window if needed, then redraw.
                cx.observe_in(&style, window, |_, style, window, cx| {
                    let style = style.read(cx);
                    let wanted = size(px(style.width), px(style.height));
                    if window.viewport_size() != wanted {
                        window.resize(wanted);
                    }
                    cx.notify();
                })
                .detach();
                ClockWidget { style }
            })
        })
        .expect("Failed to open widget");
    handle.into()
}

/// Keeps the widget on the desktop layer, like a Rainmeter skin set to "On desktop":
/// below every normal window, but always directly above the desktop itself, so it
/// also stays visible after Win+D (which raises the desktop over everything).
#[cfg(windows)]
mod win32 {
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

    use gpui_kit::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Dwm::{
        DWM_WINDOW_CORNER_PREFERENCE, DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR,
        DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_DONOTROUND, DwmSetWindowAttribute,
    };
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_SYSTEM_FOREGROUND, FindWindowW, GW_HWNDPREV, GWL_EXSTYLE, GetWindow, GetWindowLongW,
        HWND_BOTTOM, HWND_TOP, KillTimer, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SetTimer, SetWindowPos, WINDOWPOS, WINEVENT_OUTOFCONTEXT, WM_NCCALCSIZE,
        WM_NCDESTROY, WM_TIMER, WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING, WS_EX_TOPMOST,
    };
    use windows::core::{PCWSTR, w};

    const SUBCLASS_ID: usize = 1;
    // Unusual id so it can't clash with GPUI's own timers on this window.
    const RECHECK_TIMER_ID: usize = 0x5EED;
    const RECHECK_TICKS: u32 = 10;
    const RECHECK_INTERVAL_MS: u32 = 100;

    // The foreground hook is a plain function with no `self`, so it finds the widget through these.
    static WIDGET: AtomicIsize = AtomicIsize::new(0);
    static FOREGROUND_HOOK: AtomicIsize = AtomicIsize::new(0);
    static PLACING: AtomicBool = AtomicBool::new(false);
    static RECHECKS_DONE: AtomicU32 = AtomicU32::new(0);

    pub fn pin_to_desktop(window: &Window) {
        // GPUI's Window has its own `window_handle()`, so call the raw-window-handle one by name.
        let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else { return };
        let hwnd = HWND(win32.hwnd.get() as *mut _);
        WIDGET.store(hwnd.0 as isize, Ordering::Relaxed);

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

            // Win+D changes the foreground window to the desktop, so re-place the widget then.
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

    /// The `hwndInsertAfter` that puts `hwnd` directly above the desktop,
    /// or `None` when it's already there.
    fn slot_above_desktop(hwnd: HWND) -> Option<HWND> {
        let Ok(desktop) = (unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }) else {
            return Some(HWND_BOTTOM);
        };
        match unsafe { GetWindow(desktop, GW_HWNDPREV) } {
            Ok(above) if above == hwnd => None,
            Ok(above) if !is_topmost(above) => Some(above),
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
        let widget = HWND(WIDGET.load(Ordering::Relaxed) as *mut _);
        if widget.is_invalid() {
            return;
        }
        place(widget);
        // The event fires before the desktop actually moves (Win+D raising it, or dropping it
        // back on restore), so keep re-checking for a short moment afterwards.
        RECHECKS_DONE.store(0, Ordering::Relaxed);
        unsafe { SetTimer(Some(widget), RECHECK_TIMER_ID, RECHECK_INTERVAL_MS, None) };
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
                if RECHECKS_DONE.fetch_add(1, Ordering::Relaxed) + 1 >= RECHECK_TICKS {
                    let _ = unsafe { KillTimer(Some(hwnd), RECHECK_TIMER_ID) };
                }
                return LRESULT(0);
            }
            WM_NCDESTROY => unsafe {
                let _ = KillTimer(Some(hwnd), RECHECK_TIMER_ID);
                let _ = RemoveWindowSubclass(hwnd, Some(keep_above_desktop), SUBCLASS_ID);
                let hook = FOREGROUND_HOOK.swap(0, Ordering::Relaxed);
                if hook != 0 {
                    let _ = UnhookWinEvent(HWINEVENTHOOK(hook as *mut _));
                }
                WIDGET.store(0, Ordering::Relaxed);
            },
            _ => {}
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}
