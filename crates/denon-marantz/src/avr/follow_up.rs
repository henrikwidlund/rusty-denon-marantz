//! Reads the values that a command over HTTP changed. A command over HTTP
//! gets no answer, so the state does not change by itself.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use denon_marantz_proto::Zone;
use denon_marantz_proto::appcommand::{self, Definition, Endpoint};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until, timeout};

use super::{Info, UpdateMethod, read_appcommand, read_status_pages, request_body};
use crate::channels::Channels;
use crate::command::Reads;
use crate::error::Error;
use crate::http::HttpClient;

/// When the values that a command over HTTP changed are read.
///
/// The read waits until no command came for `quiet`, so a key that is held
/// down causes one read, not one for each command. It does not wait longer
/// than `longest_wait` after the first command, so the state also changes
/// while commands keep coming. The read has only the values that the command
/// changed: for example the volume and mute for a volume command.
///
/// The receiver shows a new source or sound mode late in its answers. So these
/// are read once more `second_read` after the first read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FollowUpRead {
    /// Time without a command before the read.
    pub quiet: Duration,
    /// Longest time between the first command and the read.
    pub longest_wait: Duration,
    /// Time after a read of the source or the sound mode until it is read
    /// again.
    pub second_read: Duration,
}

impl Default for FollowUpRead {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(500),
            longest_wait: Duration::from_secs(2),
            second_read: Duration::from_millis(1500),
        }
    }
}

impl FollowUpRead {
    pub(super) const fn check(&self) -> Result<(), Error> {
        if self.quiet.is_zero() || self.longest_wait.is_zero() || self.second_read.is_zero() {
            return Err(Error::InvalidConfig(
                "quiet, longest_wait and second_read must be above 0",
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

/// What the worker needs to read.
struct Reader {
    http: Arc<HttpClient>,
    channels: Channels,
    zones: Box<[Zone]>,
    method: UpdateMethod,
}

/// A request for one kind of value.
struct Request {
    reads: Reads,
    commands: &'static [Definition],
    body: Bytes,
}

impl Request {
    fn new(reads: Reads, commands: &'static [Definition]) -> Result<Self, Error> {
        let cmds: Vec<_> = commands.iter().map(|definition| definition.cmd).collect();
        Ok(Self {
            reads,
            commands,
            body: request_body(&cmds)?,
        })
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
        http: &Arc<HttpClient>,
        channels: &Channels,
        info: &Info,
    ) -> Result<Option<Self>, Error> {
        let Some(config) = config else {
            return Ok(None);
        };
        let reader = Reader {
            http: Arc::clone(http),
            channels: channels.clone(),
            zones: info.zones.clone().into_boxed_slice(),
            method: info.update_method,
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

async fn run(
    config: FollowUpRead,
    reader: Reader,
    requests: Vec<Request>,
    mut queue: mpsc::UnboundedReceiver<u8>,
) {
    // When the source and the sound mode are read once more, and what was
    // asked since.
    let mut again: Option<Instant> = None;
    let mut again_wanted = 0_u8;
    loop {
        let first = if let Some(at) = again {
            tokio::select! {
                bits = queue.recv() => bits,
                () = sleep_until(at) => {
                    again = None;
                    let wanted = std::mem::take(&mut again_wanted);
                    tracing::debug!(wanted, "second read over HTTP");
                    read_logged(&reader, &requests, wanted).await;
                    continue;
                }
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
        read_logged(&reader, &requests, wanted).await;
        let slow = wanted & Reads::SLOW;
        if slow != 0 {
            again_wanted |= slow;
            again = Some(Instant::now() + config.second_read);
        }
    }
}

async fn read_logged(reader: &Reader, requests: &[Request], wanted: u8) {
    if let Err(error) = read(reader, requests, wanted).await {
        tracing::debug!(%error, "the read after a command failed");
    }
}

async fn read(reader: &Reader, requests: &[Request], wanted: u8) -> Result<(), Error> {
    if reader.method == UpdateMethod::StatusPages {
        return read_status_pages(&reader.http, &reader.channels, &reader.zones).await;
    }
    let mut result = Ok(());
    for request in requests.iter().filter(|r| wanted & r.reads.bit() != 0) {
        let read = read_appcommand(
            &reader.http,
            &reader.channels,
            &reader.zones,
            Endpoint::AppCommand,
            request.commands,
            request.body.clone(),
        )
        .await;
        result = result.and(read);
    }
    result
}
