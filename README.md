# gpui-hello

A small [GPUI](https://www.gpui.rs/) app for learning Rust. It has a main panel with a counter and
a button that shows or hides a desktop widget, similar to a Rainmeter skin.

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

To stop the app, close the main window, or press `Ctrl+C` in the terminal.

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
├── main.rs             entry point: main panel with the counter, the widget toggle and the style panel
├── widget.rs           the desktop widget, its WidgetStyle, and the Windows code that pins it to the desktop
└── widget_settings.rs  the "Widget style" panel: sliders, border type and color pickers
```

## The desktop widget

Click **Show widget** in the main panel to open it, and **Hide widget** to close it.

- It has no title bar, no taskbar button, and a see-through background.
- It stays below all normal windows, directly above the desktop wallpaper.
- It stays visible after Win+D (show desktop).
- Drag it anywhere to move it.
- The **Widget style** panel in the main window changes it live: size, padding, corner radius,
  font size, background opacity, border (none / solid / dashed) and width, and the text,
  background and border colors.

It has no close button of its own. If you close the main panel while the widget is open and the
app keeps running, stop it with `Ctrl+C` in the terminal.

## Notes

- `Cargo.toml` turns off debug assertions for the `gpui-pre-windows` crate. On this machine the
  Direct3D 11 debug layer is mismatched (error `0x887A002D`), and GPUI asks for it whenever debug
  assertions are on. Turning them off for that one crate avoids the error.
