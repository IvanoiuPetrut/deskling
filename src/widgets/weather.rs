use super::{Content, Row, WidgetType};
use crate::dashboard::WidgetConfig;
use crate::stats::SystemStats;
use crate::weather::{self, Sky};

pub struct Weather;

impl WidgetType for Weather {
    fn name(&self) -> &'static str {
        "Weather"
    }

    fn content(&self, config: &WidgetConfig, stats: &SystemStats) -> Content {
        let options = &config.weather;
        let empty = |label: String| Content {
            label,
            value: "–".into(),
            bar: None,
            graph: None,
            rows: Vec::new(),
            player: None,
            icon: None,
        };
        let Some(place) = &options.place else { return empty("Pick a city in the dashboard".into()) };
        let report = stats.weather.iter().find(|report| report.is_for(place));
        let Some((now, report)) = report.and_then(|report| Some((report.now.as_ref()?, report))) else {
            // Nothing fetched yet: still on its way, or Open-Meteo can't be reached.
            let failed = report.is_some_and(|report| report.failed);
            return empty(if failed { format!("{} · offline", place.name) } else { place.name.clone() });
        };

        let degrees = |celsius: f32| {
            let shown = if options.fahrenheit { celsius * 9. / 5. + 32. } else { celsius };
            format!("{:.0}°", shown.round() + 0.) // `+ 0.` turns -0 into 0
        };
        let rows = match options.forecast {
            true => report
                .days
                .iter()
                .take(weather::DAYS)
                .enumerate()
                .map(|(index, day)| Row {
                    name: match index {
                        0 => "Today".into(),
                        _ => day.date.format("%a").to_string(),
                    },
                    value: format!("{} / {}", degrees(day.min), degrees(day.max)),
                    fill: None,
                    dimmed: false,
                    icon: Some(icon(weather::sky(day.code), true)),
                })
                .collect(),
            false => Vec::new(),
        };
        // Fetched before but not now: the numbers may be old.
        let offline = if report.failed { " · offline" } else { "" };
        Content {
            label: format!("{} · {}{offline}", place.name, weather::describe(now.code)),
            value: degrees(now.temperature),
            bar: None,
            graph: None,
            rows,
            player: None,
            icon: Some(icon(weather::sky(now.code), now.is_day)),
        }
    }
}

/// The picture for a sky, by day or by night (the sun becomes the moon). Drawn in one color,
/// like the media buttons; the sun behind a cloud is fainter so the cloud stands out.
fn icon(sky: Sky, day: bool) -> &'static [u8] {
    match (sky, day) {
        (Sky::Clear, true) => SUN,
        (Sky::Clear, false) => MOON,
        (Sky::PartlyCloudy, true) => SUN_AND_CLOUD,
        (Sky::PartlyCloudy, false) => MOON_AND_CLOUD,
        (Sky::Cloudy, _) => CLOUD,
        (Sky::Fog, _) => FOG,
        (Sky::Drizzle, _) => DRIZZLE,
        (Sky::Rain, _) => RAIN,
        (Sky::Snow, _) => SNOW,
        (Sky::Thunder, _) => THUNDER,
    }
}

// The icons, on a 24 x 24 grid. Clouds are a few circles and a box; with precipitation the cloud
// sits higher to make room below it.
const SUN: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4.5"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"/></svg>"#;
const MOON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M20 14.6A8.2 8.2 0 0 1 9.4 4A8.4 8.4 0 1 0 20 14.6z"/></svg>"#;
const SUN_AND_CLOUD: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g opacity="0.55"><circle cx="9" cy="9" r="3.5"/><path d="M9 2.5v1.5M2.5 9h1.5M4.4 4.4l1.1 1.1M13.6 4.4l-1.1 1.1M4.4 13.6l1.1-1.1" fill="none" stroke="black" stroke-width="1.8" stroke-linecap="round"/></g><circle cx="10" cy="16" r="3.5"/><circle cx="14.5" cy="13" r="4.5"/><circle cx="18.5" cy="16.5" r="3"/><rect x="10" y="15.5" width="8.5" height="4"/></svg>"#;
const MOON_AND_CLOUD: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path opacity="0.55" d="M13 9.5A5.5 5.5 0 0 1 6.5 3A5.8 5.8 0 1 0 13 9.5z"/><circle cx="10" cy="16" r="3.5"/><circle cx="14.5" cy="13" r="4.5"/><circle cx="18.5" cy="16.5" r="3"/><rect x="10" y="15.5" width="8.5" height="4"/></svg>"#;
const CLOUD: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="8" cy="14.5" r="4.5"/><circle cx="13" cy="11" r="5.5"/><circle cx="17.5" cy="15" r="4"/><rect x="8" y="14" width="9.5" height="5"/></svg>"#;
const FOG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="8" cy="11.5" r="4"/><circle cx="12.5" cy="8.5" r="5"/><circle cx="17" cy="12" r="3.5"/><rect x="8" y="11" width="9" height="4.5"/><path d="M4.5 18.5h15M7 22h10" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"/></svg>"#;
const DRIZZLE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="8" cy="10.5" r="4.5"/><circle cx="13" cy="7" r="5.5"/><circle cx="17.5" cy="11" r="4"/><rect x="8" y="10" width="9.5" height="5"/><circle cx="8" cy="18.5" r="1.1"/><circle cx="12.5" cy="20.5" r="1.1"/><circle cx="17" cy="18.5" r="1.1"/></svg>"#;
const RAIN: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="8" cy="10.5" r="4.5"/><circle cx="13" cy="7" r="5.5"/><circle cx="17.5" cy="11" r="4"/><rect x="8" y="10" width="9.5" height="5"/><path d="M8.5 17.5l-1.2 3.5M13 17.5l-1.2 3.5M17.5 17.5l-1.2 3.5" fill="none" stroke="black" stroke-width="2" stroke-linecap="round"/></svg>"#;
const SNOW: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="8" cy="10.5" r="4.5"/><circle cx="13" cy="7" r="5.5"/><circle cx="17.5" cy="11" r="4"/><rect x="8" y="10" width="9.5" height="5"/><circle cx="7.5" cy="18" r="1.4"/><circle cx="12.5" cy="18" r="1.4"/><circle cx="17.5" cy="18" r="1.4"/><circle cx="10" cy="21.5" r="1.4"/><circle cx="15" cy="21.5" r="1.4"/></svg>"#;
const THUNDER: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="8" cy="10.5" r="4.5"/><circle cx="13" cy="7" r="5.5"/><circle cx="17.5" cy="11" r="4"/><rect x="8" y="10" width="9.5" height="5"/><path d="M13.5 15.5l-4 4.5h3l-1.5 3.5 5-5.5h-3l1.8-2.5z"/></svg>"#;
