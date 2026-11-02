//! The weather for the Weather widget, from Open-Meteo (open-meteo.com: free, no API key): the
//! temperature and sky right now, and the coming days' lows and highs. Also the city search.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig};

/// How often each place's weather is fetched (Open-Meteo updates its "current" every 15 minutes).
const REFRESH: Duration = Duration::from_secs(15 * 60);
/// After a failed fetch (offline, say), how soon to try again.
const RETRY: Duration = Duration::from_secs(60);
/// Days in the forecast rows: today and the two after.
pub const DAYS: usize = 3;

/// A place picked in the city search.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    /// Region and country, e.g. "Bucharest, Romania" (can be empty).
    pub region: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    /// "Oslo, Norway": the name and, if there is one, the region and country.
    pub fn full_name(&self) -> String {
        match self.region.is_empty() {
            true => self.name.clone(),
            false => format!("{}, {}", self.name, self.region),
        }
    }

    fn at(&self) -> (f64, f64) {
        (self.latitude, self.longitude)
    }
}

/// The weather at one place, as far as it's known.
#[derive(Clone)]
pub struct Report {
    pub latitude: f64,
    pub longitude: f64,
    pub now: Option<Now>,
    pub days: Vec<Day>,
    /// The last fetch failed; whatever was fetched before is kept.
    pub failed: bool,
}

impl Report {
    pub fn is_for(&self, place: &Place) -> bool {
        (self.latitude, self.longitude) == place.at()
    }
}

#[derive(Clone)]
pub struct Now {
    /// °C.
    pub temperature: f32,
    /// WMO weather code, see [`sky`].
    pub code: u8,
    pub is_day: bool,
}

#[derive(Clone)]
pub struct Day {
    pub date: NaiveDate,
    pub code: u8,
    /// °C.
    pub min: f32,
    pub max: f32,
}

/// What the sky looks like, for picking an icon.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Sky {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Thunder,
}

/// The sky for a WMO weather code, see https://open-meteo.com/en/docs ("WMO Weather interpretation codes").
pub fn sky(code: u8) -> Sky {
    match code {
        0 | 1 => Sky::Clear,
        2 => Sky::PartlyCloudy,
        3 => Sky::Cloudy,
        45 | 48 => Sky::Fog,
        51..=57 => Sky::Drizzle,
        61..=67 | 80..=82 => Sky::Rain,
        71..=77 | 85 | 86 => Sky::Snow,
        95..=99 => Sky::Thunder,
        _ => Sky::Cloudy,
    }
}

/// A short description of a WMO weather code, e.g. "Light rain".
pub fn describe(code: u8) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 => "Light snow",
        73 => "Snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80..=82 => "Showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm, hail",
        _ => "Unknown",
    }
}

/// The latest reports, written by the weather thread.
static REPORTS: Mutex<Vec<Report>> = Mutex::new(Vec::new());
/// Places for the weather thread to start fetching; set once it's started.
static WATCH: OnceLock<Sender<(f64, f64)>> = OnceLock::new();

/// Keeps `place`'s weather fetched from now on (every 15 minutes, on a thread of its own). Calling
/// it again for the same place does nothing, so a widget can call it whenever its settings change.
pub fn watch(place: &Place) {
    let sender = WATCH.get_or_init(|| {
        let (sender, places) = mpsc::channel();
        if let Err(error) = std::thread::Builder::new().name("weather".into()).spawn(move || run(places)) {
            log::warn!("Could not start fetching the weather: {error}");
        }
        sender
    });
    let _ = sender.send(place.at());
}

/// Every watched place's weather as of the last fetch.
pub fn latest() -> Vec<Report> {
    REPORTS.lock().unwrap().clone()
}

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            // Trust what Windows trusts (including a company's own certificates).
            .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
            .build()
            .new_agent()
    })
}

