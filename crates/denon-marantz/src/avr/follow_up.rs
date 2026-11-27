//! Reads the values that a command over HTTP changed. A command over HTTP
//! gets no answer, so the state does not change by itself.

use std::sync::Arc;
#[cfg(feature = "telnet")]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::Bytes;
use denon_marantz_proto::appcommand::{self, Definition, Endpoint, SettingsPart};
use denon_marantz_proto::telnet::QueryGroup;
use denon_marantz_proto::{Brand, PlayingKind, State, TUNER_STATUS_PATH, TunerPage, Zone};
#[cfg(feature = "telnet")]
use tokio::sync::watch;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{Instant, timeout, timeout_at};
use tracing::Instrument;

use super::{
    Info, Missing, UpdateMethod, definitions_body, read_appcommand, read_settings_request,
    read_status_pages, settings_request,
};
use crate::channels::Channels;
use crate::command::{ReadBits, Reads};
use crate::error::Error;
use crate::http::HttpClient;
#[cfg(feature = "telnet")]
use crate::telnet::{Telnet, TelnetStatus};

/// When the values that a command over HTTP changed are read.
///
/// The read waits until no command came for `quiet`, so a key that is held
/// down causes one read, not one for each command. It does not wait longer
/// than `longest_wait` after the first command, so the state also changes
/// while commands keep coming. The read has only the values that the command
/// changed: for example the volume and mute for a volume command.
///
/// The receiver shows a new power state, source or sound mode late in its
/// answers, up to a few seconds. So these are read again every `retry_every`,
/// until a read shows a change, but not more than `retries` times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FollowUpRead {
    /// Time without a command before the read.
    pub quiet: Duration,
    /// Longest time between the first command and the read.
    pub longest_wait: Duration,
    /// Time between the reads of the power state, the source and the sound
    /// mode, until one shows a change.
    pub retry_every: Duration,
    /// The most reads that follow the first read of these values.
    pub retries: u8,
}

impl Default for FollowUpRead {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(500),
            longest_wait: Duration::from_secs(2),
            retry_every: Duration::from_millis(1500),
            retries: 3,
        }
    }
}

impl FollowUpRead {
    pub(super) const fn check(&self) -> Result<(), Error> {
        if self.quiet.is_zero() || self.longest_wait.is_zero() || self.retry_every.is_zero() {
            return Err(Error::InvalidConfig(
                "quiet, longest_wait and retry_every must be above 0",
            ));
        }
        Ok(())
    }
}

/// The queries for one kind of value.
const POWER: [Definition; 1] = [appcommand::GET_ALL_ZONE_POWER_STATUS];
const VOLUME: [Definition; 2] = [
    appcommand::GET_ALL_ZONE_VOLUME,
    appcommand::GET_ALL_ZONE_MUTE_STATUS,
];
const SOURCE: [Definition; 1] = [appcommand::GET_ALL_ZONE_SOURCE];
const SOUND_MODE: [Definition; 1] = [appcommand::GET_SURROUND_MODE_STATUS];
const DEVICE: [Definition; 3] = [
    appcommand::GET_AUTO_STANDBY,
    appcommand::GET_DIMMER,
    appcommand::GET_ECO,
];
const TONE: [Definition; 1] = [appcommand::GET_TONE_CONTROL];
const AUDYSSEY: [Definition; 1] = [appcommand::GET_AUDYSSEY];
const HDMI_OUTPUT: [Definition; 1] = [appcommand::GET_OUTPUT_SETTINGS];

/// What the worker needs to read.
struct Reader {
    http: HttpClient,
    channels: Channels,
    zones: Box<[Zone]>,
    method: UpdateMethod,
    /// The receiver gets `GetAudyssey`, see `appcommand0300_commands`.
    audyssey: bool,
    /// The receiver gets `GetOutputSettings`, see `appcommand0300_commands`.
    hdmi_output: bool,
    /// The settings requests, with their number of commands, for
    /// [`Reads::Settings`] and [`Reads::SlowSettings`].
    settings: [(Reads, Option<(Bytes, usize)>); 2],
    missing: Arc<Missing>,
    #[cfg(feature = "telnet")]
    telnet: watch::Receiver<TelnetStatus>,
    brand: Brand,
    /// The groups that telnet asks for after a connect. None without telnet.
    telnet_groups: &'static [QueryGroup],
    /// The flag of `Telnet::forget_values`.
    #[cfg(feature = "telnet")]
    forget: Arc<AtomicBool>,
}

