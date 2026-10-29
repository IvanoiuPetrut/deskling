use chrono::{Local, NaiveDateTime, TimeDelta, Utc};

use super::{Content, WidgetType};
use crate::dashboard::WidgetConfig;
use crate::stats::SystemStats;

pub struct Clock;

impl WidgetType for Clock {
    fn name(&self) -> &'static str {
        "Clock"
    }

    fn content(&self, config: &WidgetConfig, _: &SystemStats) -> Content {
        let options = config.clock;

        // Another time zone = UTC plus a number of hours (can be half hours, like India's +5.5).
        let now: NaiveDateTime = match options.utc_offset {
            Some(hours) => Utc::now().naive_utc() + TimeDelta::minutes((hours * 60.).round() as i64),
            None => Local::now().naive_local(),
        };

        let time_pattern = match (options.hour24, options.seconds) {
            (true, false) => "%H:%M",
            (true, true) => "%H:%M:%S",
            (false, false) => "%-I:%M %p",
            (false, true) => "%-I:%M:%S %p",
        };
        let mut label = now.format(options.date.pattern()).to_string();
        if let Some(hours) = options.utc_offset {
            label = format!("{label} · {}", utc_name(hours));
        }

        Content { label, value: now.format(time_pattern).to_string(), bar: None, graph: None }
    }
}

/// E.g. `UTC+9`, `UTC-3:30`, `UTC`.
pub fn utc_name(hours: f32) -> String {
    let minutes = (hours * 60.).round() as i32;
    let (sign, minutes) = if minutes < 0 { ('-', -minutes) } else { ('+', minutes) };
    match (minutes / 60, minutes % 60) {
        (0, 0) => "UTC".into(),
        (h, 0) => format!("UTC{sign}{h}"),
        (h, m) => format!("UTC{sign}{h}:{m:02}"),
    }
}
