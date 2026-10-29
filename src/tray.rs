//! The notification-area (tray) icon, showing/hiding the dashboard window, and "Start with Windows".
//!
//! GPUI has no tray support, so this talks to Windows directly: a hidden message-only window
//! receives the icon's clicks, and sends them to the app through a channel.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, GetCursorPos,
    HWND_MESSAGE, IDI_APPLICATION, LoadIconW, MF_STRING, RegisterClassW, SW_HIDE, SW_SHOW,
    SetForegroundWindow, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
};
use windows::core::{HSTRING, PCWSTR, w};

/// What the user picked from the tray icon.
pub enum TrayCommand {
    OpenDashboard,
    Quit,
}

/// The message Windows sends our hidden window when something happens on the icon.
const TRAY_MESSAGE: u32 = WM_APP + 1;
const MENU_OPEN: usize = 1;
const MENU_QUIT: usize = 2;

/// The window procedure is a plain function with no `self`, so it reaches the app through this.
static COMMANDS: Mutex<Option<Sender<TrayCommand>>> = Mutex::new(None);

/// The tray icon. Removed from the notification area when this is dropped.
pub struct Tray {
    window: HWND,
    pub commands: Receiver<TrayCommand>,
}

impl Tray {
    pub fn new() -> Option<Self> {
        let (sender, commands) = channel();
        *COMMANDS.lock().unwrap() = Some(sender);

        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(tray_window_proc),
                hInstance: instance.into(),
                lpszClassName: w!("GpuiHelloTray"),
                ..Default::default()
            };
            RegisterClassW(&class);
            // A message-only window: never visible, it only receives the icon's messages.
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("GpuiHelloTray"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;

            let mut icon = icon_data(window);
            icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            icon.uCallbackMessage = TRAY_MESSAGE;
            icon.hIcon = LoadIconW(None, IDI_APPLICATION).ok()?;
            let tip: Vec<u16> = "Desktop widgets".encode_utf16().collect();
            icon.szTip[..tip.len()].copy_from_slice(&tip);
            Shell_NotifyIconW(NIM_ADD, &icon).ok().ok()?;

            Some(Self { window, commands })
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(self.window));
        }
    }
}

fn icon_data(window: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        ..Default::default()
    }
}

fn send(command: TrayCommand) {
    if let Some(sender) = COMMANDS.lock().unwrap().as_ref() {
        let _ = sender.send(command);
    }
}

unsafe extern "system" fn tray_window_proc(window: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == TRAY_MESSAGE {
        // Which mouse event happened on the icon is in the low bits of lparam.
        match lparam.0 as u32 & 0xFFFF {
            WM_LBUTTONUP => send(TrayCommand::OpenDashboard),
            WM_RBUTTONUP => unsafe { show_menu(window) },
            _ => {}
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(window, msg, wparam, lparam) }
}

unsafe fn show_menu(window: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, MENU_OPEN, w!("Open dashboard"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit"));
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        // Without this, the menu doesn't close when you click somewhere else.
        let _ = SetForegroundWindow(window);
        let picked = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, cursor.x, cursor.y, None, window, None);
        let _ = DestroyMenu(menu);
        match picked.0 as usize {
            MENU_OPEN => send(TrayCommand::OpenDashboard),
            MENU_QUIT => send(TrayCommand::Quit),
            _ => {}
        }
    }
}

/// A GPUI window's Win32 handle as a plain number, usable outside GPUI updates.
pub fn raw_hwnd(window: &Window) -> Option<isize> {
    // GPUI's Window has its own `window_handle()`, so call the raw-window-handle one by name.
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else { return None };
    Some(win32.hwnd.get())
}

/// Shows (and brings to the front) or hides a window by its raw Win32 handle.
pub fn set_window_visible(raw_hwnd: isize, visible: bool) {
    let window = HWND(raw_hwnd as *mut _);
    unsafe {
        let _ = ShowWindow(window, if visible { SW_SHOW } else { SW_HIDE });
        if visible {
            let _ = SetForegroundWindow(window);
        }
    }
}

// "Start with Windows" = a value under this registry key, holding the command to run at sign-in.
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const RUN_VALUE: PCWSTR = w!("GpuiHelloWidgets");

pub fn starts_with_windows() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, RRF_RT_REG_SZ, None, None, None).is_ok() }
}

/// Turns "Start with Windows" on or off for the current user. On = start this exe hidden
/// (`--hidden`: widgets and tray icon only, no dashboard).
pub fn set_starts_with_windows(enabled: bool) -> windows::core::Result<()> {
    unsafe {
        if enabled {
            let exe = std::env::current_exe().map_err(|error| windows::core::Error::new(
                windows::core::HRESULT(-1),
                error.to_string(),
            ))?;
            let command = HSTRING::from(format!("\"{}\" --hidden", exe.display()));
            // Registry strings are UTF-16 and include the closing 0.
            let bytes = (command.len() + 1) * size_of::<u16>();
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                RUN_VALUE,
                REG_SZ.0,
                Some(command.as_ptr() as *const _),
                bytes as u32,
            )
            .ok()
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE).ok()
        }
    }
}
