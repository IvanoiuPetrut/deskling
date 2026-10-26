use gpui_kit::*;

struct ClockWidget;

impl Render for ClockWidget {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .rounded_xl()
            .bg(rgba(0x00000099))
            .text_color(rgb(0xffffff))
            .text_2xl()
            .window_control_area(WindowControlArea::Drag)
            .child("12:34")
    }
}

/// Opens the widget window and returns its handle, so it can be closed later.
pub fn open(cx: &mut App) -> AnyWindowHandle {
    let bounds = Bounds::new(point(px(100.), px(100.)), size(px(200.), px(100.)));
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
            cx.new(|_| ClockWidget)
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
    use windows::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DwmSetWindowAttribute};
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_SYSTEM_FOREGROUND, FindWindowW, GW_HWNDPREV, GWL_EXSTYLE, GetWindow, GetWindowLongW,
        HWND_BOTTOM, HWND_TOP, KillTimer, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetTimer,
        SetWindowPos, WINDOWPOS, WINEVENT_OUTOFCONTEXT, WM_NCDESTROY, WM_TIMER, WM_WINDOWPOSCHANGED,
        WM_WINDOWPOSCHANGING, WS_EX_TOPMOST,
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

            // Windows asks before every move/raise; this hook answers "stay above the desktop".
            let _ = SetWindowSubclass(hwnd, Some(keep_above_desktop), SUBCLASS_ID, 0);

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
