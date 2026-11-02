//! What's playing: the song or video Windows shows in its media pop-up (the one next to the
//! volume overlay), from Spotify, a browser or any other app that reports to Windows, and the
//! play/pause and skip buttons for it.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use gpui_kit::RenderImage;

/// How often the media session is read.
const POLL: Duration = Duration::from_secs(1);
/// After a button press, how long to give the app before reading again, so the next reading
/// shows the new state instead of the old one.
const CATCH_UP: Duration = Duration::from_millis(400);
/// Cover art is made at most this many pixels wide or tall (Spotify sends 300 x 300).
const ART_SIZE: u32 = 256;

#[derive(Clone, Copy)]
pub enum Command {
    PlayPause,
    Previous,
    Next,
}

#[derive(Clone)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The app playing it, e.g. "Spotify".
    pub app: String,
    pub playing: bool,
    /// Which buttons the app allows right now (e.g. no "previous" on the first track).
    pub can_play_pause: bool,
    pub can_previous: bool,
    pub can_next: bool,
    /// How far into the track it was at `position_at`, and how long it is. Zero length when
    /// the app doesn't say (some browsers).
    pub position: Duration,
    pub length: Duration,
    pub position_at: SystemTime,
    /// The cover art, as GPUI draws it.
    pub art: Option<Arc<RenderImage>>,
}

impl NowPlaying {
    /// How far through the track it is right now (0.0–1.0), or `None` when the length is unknown.
    /// Apps only report the position now and then, so the time since then is added while playing.
    pub fn progress(&self) -> Option<f32> {
        if self.length.is_zero() {
            return None;
        }
        let since = match self.playing {
            true => SystemTime::now().duration_since(self.position_at).unwrap_or_default(),
            false => Duration::ZERO,
        };
        Some(((self.position + since).as_secs_f32() / self.length.as_secs_f32()).clamp(0., 1.))
    }

    /// Flips playing / paused, keeping the position right.
    fn toggle(&mut self) {
        let now = SystemTime::now();
        if self.playing {
            self.position += now.duration_since(self.position_at).unwrap_or_default();
        }
        self.position_at = now;
        self.playing = !self.playing;
    }
}

/// The latest reading, written by the media thread.
static LATEST: Mutex<Option<NowPlaying>> = Mutex::new(None);
/// Button presses for the media thread; set once it's started.
static COMMANDS: OnceLock<Sender<Command>> = OnceLock::new();

/// Starts reading what's playing, once a second on a thread of its own. Only the first call
/// does anything, so every Now Playing widget can call it.
pub fn start() {
    COMMANDS.get_or_init(|| {
        let (sender, commands) = mpsc::channel();
        if let Err(error) = std::thread::Builder::new().name("media".into()).spawn(move || win32::run(commands)) {
            log::warn!("Could not start reading media: {error}");
        }
        sender
    });
}

/// What's playing as of the last reading; `None` when nothing is (or `start` wasn't called).
pub fn latest() -> Option<NowPlaying> {
    LATEST.lock().unwrap().clone()
}

/// Asks the app that's playing to play/pause or skip. Play/pause shows right away; a skip
/// shows once the app has moved to the next track.
pub fn send(command: Command) {
    let Some(commands) = COMMANDS.get() else { return };
    if commands.send(command).is_ok()
        && matches!(command, Command::PlayPause)
        && let Some(playing) = LATEST.lock().unwrap().as_mut()
    {
        playing.toggle();
    }
}

/// A readable name for the app id Windows gives, e.g. "Spotify.exe" → "Spotify".
fn app_name(id: &str) -> String {
    // Store apps look like "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic".
    let name = id.rsplit('!').next().unwrap_or(id);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let name = name.rsplit('.').next().unwrap_or(name);
    match name {
        "MSEdge" => "Edge".into(),
        "ZuneMusic" => "Media Player".into(),
        // Firefox reports this id instead of a name.
        "308046B0AF4A39CB" => "Firefox".into(),
        name => name.to_string(),
    }
}