impl Reader {
    /// Telnet is open, so the receiver reports the changes itself.
    // Without telnet it is always false.
    #[cfg_attr(
        not(feature = "telnet"),
        allow(clippy::missing_const_for_fn, clippy::unused_self)
    )]
    fn telnet_open(&self) -> bool {
        #[cfg(feature = "telnet")]
        return self.telnet.borrow().is_connected();
        #[cfg(not(feature = "telnet"))]
        false
    }

    /// After a read set values while telnet was open, the next telnet event
    /// with the old value is a real change.
    #[cfg_attr(
        not(feature = "telnet"),
        allow(clippy::missing_const_for_fn, clippy::unused_self)
    )]
    fn forget_telnet_values(&self) {
        #[cfg(feature = "telnet")]
        self.forget.store(true, Ordering::Relaxed);
    }
}

/// A request for one kind of value.
struct Request {
    reads: Reads,
    endpoint: Endpoint,
    commands: &'static [Definition],
    body: Bytes,
}

impl Request {
    fn new(reads: Reads, commands: &'static [Definition]) -> Result<Self, Error> {
        Self::on(Endpoint::AppCommand, reads, commands)
    }

    fn on(
        endpoint: Endpoint,
        reads: Reads,
        commands: &'static [Definition],
    ) -> Result<Self, Error> {
        Ok(Self {
            reads,
            endpoint,
            commands,
            body: definitions_body(commands)?,
        })
    }

    /// Whether to send the request: the receiver answers it, as far as is
    /// known, and the answer does not undo what telnet reported.
    fn useful(&self, reader: &Reader) -> bool {
        match self.reads {
            Reads::Tone => !reader.missing.tone_control.load(Ordering::Relaxed),
            // A receiver can answer blank Audyssey values, which would clear
            // the values that telnet reported.
            Reads::Audyssey => {
                !reader.missing.appcommand0300.load(Ordering::Relaxed) && !reader.telnet_open()
            }
            Reads::HdmiOutput => !reader.missing.appcommand0300.load(Ordering::Relaxed),
            _ => true,
        }
    }
}

/// Collects what commands over HTTP changed.
#[derive(Debug)]
pub(super) struct FollowUp {
    wanted: mpsc::UnboundedSender<ReadBits>,
}

impl FollowUp {
    /// Starts the task, if there is a setting. It ends when this value is
    /// dropped.
    pub(super) fn start(
        config: Option<FollowUpRead>,
        http: &HttpClient,
        channels: &Channels,
        info: &Info,
        missing: &Arc<Missing>,
        appcommand0300: &[Definition],
        #[cfg(feature = "telnet")] telnet: &Telnet,
    ) -> Result<Option<Self>, Error> {
        let Some(config) = config else {
            return Ok(None);
        };
        let reader = Reader {
            // The read is for the commands, so it does not wait for the pause
            // after them. It runs between the commands of a key that is held.
            http: http.without_pause(),
            channels: channels.clone(),
            zones: info.zones.clone().into_boxed_slice(),
            method: info.update_method,
            audyssey: appcommand0300.contains(&appcommand::GET_AUDYSSEY),
            hdmi_output: appcommand0300.contains(&appcommand::GET_OUTPUT_SETTINGS),
            settings: [
                (Reads::Settings, settings_request(info, SettingsPart::Fast)),
                (
                    Reads::SlowSettings,
                    settings_request(info, SettingsPart::Slow),
                ),
            ],
            missing: Arc::clone(missing),
            #[cfg(feature = "telnet")]
            telnet: telnet.watch_status(),
            brand: info.brand,
            #[cfg(feature = "telnet")]
            telnet_groups: telnet.query_groups(),
            #[cfg(not(feature = "telnet"))]
            telnet_groups: &[],
            #[cfg(feature = "telnet")]
            forget: telnet.forget_flag(),
        };
        Self::spawn(config, reader).map(Some)
    }

    fn spawn(config: FollowUpRead, reader: Reader) -> Result<Self, Error> {
        config.check()?;
        let requests = match reader.method {
            UpdateMethod::AppCommand => {
                let mut requests = vec![
                    Request::new(Reads::Power, &POWER)?,
                    Request::new(Reads::Volume, &VOLUME)?,
                    Request::new(Reads::Source, &SOURCE)?,
                    Request::new(Reads::SoundMode, &SOUND_MODE)?,
                    Request::new(Reads::Device, &DEVICE)?,
                    Request::new(Reads::Tone, &TONE)?,
                ];
                // A receiver can fail a request with a query that it does not know.
                if reader.audyssey {
                    requests.push(Request::on(
                        Endpoint::AppCommand0300,
                        Reads::Audyssey,
                        &AUDYSSEY,
                    )?);
                }
                if reader.hdmi_output {
                    requests.push(Request::on(
                        Endpoint::AppCommand0300,
                        Reads::HdmiOutput,
                        &HDMI_OUTPUT,
                    )?);
                }
                requests
            }
            UpdateMethod::StatusPages => Vec::new(),
        };
        let (wanted, queue) = mpsc::unbounded_channel();
        let span = reader.http.span().clone();
        tokio::spawn(run(config, reader, requests, queue).instrument(span));
        Ok(Self { wanted })
    }

