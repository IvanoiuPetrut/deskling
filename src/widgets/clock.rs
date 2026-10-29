use super::{Content, WidgetType};
use crate::dashboard::WidgetConfig;
use crate::stats::SystemStats;

pub struct Clock;

impl WidgetType for Clock {
    fn name(&self) -> &'static str {
        "Clock"
    }

    fn content(&self, _: &WidgetConfig, _: &SystemStats) -> Content {
        let now = chrono::Local::now();
        Content {
            label: now.format("%a %d %b").to_string(),
            value: now.format("%H:%M").to_string(),
            bar: None,
            graph: None,
        }
    }
}