#[cfg(windows)]
mod win32 {
    use std::sync::Arc;
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use gpui_kit::RenderImage;
    use image::Frame;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as SessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
    };
    use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};

    use super::{ART_SIZE, CATCH_UP, Command, LATEST, NowPlaying, POLL, app_name};

    /// Apps often set the cover a moment after the title, and Spotify first sends the previous
    /// track's: so the cover is fetched on the first readings of a track, and if there's still
    /// none, a few more times.
    const COVER_READS: u32 = 3;
    const COVER_TRIES: u32 = 10;
    /// Windows times count 100 ns steps from 1601; this is 1970 (the Unix epoch) in those steps.
    const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

    /// The cover of the track last read, so it's not fetched again every second.
    #[derive(Default)]
    struct Cover {
        track: String,
        art: Option<Arc<RenderImage>>,
        reads: u32,
    }

    pub fn run(commands: Receiver<Command>) {
        let mut manager: Option<SessionManager> = None;
        let mut cover = Cover::default();
        loop {
            if manager.is_none() {
                manager = SessionManager::RequestAsync()
                    .and_then(|request| request.join())
                    .inspect_err(|error| log::warn!("No media sessions: {error}"))
                    .ok();
            }
            // The session Windows' media pop-up shows; there's none when nothing plays.
            let session = || manager.as_ref().and_then(|manager| manager.GetCurrentSession().ok());
            let now_playing = session().and_then(|session| read(&session, &mut cover).ok());
            *LATEST.lock().unwrap() = now_playing;

            match commands.recv_timeout(POLL) {
                Ok(command) => {
                    if let Some(session) = session() {
                        let sent = match command {
                            Command::PlayPause => session.TryTogglePlayPauseAsync(),
                            Command::Previous => session.TrySkipPreviousAsync(),
                            Command::Next => session.TrySkipNextAsync(),
                        };
                        let _ = sent.and_then(|sent| sent.join());
                    }
                    std::thread::sleep(CATCH_UP);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn read(session: &Session, cover: &mut Cover) -> windows::core::Result<NowPlaying> {
        let properties = session.TryGetMediaPropertiesAsync()?.join()?;
        let playback = session.GetPlaybackInfo()?;
        let controls = playback.Controls()?;
        let timeline = session.GetTimelineProperties()?;

        let app = app_name(&session.SourceAppUserModelId()?.to_string());
        let title = properties.Title()?.to_string();
        let artist = properties.Artist()?.to_string();
        let album = properties.AlbumTitle()?.to_string();

        let track = format!("{app}\n{title}\n{artist}\n{album}");
        if cover.track != track {
            *cover = Cover { track, ..Default::default() };
        }
        if cover.reads < COVER_READS || (cover.art.is_none() && cover.reads < COVER_TRIES) {
            cover.reads += 1;
            if let Some(art) = properties.Thumbnail().ok().and_then(|thumbnail| cover_art(&thumbnail)) {
                cover.art = Some(art);
            }
        }

        let ticks = |ticks: i64| Duration::from_nanos(ticks.max(0) as u64 * 100);
        let start = timeline.StartTime()?.Duration;
        let updated = timeline.LastUpdatedTime()?.UniversalTime;        Ok(NowPlaying {
            title,
            artist,
            album,
            app,
            playing: playback.PlaybackStatus()? == PlaybackStatus::Playing,
            can_play_pause: controls.IsPlayPauseToggleEnabled()?,
            can_previous: controls.IsPreviousEnabled()?,
            can_next: controls.IsNextEnabled()?,
            position: ticks(timeline.Position()?.Duration - start),
            length: ticks(timeline.EndTime()?.Duration - start),
            // Some apps never set it; then count from now.
            position_at: match updated > UNIX_EPOCH_TICKS {
                true => UNIX_EPOCH + ticks(updated - UNIX_EPOCH_TICKS),
                false => SystemTime::now(),
            },
            art: cover.art.clone(),
        })
    }

    /// Reads and decodes the cover picture (usually a JPEG or PNG).
    fn cover_art(thumbnail: &IRandomAccessStreamReference) -> Option<Arc<RenderImage>> {
        let stream = thumbnail.OpenReadAsync().ok()?.join().ok()?;
        let size = u32::try_from(stream.Size().ok()?).ok().filter(|size| *size > 0)?;
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        reader.LoadAsync(size).ok()?.join().ok()?;
        let mut bytes = vec![0; size as usize];
        reader.ReadBytes(&mut bytes).ok()?;

        let mut image = image::load_from_memory(&bytes).ok()?.thumbnail(ART_SIZE, ART_SIZE).into_rgba8();
        // GPUI wants BGRA bytes; `RgbaImage` is only the container type here.
        for pixel in image.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        Some(Arc::new(RenderImage::new([Frame::new(image)])))
    }
}

/// Other platforms: nothing is ever playing.
#[cfg(not(windows))]
mod win32 {
    use std::sync::mpsc::Receiver;

    use super::Command;

    pub fn run(_: Receiver<Command>) {}
}

#[cfg(test)]
mod tests {
    use super::app_name;

    #[test]
    fn app_ids_become_names() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(app_name("Chrome"), "Chrome");
        assert_eq!(app_name("MSEdge"), "Edge");
        assert_eq!(app_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "Media Player");
        assert_eq!(app_name("308046B0AF4A39CB"), "Firefox");
    }
}