fn run(new_places: Receiver<(f64, f64)>) {
    // Each place and when to fetch it next.
    let mut places: Vec<((f64, f64), Instant)> = Vec::new();
    loop {
        for (place, next) in &mut places {
            if *next > Instant::now() {
                continue;
            }
            let fetched = fetch(*place).inspect_err(|error| log::warn!("Could not fetch the weather: {error}"));
            *next = Instant::now() + if fetched.is_ok() { REFRESH } else { RETRY };
            store(*place, fetched.ok());
        }
        let wait = places.iter().map(|(_, next)| next.saturating_duration_since(Instant::now())).min();
        match new_places.recv_timeout(wait.unwrap_or(REFRESH)) {
            Ok(place) if !places.iter().any(|(known, _)| *known == place) => places.push((place, Instant::now())),
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Saves a fetch's result; a failed one (`None`) keeps the old numbers and marks them as failed.
fn store((latitude, longitude): (f64, f64), fetched: Option<Report>) {
    let mut reports = REPORTS.lock().unwrap();
    let known = reports.iter_mut().find(|report| (report.latitude, report.longitude) == (latitude, longitude));
    match (known, fetched) {
        (Some(known), Some(report)) => *known = report,
        (Some(known), None) => known.failed = true,
        (None, report) => reports.push(report.unwrap_or(Report {
            latitude,
            longitude,
            now: None,
            days: Vec::new(),
            failed: true,
        })),
    }
}

// What Open-Meteo answers, with only the fields asked for.
#[derive(Deserialize)]
struct ForecastJson {
    current: NowJson,
    daily: DailyJson,
}

#[derive(Deserialize)]
struct NowJson {
    temperature_2m: f32,
    weather_code: u8,
    is_day: u8,
}

#[derive(Deserialize)]
struct DailyJson {
    time: Vec<String>,
    weather_code: Vec<Option<u8>>,
    temperature_2m_min: Vec<Option<f32>>,
    temperature_2m_max: Vec<Option<f32>>,
}

fn fetch((latitude, longitude): (f64, f64)) -> Result<Report, ureq::Error> {
    let forecast: ForecastJson = agent()
        .get("https://api.open-meteo.com/v1/forecast")
        .query("latitude", latitude.to_string())
        .query("longitude", longitude.to_string())
        .query("current", "temperature_2m,weather_code,is_day")
        .query("daily", "weather_code,temperature_2m_min,temperature_2m_max")
        // Days start at midnight where the place is, not where this PC is.
        .query("timezone", "auto")
        .query("forecast_days", DAYS.to_string())
        .call()?
        .body_mut()
        .read_json()?;

    let daily = forecast.daily;
    let days = (0..daily.time.len())
        .filter_map(|i| {
            Some(Day {
                date: NaiveDate::parse_from_str(&daily.time[i], "%Y-%m-%d").ok()?,
                code: (*daily.weather_code.get(i)?)?,
                min: (*daily.temperature_2m_min.get(i)?)?,
                max: (*daily.temperature_2m_max.get(i)?)?,
            })
        })
        .collect();
    Ok(Report {
        latitude,
        longitude,
        now: Some(Now {
            temperature: forecast.current.temperature_2m,
            code: forecast.current.weather_code,
            is_day: forecast.current.is_day != 0,
        }),
        days,
        failed: false,
    })
}

#[derive(Deserialize)]
struct SearchJson {
    // Left out when nothing matches.
    #[serde(default)]
    results: Vec<FoundJson>,
}

#[derive(Deserialize)]
struct FoundJson {
    name: String,
    latitude: f64,
    longitude: f64,
    admin1: Option<String>,
    country: Option<String>,
}

/// Places whose name starts with `name`, biggest first. Blocks while it asks Open-Meteo, so
/// call it off the UI thread.
pub fn search(name: &str) -> Result<Vec<Place>, ureq::Error> {
    let found: SearchJson = agent()
        .get("https://geocoding-api.open-meteo.com/v1/search")
        .query("name", name.trim())
        .query("count", "6")
        .query("language", "en")
        .call()?
        .body_mut()
        .read_json()?;
    Ok(found
        .results
        .into_iter()
        .map(|found| {
            // "Bucharest, Bucharest, Romania" says the city twice; keep the region only when it adds something.
            let region = found.admin1.filter(|region| *region != found.name);
            let region = region.into_iter().chain(found.country).collect::<Vec<_>>().join(", ");
            Place { name: found.name, region, latitude: found.latitude, longitude: found.longitude }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{Sky, describe, sky};

    #[test]
    fn weather_codes_have_a_sky_and_a_name() {
        assert_eq!(sky(0), Sky::Clear);
        assert_eq!(sky(2), Sky::PartlyCloudy);
        assert_eq!(sky(63), Sky::Rain);
        assert_eq!(sky(81), Sky::Rain);
        assert_eq!(sky(73), Sky::Snow);
        assert_eq!(sky(96), Sky::Thunder);
        assert_eq!(describe(61), "Light rain");
        assert_eq!(describe(200), "Unknown");
    }
}
