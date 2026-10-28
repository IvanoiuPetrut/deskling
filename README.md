# gpui-hello

A small [GPUI](https://www.gpui.rs/) app for learning Rust: desktop widgets (clock, CPU, RAM, disk
space), similar to Rainmeter skins, with a dashboard window to turn them on and style them.

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

To stop the app, close the dashboard window (that also closes the widgets), or press `Ctrl+C`
in the terminal.

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
├── main.rs             entry point: starts the stats reader and opens the dashboard
├── dashboard.rs        the list of widgets and their settings; saving/loading dashboard.json
├── dashboard_view.rs   the dashboard window: widget list on the left, options on the right
├── stats.rs            reads CPU, RAM, disk and temperature numbers once a second
├── widget.rs           the desktop widget window, its WidgetStyle, and the Windows code that pins it
├── widget_settings.rs  the style editor: sliders, border type and color pickers
└── glass.rs            the glass effect: crops and blurs a picture of the desktop behind a widget
```

## The dashboard

The window that opens is the dashboard. On the left is the list of widgets, each with a switch
that shows or hides it on the desktop; click a widget to see its options on the right.

| Widget | Shows | Options |
|---|---|---|
| Clock | time and date | |
| CPU | usage in %, and the temperature | show temperature |
| RAM | used / total and % | |
| Disk space | free space on one drive | which drive |

Each widget either uses the **Shared style** (default) or, with **Use shared style** turned off,
its own style. A style covers size, padding, corner radius, font size, background opacity,
border (none / solid / dashed) and width, glass blur, and the text, background and border colors.

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

## Notes

- `Cargo.toml` turns off debug assertions for the `gpui-pre-windows` crate. On this machine the
  Direct3D 11 debug layer is mismatched (error `0x887A002D`), and GPUI asks for it whenever debug
  assertions are on. Turning them off for that one crate avoids the error.