    /// Says that a command changed this kind of value.
    pub(super) fn note(&self, reads: Reads) {
        // There is an error if the task ended. Then nothing is read.
        let _ = self.wanted.send(reads.bit());
    }
}

/// The values that a later read still looks for.
struct Retry {
    at: Instant,
    wanted: ReadBits,
    /// Read again with `wanted`, but not looked for: the fast settings, if a
    /// sound mode or source command of the main zone asked for them. The
    /// receiver shows the settings of a new sound mode late too, and it keeps
    /// a sound mode for each source. A settings read never counts as seen.
    also: ReadBits,
    left: u8,
}

async fn run(
    config: FollowUpRead,
    reader: Reader,
    requests: Vec<Request>,
    mut queue: mpsc::UnboundedReceiver<ReadBits>,
) {
    let mut retry: Option<Retry> = None;
    loop {
        let first = if let Some(next) = retry.as_ref() {
            if let Ok(bits) = timeout_at(next.at, queue.recv()).await {
                bits
            } else {
                let (wanted, also, left) = (next.wanted, next.also, next.left - 1);
                tracing::debug!(wanted, "read again over HTTP");
                let unseen = read_until_seen(&reader, &requests, wanted | also).await & wanted;
                retry = (unseen != 0 && left > 0).then(|| Retry {
                    at: Instant::now() + config.retry_every,
                    wanted: unseen,
                    also,
                    left,
                });
                continue;
            }
        } else {
            queue.recv().await
        };
        let Some(first) = first else {
            return;
        };
        let mut wanted = first;
        let started = Instant::now();
        loop {
            let wait = config
                .quiet
                .min(config.longest_wait.saturating_sub(started.elapsed()));
            if wait.is_zero() {
                break;
            }
            match timeout(wait, queue.recv()).await {
                Ok(Some(bits)) => wanted |= bits,
                Ok(None) | Err(_) => break,
            }
        }
        tracing::debug!(wanted, "read after commands over HTTP");
        let unseen = read_until_seen(&reader, &requests, wanted).await;
        let (old, old_also) = retry
            .as_ref()
            .map_or((0, 0), |next| (next.wanted, next.also));
        let pending = pending_after(old, wanted, unseen);
        retry = (pending != 0 && config.retries > 0).then(|| Retry {
            at: Instant::now() + config.retry_every,
            wanted: pending,
            also: old_also | late_settings(wanted),
            left: config.retries,
        });
    }
}

/// The fast settings in `wanted`, if it also has the sound mode or the source.
/// A settings command alone does not make them read again.
const fn late_settings(wanted: ReadBits) -> ReadBits {
    if wanted & (Reads::SoundMode.bit() | Reads::Source.bit()) == 0 {
        0
    } else {
        wanted & Reads::Settings.bit()
    }
}

/// The values that a later read still looks for. `old` was looked for before.
/// `wanted` was read now, and `unseen` is the part of it that did not change.
/// A value that this read showed is not looked for again.
const fn pending_after(old: ReadBits, wanted: ReadBits, unseen: ReadBits) -> ReadBits {
    let seen = wanted & !unseen;
    (old & !seen) | (unseen & Reads::SLOW)
}

/// Reads, and returns the part of `wanted` that the read did not change. A
/// read of values that are not late counts as seen.
async fn read_until_seen(reader: &Reader, requests: &[Request], wanted: ReadBits) -> ReadBits {
    let mut changes = reader.channels.subscribe_changes();
    if let Err(error) = read(reader, requests, wanted).await {
        tracing::debug!(%error, "the read after a command failed");
        return wanted;
    }
    let mut seen = 0;
    loop {
        match changes.try_recv() {
            Ok(change) => seen |= Reads::of_setting(change.setting),
            Err(broadcast::error::TryRecvError::Lagged(_)) => seen = ReadBits::MAX,
            Err(_) => break,
        }
    }
    wanted & !seen
}

