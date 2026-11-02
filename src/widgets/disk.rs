use super::{Content, WidgetType, gigabytes};
use crate::dashboard::WidgetConfig;
use crate::stats::SystemStats;

pub struct Disk;

impl WidgetType for Disk {
    fn name(&self) -> &'static str {
        "Disk space"
    }

    fn content(&self, config: &WidgetConfig, stats: &SystemStats) -> Content {
        match stats.disk(&config.disk).or(stats.disks.first()) {
            Some(disk) => Content {
                label: format!("{} free of {}", disk.mount, gigabytes(disk.total)),
                value: gigabytes(disk.free),
                bar: Some(1. - disk.free as f32 / disk.total.max(1) as f32),
                graph: None,
                rows: Vec::new(),
                player: None,
            },
            None => Content {
                label: "Disk".into(),
                value: "–".into(),
                bar: None,
                graph: None,
                rows: Vec::new(),
                player: None,
            },
        }
    }
}
