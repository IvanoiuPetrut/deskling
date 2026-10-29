//! System numbers for the widgets: CPU, RAM, disks and (when Windows exposes it) temperature.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gpui_kit::*;
use sysinfo::{Components, Disks, Networks, System};

/// How often the numbers are read.
const TICK: Duration = Duration::from_secs(1);
/// The temperature (a slow WMI query) and the list of drives are only re-read every this many ticks.
const SLOW_EVERY: u32 = 5;
/// How many readings the history graphs keep (one per tick, so 60 = the last minute).
pub const HISTORY_LEN: usize = 60;

/// The last `HISTORY_LEN` readings, oldest first.
#[derive(Clone, Default)]
pub struct History(VecDeque<f32>);

impl History {
    fn push(&mut self, value: f32) {
        if self.0.len() == HISTORY_LEN {
            self.0.pop_front();
        }
        self.0.push_back(value);
    }

    pub fn to_vec(&self) -> Vec<f32> {
        self.0.iter().copied().collect()
    }
}

#[derive(Clone)]
pub struct DiskStats {
    /// Where the drive is mounted, e.g. `C:\`.
    pub mount: String,
    /// The drive's label, e.g. `Windows` (can be empty).
    pub name: String,
    pub free: u64,
    pub total: u64,
}

#[derive(Default)]
pub struct SystemStats {
    /// 0–100, all cores together.
    pub cpu_usage: f32,
    /// Degrees Celsius, `None` when Windows doesn't report one (common without admin rights).
    pub cpu_temperature: Option<f32>,
    pub ram_used: u64,
    pub ram_total: u64,
    pub disks: Vec<DiskStats>,
    /// Bytes per second, all network adapters together.
    pub net_down: f32,
    pub net_up: f32,
    /// CPU and RAM in %, network in bytes per second.
    pub cpu_history: History,
    pub ram_history: History,
    pub net_down_history: History,
}

impl SystemStats {
    pub fn disk(&self, mount: &str) -> Option<&DiskStats> {
        self.disks.iter().find(|disk| disk.mount == mount)
    }
}

/// The sysinfo objects, kept between reads (CPU usage is measured between two reads).
struct Sampler {
    system: System,
    disks: Disks,
    networks: Networks,
    last_read: Instant,
}

impl Sampler {
    fn read(&mut self, slow: bool, into: &mut SystemStats) {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        if slow {
            self.disks.refresh_list();
        } else {
            self.disks.refresh();
        }

        // Networks only report bytes moved since the last refresh, so divide by the time since then.
        self.networks.refresh();
        let seconds = self.last_read.elapsed().as_secs_f32().max(0.001);
        self.last_read = Instant::now();
        let (down, up) = self
            .networks
            .iter()
            .fold((0, 0), |(down, up), (_, data)| (down + data.received(), up + data.transmitted()));
        into.net_down = down as f32 / seconds;
        into.net_up = up as f32 / seconds;

        into.cpu_usage = self.system.global_cpu_usage();
        into.ram_used = self.system.used_memory();
        into.ram_total = self.system.total_memory();
        into.disks = self
            .disks
            .iter()
            .map(|disk| DiskStats {
                mount: disk.mount_point().to_string_lossy().into_owned(),
                name: disk.name().to_string_lossy().into_owned(),
                free: disk.available_space(),
                total: disk.total_space(),
            })
            .collect();

        if slow {
            // Windows: WMI's MSAcpi_ThermalZoneTemperature. Usually needs admin rights, and it's a
            // motherboard "thermal zone" rather than the exact CPU core temperature.
            into.cpu_temperature = Components::new_with_refreshed_list()
                .iter()
                .map(|component| component.temperature())
                .filter(|celsius| celsius.is_finite() && *celsius > 0.)
                .reduce(f32::max);
        }
    }
}

/// Starts reading the numbers once a second. Widgets observe the returned entity to redraw.
pub fn start(cx: &mut App) -> Entity<SystemStats> {
    let stats = cx.new(|_| SystemStats::default());
    let weak = stats.downgrade();

    cx.spawn(async move |cx| {
        let mut sampler = Sampler {
            system: System::new(),
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            last_read: Instant::now(),
        };
        let mut fresh = SystemStats::default();
        let mut tick = 0u32;
        loop {
            let slow = tick % SLOW_EVERY == 0;
            // Reading can take a while (the WMI query especially), so do it off the UI thread.
            // The sampler goes into the background task and comes back out with the result.
            (sampler, fresh) = cx
                .background_executor()
                .spawn(async move {
                    sampler.read(slow, &mut fresh);
                    (sampler, fresh)
                })
                .await;

            let updated = weak.update(cx, |stats, cx| {
                stats.cpu_usage = fresh.cpu_usage;
                stats.ram_used = fresh.ram_used;
                stats.ram_total = fresh.ram_total;
                stats.disks = fresh.disks.clone();
                stats.net_down = fresh.net_down;
                stats.net_up = fresh.net_up;
                stats.cpu_history.push(fresh.cpu_usage);
                stats.ram_history.push(fresh.ram_used as f32 / fresh.ram_total.max(1) as f32 * 100.);
                stats.net_down_history.push(fresh.net_down);
                if slow {
                    stats.cpu_temperature = fresh.cpu_temperature;
                }
                cx.notify();
            });
            if updated.is_err() {
                break; // app is shutting down
            }

            cx.background_executor().timer(TICK).await;
            tick = tick.wrapping_add(1);
        }
    })
    .detach();

    stats
}
