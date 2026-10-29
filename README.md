# gpui-hello

A small [GPUI](https://www.gpui.rs/) app for learning Rust: desktop widgets (clock, CPU, RAM, disk
space, network), similar to Rainmeter skins, with a dashboard window to add, remove and style them,
and a tray icon.

## Requirements

- [Rust](https://rustup.rs/) (installed with `rustup`, which also installs `cargo`)
- Windows 10 or 11. The desktop widget uses Windows APIs; the main panel builds on other platforms too.

## Running

From the project folder:

```bash
cargo run
```

The first build downloads and compiles every dependency, so it takes a few minutes. Later builds
only recompile your own code.

Closing the dashboard only hides it: the widgets keep running and the app stays in the tray
(notification area). Click the tray icon to open the dashboard again. To stop the app, use **Quit**
in the dashboard or right-click the tray icon → **Quit** (or press `Ctrl+C` in the terminal).

Run with `--hidden` to start with only the widgets and the tray icon:

```bash
cargo run -- --hidden
```

### Other useful commands

| Command | What it does |
|---|---|
| `cargo run` | Build (debug) and start the app |
| `cargo check` | Only check that the code compiles; faster than a full build |
| `cargo build` | Build without starting; the program lands in `target/debug/gpui-hello.exe` |
| `cargo run --release` | Optimized build: slower to compile, faster to run |

## Project layout

```
Cargo.toml      dependencies and build settings (like package.json)
src/
├── main.rs             entry point: stats reader, dashboard window, tray icon
├── dashboard.rs        the list of widgets and their settings; saving/loading dashboard.json
├── dashboard_view.rs   the dashboard window: widget list on the left, options on the right
├── stats.rs            reads CPU, RAM, disk, network and temperature once a second, keeps history
├── tray.rs             tray icon, hiding/showing the dashboard, "Start with Windows" (Windows only)
├── widget.rs           the desktop widget window, its WidgetStyle, graphs, and the Windows pinning code
├── widget_settings.rs  the style editor: sliders, border type and color pickers
├── glass.rs            the glass effect: crops and blurs a picture of the desktop behind a widget
└── widgets/            one file per widget type, each implementing the `WidgetType` trait
    ├── mod.rs          the trait and the kind → type lookup
    ├── clock.rs  cpu.rs  ram.rs  disk.rs  network.rs
```

## The dashboard

The window that opens is the dashboard. On the left is the list of widgets, each with a switch
that shows or hides it on the desktop; click a widget to see its options on the right. **Add widget**
adds a new one (several of the same type are fine, e.g. two Disk widgets for C: and D:), and
**Remove widget** on its options page removes it.

| Widget | Shows | Options |
|---|---|---|
| Clock | time and date | |
| CPU | usage in %, and the temperature | show temperature, history graph |
| RAM | used / total and % | history graph |
| Disk space | free space on one drive | which drive |
| Network | download and upload speed (all adapters) | history graph |

The **history graph** replaces the usage bar with a small chart of the last minute.

Each widget either uses the **Shared style** (default) or, with **Use shared style** turned off,
its own style. A style covers size, padding, corner radius, font size, background opacity,
border (none / solid / dashed) and width, glass blur, and the text, background and border colors.

**Start with Windows** (in the dashboard header) adds the app to your sign-in programs (the
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` registry key), started with `--hidden`.
It records the path of the program you're running now, so use it from a release build
(`cargo build --release`, then run `target\release\gpui-hello.exe`): `cargo clean` or moving
the project would break a path into `target\debug`.

**Save** writes everything (which widgets are shown, their options, styles and positions) to
`%APPDATA%\gpui-hello\dashboard.json`, and the app loads it on the next start. Delete that file to
go back to the defaults. A `widget-style.json` from an older version is picked up as the shared style.

## The desktop widgets

- No title bar, no taskbar button, and a see-through background.
- They stay below all normal windows, directly above the desktop wallpaper, and stay visible
  after Win+D (show desktop).
- Drag one anywhere to move it; click **Save** in the dashboard to keep the new position.
- **Glass blur** blurs the desktop behind the widget. Windows' own blur effects don't work on
  GPUI windows, so the app takes a picture of the desktop and blurs it itself. The picture is
  retaken every 10 seconds, so a wallpaper or icon change shows up with a short delay.
- **CPU temperature**: Windows has no standard way to read it. The app asks WMI
  (`MSAcpi_ThermalZoneTemperature`), which usually only answers apps run as administrator, and
  often reports a motherboard "thermal zone" rather than the exact CPU core temperature. When
  there's no answer the widget shows "temp n/a".

To add a new widget type: add a variant to `WidgetKind` in `dashboard.rs`, a file in
`src/widgets/` with a struct implementing `WidgetType`, and a line in `widget_type()`.

## Notes

- `Cargo.toml` turns off debug assertions for the `gpui-pre-windows` crate. On this machine the
  Direct3D 11 debug layer is mismatched (error `0x887A002D`), and GPUI asks for it whenever debug
  assertions are on. Turning them off for that one crate avoids the error.