async fn read(reader: &Reader, requests: &[Request], wanted: ReadBits) -> Result<(), Error> {
    let mut result = Ok(());
    if reader.method == UpdateMethod::StatusPages {
        let not_on_pages = Reads::Tone.bit()
            | Reads::Audyssey.bit()
            | Reads::Tuner.bit()
            | Reads::HdmiOutput.bit()
            | Reads::Settings.bit()
            | Reads::SlowSettings.bit();
        if wanted & !not_on_pages != 0 {
            result = read_status_pages(&reader.http, &reader.channels, &reader.zones).await;
        }
    } else {
        for request in requests
            .iter()
            .filter(|r| wanted & r.reads.bit() != 0 && r.useful(reader))
        {
            let read = read_request(reader, request).await;
            if let Err(error) = &read {
                note_missing(reader, requests, request.reads, error).await;
            }
            result = result.and(read);
        }
        // Telnet reports these settings itself. A failed read of them is
        // only logged, so that it does not make the other values count as
        // not seen.
        for (reads, request) in &reader.settings {
            let Some((body, count)) = request else {
                continue;
            };
            if wanted & reads.bit() == 0 || reader.telnet_open() {
                continue;
            }
            let read = read_settings_request(
                &reader.http,
                &reader.channels,
                (body.clone(), *count),
                reader.brand,
                reader.telnet_groups,
                || reader.telnet_open(),
                || reader.forget_telnet_values(),
            )
            .await;
            if let Err(error) = read {
                tracing::debug!(%error, "the settings after a command were not read");
            }
        }
    }
    // A receiver without the page must not make the other values count as
    // not seen.
    if wanted & Reads::Tuner.bit() != 0
        && let Err(error) = read_tuner_page(reader).await
    {
        tracing::debug!(%error, "the tuner page after a command was not read");
    }
    result
}

async fn read_request(reader: &Reader, request: &Request) -> Result<(), Error> {
    read_appcommand(
        &reader.http,
        &reader.channels,
        &reader.zones,
        request.endpoint,
        request.commands,
        request.body.clone(),
    )
    .await
}

/// Notes a read that the receiver does not answer, so that it is not sent
/// again. `requests` are all requests of the reader.
async fn note_missing(reader: &Reader, requests: &[Request], reads: Reads, error: &Error) {
    let missing = &reader.missing;
    match reads {
        // Only a missing file means that the receiver does not have it.
        Reads::Audyssey | Reads::HdmiOutput
            if matches!(error, Error::Status { status: 404, .. }) =>
        {
            missing.appcommand0300.store(true, Ordering::Relaxed);
        }
        // As in the update: tone control counts as missing only if the
        // receiver answers a request without it. The power request is the
        // smallest one to the same file.
        Reads::Tone if missing.may_lack_tone_control(error) => {
            let Some(power) = requests.iter().find(|r| r.reads == Reads::Power) else {
                return;
            };
            if read_request(reader, power).await.is_ok() {
                tracing::debug!("tone control is left out of the reads after commands");
                missing.tone_control.store(true, Ordering::Relaxed);
            }
        }
        _ => {}
    }
}

/// Reads the band and the frequency, if the main zone plays the tuner. The
/// page has no values for another source.
async fn read_tuner_page(reader: &Reader) -> Result<(), Error> {
    if reader.channels.read(State::main_playing_kind) != Some(PlayingKind::Tuner) {
        return Ok(());
    }
    let page = reader
        .http
        .get_parsed(TUNER_STATUS_PATH, TunerPage::parse)
        .await?;
    reader
        .channels
        .modify(|state, notify| notify(state.apply_tuner_page(page)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_that_a_read_showed_is_not_looked_for_again() {
        let (power, source, volume) =
            (Reads::Power.bit(), Reads::Source.bit(), Reads::Volume.bit());
        // The source was looked for. A new read of the source shows a change.
        assert_eq!(pending_after(source, source, 0), 0);
        // The new read shows nothing: it is still looked for.
        assert_eq!(pending_after(source, source, source), source);
        // The old retry is for the source, and the new read is for power.
        assert_eq!(pending_after(source, power, power), source | power);
        assert_eq!(pending_after(source, power, 0), source);
        // A value that is not late is never looked for again.
        assert_eq!(pending_after(0, volume, volume), 0);
        assert_eq!(pending_after(0, volume | source, volume | source), source);
    }

    #[test]
    fn settings_are_read_again_only_with_a_sound_mode_or_source() {
        let settings = Reads::Settings.bit();
        let sound_mode = Reads::SoundMode.bit();
        assert_eq!(late_settings(sound_mode | settings), settings);
        assert_eq!(late_settings(Reads::Source.bit() | settings), settings);
        assert_eq!(late_settings(Reads::Power.bit() | settings), 0);
        assert_eq!(late_settings(sound_mode), 0);
    }
}
