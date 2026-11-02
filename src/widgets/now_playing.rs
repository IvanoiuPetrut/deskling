use super::{Content, Player, WidgetType};
use crate::dashboard::WidgetConfig;
use crate::stats::SystemStats;

pub struct NowPlaying;

impl WidgetType for NowPlaying {
    fn name(&self) -> &'static str {
        "Now playing"
    }

    fn content(&self, config: &WidgetConfig, stats: &SystemStats) -> Content {
        let Some(media) = &stats.media else {
            return Content {
                label: "Nothing playing".into(),
                value: "–".into(),
                bar: None,
                graph: None,
                rows: Vec::new(),
                player: None,
            };
        };
        // Videos and podcasts often have no artist; then the album or the app is shown instead.
        let label = [&media.artist, &media.album, &media.app].into_iter().find(|text| !text.trim().is_empty());
        let value = if media.title.trim().is_empty() { &media.app } else { &media.title };
        Content {
            label: label.cloned().unwrap_or_default(),
            value: value.clone(),
            bar: media.progress(),
            graph: None,
            rows: Vec::new(),
            player: Some(Player {
                art: config.media.show_art.then(|| media.art.clone()).flatten(),
                show_art: config.media.show_art,
                show_controls: config.media.show_controls,
                playing: media.playing,
                can_play_pause: media.can_play_pause,
                can_previous: media.can_previous,
                can_next: media.can_next,
            }),
        }
    }
}
