# Deskling

Desktop widgets for Windows, written in Rust with [GPUI](https://www.gpui.rs/) while learning both: (clock, CPU, RAM, disk
space, network, batteries, now playing, weather), similar to Rainmeter skins, with a dashboard window to add, remove and style them,
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
| `cargo build` | Build without starting; the program lands in `target/debug/deskling.exe` |
| `cargo run --release` | Optimized build: slower to compile, faster to run |

## Project layout

```
Cargo.toml      dependencies and build settings (like package.json)
src/
├── main.rs             entry point: stats reader, dashboard window, tray icon
├── battery.rs          reads this PC's battery and Bluetooth devices' battery levels (Windows)
├── dashboard.rs        the list of widgets and their settings; saving/loading dashboard.json
├── dashboard_view.rs   the dashboard window: widget list on the left, options on the right
├── stats.rs            reads CPU, RAM, disk, network and temperature once a second, batteries every 15 s
├── weather.rs          fetches the weather from Open-Meteo every 15 minutes, and the city search
├── tray.rs             tray icon, hiding/showing the dashboard, "Start with Windows" (Windows only)
├── widget.rs           the desktop widget window, its WidgetStyle, graphs, and the Windows pinning code
├── widget_settings.rs  the style editor: theme picker, sliders, border type and color pickers
├── theme.rs            built-in themes, the user's themes (themes.json), import/export, previews
├── media.rs            reads what's playing (title, artist, cover, position) and sends play/pause/skip (Windows)
├── glass.rs            the glass effect: crops and blurs a picture of the desktop behind a widget
└── widgets/            one file per widget type, each implementing the `WidgetType` trait
    ├── mod.rs          the trait and the kind → type lookup
    ├── clock.rs  cpu.rs  ram.rs  disk.rs  network.rs  battery.rs  now_playing.rs  weather.rs
```

## The dashboard

The window that opens is the dashboard. On the left is the list of widgets, each with a switch
that shows or hides it on the desktop; click a widget to see its options on the right. **Add widget**
adds a new one (several of the same type are fine, e.g. two Disk widgets for C: and D:), and
**Remove widget** on its options page removes it.

| Widget | Shows | Options |
|---|---|---|
| Clock | time and date | 12/24-hour, seconds, date format, another time zone (UTC offset) |
| CPU | usage in %, and the temperature | show temperature, history graph |
| RAM | used / total and % | history graph |
| Disk space | free space on one drive | which drive |
| Network | download and upload speed (all adapters) | history graph |
| Batteries | one row per battery: this PC's, and Bluetooth mice, keyboards, headsets | which devices, your own names, switched-off devices |
| Now playing | cover art, title, artist, progress, and previous / play-pause / next buttons | cover art, buttons |
| Weather | temperature and sky in one city, with an icon; optionally today and the next 2 days | city, °C / °F, forecast rows |

The **history graph** replaces the usage bar with a small chart of the last minute.

Every widget also has:

- **Label**: your own text instead of the automatic one (e.g. "Tokyo" for a world clock).
- **Show label / value / bar**: hide the parts you don't want.
- **Lock position**: the widget can't be dragged.
- **Color by usage** (CPU, RAM, disk): above the warning or critical % the value, bar and graph
  switch to the style's Warning / Critical colors. For disks it's the used space.

The **Batteries** widget lists the battery levels Windows shows in Settings → Bluetooth &
devices, plus the laptop's own battery (with "charging"). It reads them every 15 seconds and
grows taller to fit every row. Devices that are switched off are left out unless you turn on
**Also show devices that are switched off**; they're then faded, with the last level Windows heard.
Its **Low battery** colors start on (warning at 30% or less, critical at 15% or less). A device
paired twice is listed once, using the connected entry. Devices on their own USB receiver (like
Logitech Lightspeed) don't report to Windows, so they can't be listed.

The **Now playing** widget shows what Windows shows in its media pop-up (next to the volume
overlay): Spotify, browsers, Media Player and most other players. Its buttons work like that
pop-up's; a button the app doesn't allow right now (like "previous" on the first track) is faded.
When several apps are playing, Windows picks which one. The progress bar only shows when the app
reports the track's length (live streams don't). The widget is at least wide enough for the cover
and the title, whatever the style's width; its buttons can be clicked even when it can be dragged.

The **Weather** widget uses [Open-Meteo](https://open-meteo.com/) (free, no account or API key).
Type a city in its options and pick it from the list that comes up. The weather is fetched every
15 minutes; without an internet connection the widget keeps the last weather it got and adds
"offline" to its label. **Show today and the next 2 days** adds a row per day with its icon and
lowest / highest temperature. The requests go out with the certificates Windows trusts, so they also
work behind company proxies that inspect HTTPS.

The **Layout** page has **Snap to grid** (widgets move in grid steps while you drag them, grid size
5–100 px) and **Lock all / Unlock all**.

Each widget either uses the **Shared style** (default) or, with **Use shared style** turned off,
its own style. A style covers size, padding, corner radius, font (any installed font, weight,
size, and the label's size), background opacity, border (none / solid / dashed) and width, glass
blur, and the colors: text, background, border, bars & graphs, warning and critical.

### Themes

A **theme** is a named look: colors, background opacity, border, corner radius, glass blur and
font. Applying one leaves the size, padding and font size alone, so it never breaks a layout.

- **Pick a theme** at the top of any style editor (shared style or a widget's own). The theme in
  use is highlighted. The app comes with Classic, Frosted glass, Light, Midnight, Nord,
  Solarized, Terminal, Neon, Sunset and Minimal.
- **Make your own**: style it the way you like, type a name and click **Save as theme**. Saving
  under the name of one of your themes updates it.
- The **Themes** page (under Settings) shows a preview of every theme, with **Use as shared**,
  **Use everywhere** (the shared style and every widget's own style), **Export…** and, for your
  own themes, **Delete** (click twice).
- **Import…** reads one or more theme files; **Export my themes…** writes all of yours to one
  file. A theme already there isn't added twice, and a name that's taken gets a number added.

Your themes are saved right away to `%APPDATA%\Deskling\themes.json` (applying a theme to a
style still needs **Save**). Theme files are plain JSON with colors as `#rrggbbaa`:

```json
{
  "app": "Deskling",
  "themes": [
    { "name": "Mine", "text_color": "#ffffffff", "background": "#102030ff", "opacity": 0.8,
      "border": "Solid", "border_color": "#ffffff55", "corner_radius": 12.0, "accent_color": "#4cc9f0ff" }
  ]
}
```

Fields left out get the Classic value. A file with a single theme object, or a plain list of
themes, imports too.

**Start with Windows** (on the **General** page, under Settings) adds the app to your sign-in programs (the
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` registry key), started with `--hidden`.
It records the path of the program you're running now, so use it from a release build
(`cargo build --release`, then run `target\release\deskling.exe`): `cargo clean` or moving
the project would break a path into `target\debug`.

**Save** writes everything (which widgets are shown, their options, styles and positions) to
`%APPDATA%\Deskling\dashboard.json`, and the app loads it on the next start. Delete that file to
go back to the defaults. Settings saved before the rename (in `%APPDATA%\gpui-hello`) are still picked up until you
save, and a `widget-style.json` from an older version becomes the shared style.

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
