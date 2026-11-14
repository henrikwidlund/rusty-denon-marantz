//! Reads the values that a command over HTTP changed. A command over HTTP
//! gets no answer, so the state does not change by itself.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use bytes::Bytes;
use denon_marantz_proto::appcommand::{self, Definition, Endpoint};
use denon_marantz_proto::{PlayingKind, TUNER_STATUS_PATH, TunerPage, Zone};
#[cfg(feature = "telnet")]
use tokio::sync::watch;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{Instant, timeout, timeout_at};

use super::{
    Info, Missing, UpdateMethod, playing_kind, read_appcommand, read_status_pages, request_body,
};
use crate::channels::Channels;
use crate::command::Reads;
use crate::error::Error;
use crate::http::HttpClient;
#[cfg(feature = "telnet")]
use crate::telnet::TelnetStatus;

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

/// What the worker needs to read.
struct Reader {
    http: HttpClient,
    channels: Channels,
    zones: Box<[Zone]>,
    method: UpdateMethod,
    missing: Arc<Missing>,
    #[cfg(feature = "telnet")]
    telnet: watch::Receiver<TelnetStatus>,
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
        return *self.telnet.borrow() == TelnetStatus::Connected;
        #[cfg(not(feature = "telnet"))]
        false
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
        let cmds: Vec<_> = commands.iter().map(|definition| definition.cmd).collect();
        Ok(Self {
            reads,
            endpoint,
            commands,
            body: request_body(&cmds)?,
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
            _ => true,
        }
    }
}

/// Collects what commands over HTTP changed.
#[derive(Debug)]
pub(super) struct FollowUp {
    wanted: mpsc::UnboundedSender<u8>,
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
        #[cfg(feature = "telnet")] telnet: watch::Receiver<TelnetStatus>,
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
            missing: Arc::clone(missing),
            #[cfg(feature = "telnet")]
            telnet,
        };
        Self::spawn(config, reader).map(Some)
    }

    fn spawn(config: FollowUpRead, reader: Reader) -> Result<Self, Error> {
        config.check()?;
        let requests = match reader.method {
            UpdateMethod::AppCommand => vec![
                Request::new(Reads::Power, &POWER)?,
                Request::new(Reads::Volume, &VOLUME)?,
                Request::new(Reads::Source, &SOURCE)?,
                Request::new(Reads::SoundMode, &SOUND_MODE)?,
                Request::new(Reads::Device, &DEVICE)?,
                Request::new(Reads::Tone, &TONE)?,
                Request::on(Endpoint::AppCommand0300, Reads::Audyssey, &AUDYSSEY)?,
            ],
            UpdateMethod::StatusPages => Vec::new(),
        };
        let (wanted, queue) = mpsc::unbounded_channel();
        tokio::spawn(run(config, reader, requests, queue));
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
    wanted: u8,
    left: u8,
}

async fn run(
    config: FollowUpRead,
    reader: Reader,
    requests: Vec<Request>,
    mut queue: mpsc::UnboundedReceiver<u8>,
) {
    let mut retry: Option<Retry> = None;
    loop {
        let first = if let Some(next) = retry.as_ref() {
            if let Ok(bits) = timeout_at(next.at, queue.recv()).await {
                bits
            } else {
                let (wanted, left) = (next.wanted, next.left - 1);
                tracing::debug!(wanted, "read again over HTTP");
                let unseen = read_until_seen(&reader, &requests, wanted).await;
                retry = (unseen != 0 && left > 0).then(|| Retry {
                    at: Instant::now() + config.retry_every,
                    wanted: unseen,
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
        let pending = pending_after(retry.as_ref().map_or(0, |next| next.wanted), wanted, unseen);
        retry = (pending != 0 && config.retries > 0).then(|| Retry {
            at: Instant::now() + config.retry_every,
            wanted: pending,
            left: config.retries,
        });
    }
}

/// The values that a later read still looks for. `old` was looked for before.
/// `wanted` was read now, and `unseen` is the part of it that did not change.
/// A value that this read showed is not looked for again.
const fn pending_after(old: u8, wanted: u8, unseen: u8) -> u8 {
    let seen = wanted & !unseen;
    (old & !seen) | (unseen & Reads::SLOW)
}

/// Reads, and returns the part of `wanted` that the read did not change. A
/// read of values that are not late counts as seen.
async fn read_until_seen(reader: &Reader, requests: &[Request], wanted: u8) -> u8 {
    let mut changes = reader.channels.subscribe_changes();
    if let Err(error) = read(reader, requests, wanted).await {
        tracing::debug!(%error, "the read after a command failed");
        return wanted;
    }
    let mut seen = 0;
    loop {
        match changes.try_recv() {
            Ok(change) => seen |= Reads::of_setting(change.setting),
            Err(broadcast::error::TryRecvError::Lagged(_)) => seen = u8::MAX,
            Err(_) => break,
        }
    }
    wanted & !seen
}

async fn read(reader: &Reader, requests: &[Request], wanted: u8) -> Result<(), Error> {
    let mut result = Ok(());
    if reader.method == UpdateMethod::StatusPages {
        if wanted & !(Reads::Tone.bit() | Reads::Audyssey.bit() | Reads::Tuner.bit()) != 0 {
            result = read_status_pages(&reader.http, &reader.channels, &reader.zones).await;
        }
    } else {
        for request in requests
            .iter()
            .filter(|r| wanted & r.reads.bit() != 0 && r.useful(reader))
        {
            let read = read_appcommand(
                &reader.http,
                &reader.channels,
                &reader.zones,
                request.endpoint,
                request.commands,
                request.body.clone(),
            )
            .await;
            if let Err(error) = &read {
                note_missing(&reader.missing, request.reads, error);
            }
            result = result.and(read);
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

/// Notes a read that the receiver does not answer, so that it is not sent
/// again.
fn note_missing(missing: &Missing, reads: Reads, error: &Error) {
    match reads {
        // Only a missing file means that the receiver does not have it.
        Reads::Audyssey if matches!(error, Error::Status { status: 404, .. }) => {
            missing.appcommand0300.store(true, Ordering::Relaxed);
        }
        Reads::Tone if missing.may_lack_tone_control(error) => {
            missing.tone_control.store(true, Ordering::Relaxed);
        }
        _ => {}
    }
}

/// Reads the band and the frequency, if the main zone plays the tuner. The
/// page has no values for another source.
async fn read_tuner_page(reader: &Reader) -> Result<(), Error> {
    if playing_kind(&reader.channels) != Some(PlayingKind::Tuner) {
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
}
