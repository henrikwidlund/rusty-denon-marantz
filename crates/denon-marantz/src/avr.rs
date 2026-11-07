//! A receiver: identification, state and updates over HTTP.

use bytes::Bytes;
#[cfg(feature = "telnet")]
use denon_marantz_proto::Trigger;
use denon_marantz_proto::appcommand::{
    self, Cmd, Definition, Endpoint, Field, Response, write_body,
};
use denon_marantz_proto::status_page::{self, Page};
#[cfg(feature = "telnet")]
use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{Brand, Change, DeviceDescription, ReceiverType, State, Zone};
use tokio::sync::{broadcast, watch};

#[cfg(feature = "telnet")]
use crate::audyssey::Audyssey;
use crate::channels::Channels;
#[cfg(feature = "telnet")]
use crate::dirac::Dirac;
use crate::error::Error;
use crate::http::{HttpClient, HttpConfig};
use crate::identify::{Identified, identify, identify_on};
#[cfg(feature = "telnet")]
use crate::sound_control::SoundControl;
#[cfg(feature = "telnet")]
use crate::telnet::{self, Telnet, TelnetConfig, TelnetEvent, TelnetStatus};
#[cfg(feature = "telnet")]
use crate::tone_control::ToneControl;
#[cfg(feature = "telnet")]
use crate::tuner_control::TunerControl;
#[cfg(feature = "telnet")]
use crate::video_control::VideoControl;
#[cfg(feature = "telnet")]
use crate::zone_control::ZoneControl;

/// Name of a receiver that has none.
const DEFAULT_NAME: &str = "AVR";

/// Queries that fill the state. The answer has one item for each, in this order.
/// Receivers of the 2016 generation and later get [`appcommand::GET_TONE_CONTROL`]
/// after them.
const UPDATE_COMMANDS: [Definition; 12] = [
    appcommand::GET_ALL_ZONE_POWER_STATUS,
    appcommand::GET_AUTO_STANDBY,
    appcommand::GET_DIMMER,
    appcommand::GET_ECO,
    appcommand::GET_ALL_ZONE_SOURCE,
    appcommand::GET_ALL_ZONE_VOLUME,
    appcommand::GET_ALL_ZONE_MUTE_STATUS,
    appcommand::GET_SURROUND_MODE_STATUS,
    appcommand::GET_RENAME_SOURCE,
    appcommand::GET_DELETED_SOURCE,
    appcommand::GET_ZONE_NAME,
    appcommand::GET_FRIENDLY_NAME,
];

/// How the state is read over HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UpdateMethod {
    /// `AppCommand.xml`. One request reads all zones.
    AppCommand,
    /// Status pages. Receivers without `AppCommand` have them. Two requests
    /// for each zone.
    StatusPages,
}

/// What is known about a receiver after [`AvrBuilder::connect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// Host that was given.
    pub host: String,
    /// Generation of the receiver.
    pub receiver_type: ReceiverType,
    /// Port of the web interface.
    pub port: u16,
    /// How the state is read.
    pub update_method: UpdateMethod,
    /// Brand. [`Brand::Denon`] if the receiver did not tell.
    pub brand: Brand,
    /// Manufacturer from the device description.
    pub manufacturer: Option<String>,
    /// Model from the device description.
    pub model_name: Option<String>,
    /// Serial number from the device description.
    pub serial_number: Option<String>,
    /// Name that the user gave the receiver.
    pub friendly_name: String,
    /// Version of the communication API.
    pub comm_api_version: Option<String>,
    /// MAC address of the wired network, as 12 hexadecimal digits without
    /// separators.
    pub mac_address: Option<String>,
    /// The receiver supports Dirac Live.
    pub dirac_live: bool,
    /// Zones that [`Avr::update`] reads. The main zone and the others that the
    /// receiver has.
    pub zones: Vec<Zone>,
}

/// Settings for a connection. Start with [`Avr::builder`].
#[derive(Debug, Clone)]
pub struct AvrBuilder {
    host: String,
    http: HttpConfig,
    probes: Option<Vec<(ReceiverType, u16)>>,
    description_port: Option<u16>,
    #[cfg(feature = "telnet")]
    telnet: TelnetConfig,
}

/// A receiver that is read over HTTP.
///
/// The state is in a [`watch`] channel. [`Avr::update`] reads it from the
/// receiver. Everything is `&self`, so one value can be shared between tasks.
#[derive(Debug)]
pub struct Avr {
    http: HttpClient,
    info: Info,
    /// The queries of [`Avr::update`].
    update_commands: Box<[Definition]>,
    /// The request body of `update_commands`.
    update_body: Bytes,
    channels: Channels,
    #[cfg(feature = "telnet")]
    telnet: Telnet,
}

impl Avr {
    /// Starts the settings for a receiver at `host`.
    #[must_use]
    pub fn builder(host: impl Into<String>) -> AvrBuilder {
        AvrBuilder {
            host: host.into(),
            http: HttpConfig::default(),
            probes: None,
            description_port: None,
            #[cfg(feature = "telnet")]
            telnet: TelnetConfig::default(),
        }
    }

    /// What is known about the receiver.
    #[must_use]
    pub const fn info(&self) -> &Info {
        &self.info
    }

    /// Watches the state. It is empty until [`Avr::update`] has run.
    #[must_use]
    pub fn state(&self) -> watch::Receiver<State> {
        self.channels.watch_state()
    }

    /// Follows each change of the state, with the zone and the value that
    /// changed. A subscriber that is too slow loses the oldest changes and gets
    /// [`broadcast::error::RecvError::Lagged`]. The state is still correct.
    #[must_use]
    pub fn changes(&self) -> broadcast::Receiver<Change> {
        self.channels.subscribe_changes()
    }

    /// A copy of the state now.
    #[must_use]
    pub fn snapshot(&self) -> State {
        self.channels.snapshot()
    }

    /// Reads the state of all zones from the receiver.
    ///
    /// Watchers are told only if a value changed.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver cannot be reached, or if an answer is
    /// not valid. Values that were read before the error stay in the state.
    pub async fn update(&self) -> Result<(), Error> {
        match self.info.update_method {
            UpdateMethod::AppCommand => self.update_with_appcommand().await,
            UpdateMethod::StatusPages => self.update_with_status_pages().await,
        }
    }

    /// Opens the telnet connection and keeps it open.
    ///
    /// Events from the receiver then change the state at once. If the
    /// connection is lost, a task connects again. Does nothing if the
    /// connection is open. A receiver allows one telnet connection, so a
    /// remote control app can take it away; the task takes it back.
    ///
    /// # Errors
    ///
    /// Returns an error if the first connection fails. Nothing keeps trying
    /// then.
    #[cfg(feature = "telnet")]
    pub async fn telnet_connect(&self) -> Result<(), Error> {
        self.telnet.connect(&self.info.host, &self.channels).await
    }

    /// Closes the telnet connection and stops the task that keeps it open.
    #[cfg(feature = "telnet")]
    pub async fn telnet_disconnect(&self) {
        self.telnet.disconnect().await;
    }

    /// Watches the telnet connection. It is disconnected until
    /// [`Avr::telnet_connect`] has worked, and while the task tries to
    /// connect again.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub fn telnet_status(&self) -> watch::Receiver<TelnetStatus> {
        self.telnet.watch_status()
    }

    /// Sends a command on telnet.
    ///
    /// Commands wait for each other and for the spacing between writes (see
    /// [`TelnetConfig::command_interval`]). They go before the queries that
    /// fill the state. This returns when the command is written. That does not
    /// mean that the receiver did it: its answer is an event. If the command
    /// is waiting when the future is dropped, it is still sent.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotConnected`] if the telnet connection is not open.
    /// Nothing is queued then, so a command is never sent late. Returns
    /// [`Error::InvalidCommand`] if the command has text that cannot be sent.
    #[cfg(feature = "telnet")]
    pub async fn send(&self, zone: Zone, command: Command<'_>) -> Result<(), Error> {
        self.telnet
            .send(telnet::encode(self.info.brand, zone, &command)?)
            .await
    }

    /// Turns a trigger output on (`true`) or off (`false`). The outputs
    /// switch other equipment. A receiver has up to three, and some models
    /// have fewer.
    ///
    /// # Errors
    ///
    /// See [`Avr::send`].
    #[cfg(feature = "telnet")]
    pub async fn set_trigger(&self, trigger: Trigger, on: bool) -> Result<(), Error> {
        self.send(Zone::Main, Command::SetTrigger(trigger, on))
            .await
    }

    /// Controls one zone on telnet with typed methods.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSuchZone`] if the receiver does not have the zone.
    #[cfg(feature = "telnet")]
    pub fn zone(&self, zone: Zone) -> Result<ZoneControl<'_>, Error> {
        if self.info.zones.contains(&zone) {
            Ok(ZoneControl::new(self, zone))
        } else {
            Err(Error::NoSuchZone(zone))
        }
    }

    /// Controls bass, treble and tone control on telnet with typed methods.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub const fn tone(&self) -> ToneControl<'_> {
        ToneControl::new(self)
    }

    /// Controls the Audyssey settings on telnet with typed methods.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub const fn audyssey(&self) -> Audyssey<'_> {
        Audyssey::new(self)
    }

    /// Controls the sound settings on telnet with typed methods.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub const fn sound(&self) -> SoundControl<'_> {
        SoundControl::new(self)
    }

    /// Controls the tuner on telnet with typed methods.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub const fn tuner(&self) -> TunerControl<'_> {
        TunerControl::new(self)
    }

    /// Controls the video settings on telnet with typed methods.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub const fn video(&self) -> VideoControl<'_> {
        VideoControl::new(self)
    }

    /// Controls Dirac Live on telnet with typed methods.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DiracNotSupported`] if the receiver does not support
    /// Dirac Live.
    #[cfg(feature = "telnet")]
    pub const fn dirac(&self) -> Result<Dirac<'_>, Error> {
        if self.info.dirac_live {
            Ok(Dirac::new(self))
        } else {
            Err(Error::DiracNotSupported)
        }
    }

    /// Asks the receiver for its state again on telnet. The answers are
    /// events, and events that repeat the last value are shown again.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotConnected`] if the telnet connection is not open.
    #[cfg(feature = "telnet")]
    pub async fn telnet_refresh(&self) -> Result<(), Error> {
        self.telnet.refresh().await
    }

    /// Follows the events from the receiver, as they come on telnet. Repeated
    /// events that a query causes are left out. A subscriber that is too slow
    /// loses the oldest events and gets [`broadcast::error::RecvError::Lagged`].
    #[cfg(feature = "telnet")]
    #[must_use]
    pub fn events(&self) -> broadcast::Receiver<TelnetEvent> {
        self.telnet.subscribe_events()
    }

    async fn update_with_appcommand(&self) -> Result<(), Error> {
        self.http
            .post_parsed(
                Endpoint::AppCommand.path(),
                self.update_body.clone(),
                |body| {
                    let response = Response::parse(body, self.update_commands.len())?;
                    self.channels.modify(|state, notify| {
                        for &zone in &self.info.zones {
                            response.extract(&self.update_commands, zone, |field, value| {
                                notify(state.apply_field(zone, field, value));
                            });
                        }
                        // The source lists are for the whole receiver.
                        for (definition, item) in self.update_commands.iter().zip(response.items())
                        {
                            if *definition == appcommand::GET_RENAME_SOURCE {
                                item.renamed_sources(|name, rename| {
                                    notify(state.apply_renamed_source(name, rename));
                                });
                            } else if *definition == appcommand::GET_DELETED_SOURCE {
                                item.source_use(|name, value| {
                                    notify(state.apply_source_use(name, value));
                                });
                            }
                        }
                    });
                    Ok(())
                },
            )
            .await
    }

    async fn update_with_status_pages(&self) -> Result<(), Error> {
        for &zone in &self.info.zones {
            let status = page(&self.http, status_page::status_path(zone)).await?;
            let zone_page = page(&self.http, status_page::zone_path(zone)).await?;
            let (status, zone_page) = (parse_page(status.as_ref()), parse_page(zone_page.as_ref()));
            self.channels.modify(|state, notify| {
                status_page::extract(
                    status.as_ref(),
                    zone_page.as_ref(),
                    zone,
                    &mut |field, value| {
                        notify(state.apply_field(zone, field, value));
                    },
                );
            });
        }
        Ok(())
    }
}

impl AvrBuilder {
    /// Sets timeouts, pacing and limits of the HTTP requests.
    #[must_use]
    pub const fn http_config(mut self, config: HttpConfig) -> Self {
        self.http = config;
        self
    }

    /// Sets the types and ports to try when identifying the receiver, newest
    /// type first. The default is the usual ports of each generation. Use this
    /// if the web interface is on another port, for example behind a proxy.
    #[must_use]
    pub fn probes(mut self, probes: &[(ReceiverType, u16)]) -> Self {
        self.probes = Some(probes.to_vec());
        self
    }

    /// Sets the port of the device description. The default depends on the
    /// type of the receiver.
    #[must_use]
    pub const fn description_port(mut self, port: u16) -> Self {
        self.description_port = Some(port);
        self
    }

    /// Sets the telnet connection: port, timeouts and how soon to connect
    /// again.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub const fn telnet_config(mut self, config: TelnetConfig) -> Self {
        self.telnet = config;
        self
    }

    /// Identifies the receiver and reads what it tells about itself.
    ///
    /// The state is empty until [`Avr::update`] runs.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver cannot be reached. A missing or
    /// unusable device description is not an error: the brand is then
    /// [`Brand::Denon`] and the model is not known.
    pub async fn connect(self) -> Result<Avr, Error> {
        #[cfg(feature = "telnet")]
        self.telnet.check()?;
        let Identified {
            receiver_type,
            port,
            device_info,
        } = match &self.probes {
            Some(probes) => identify_on(&self.host, probes, self.http).await?,
            None => identify(&self.host, self.http).await?,
        };
        let http = HttpClient::new(&self.host, port, self.http)?;
        let description = self.description(receiver_type).await?;
        let (update_method, friendly_name) = detect_update_method(&http, receiver_type).await?;

        let brand = description
            .as_ref()
            .map_or_else(|| Brand::from_manufacturer(None), DeviceDescription::brand);
        let friendly_name = friendly_name
            .or_else(|| {
                description
                    .as_ref()
                    .and_then(|d| owned(d.friendly_name.as_ref()))
            })
            .unwrap_or_else(|| format!("{} {DEFAULT_NAME}", brand.name()));

        let zone_count =
            usize::from(device_info.device_zones.unwrap_or(1)).clamp(1, Zone::ALL.len());
        let info = Info {
            host: self.host,
            receiver_type,
            port,
            update_method,
            brand,
            manufacturer: description.as_ref().map(|d| d.manufacturer.to_string()),
            model_name: description
                .as_ref()
                .and_then(|d| owned(d.model_name.as_ref())),
            serial_number: description
                .as_ref()
                .and_then(|d| owned(d.serial_number.as_ref())),
            friendly_name,
            comm_api_version: owned(device_info.comm_api_version.as_ref()),
            mac_address: owned(device_info.mac_address.as_ref()),
            dirac_live: device_info.dirac_live,
            zones: Zone::ALL[..zone_count].to_vec(),
        };
        tracing::info!(host = %info.host, name = %info.friendly_name, method = ?info.update_method, "connected");

        let mut update_commands = UPDATE_COMMANDS.to_vec();
        if receiver_type == ReceiverType::AvrX2016 {
            update_commands.push(appcommand::GET_TONE_CONTROL);
        }
        let commands: Vec<Cmd<'_>> = update_commands
            .iter()
            .map(|definition| definition.cmd)
            .collect();
        let update_body = request_body(&commands)?;
        let single_zone = device_info.device_zones == Some(1);
        Ok(Avr {
            http,
            info,
            update_commands: update_commands.into_boxed_slice(),
            update_body,
            channels: Channels::new(State::new(single_zone)),
            #[cfg(feature = "telnet")]
            telnet: Telnet::new(self.telnet, brand, single_zone),
        })
    }

    /// Reads the device description. `None` if the receiver has no usable one.
    async fn description(
        &self,
        receiver_type: ReceiverType,
    ) -> Result<Option<DeviceDescription<'static>>, Error> {
        let port = self
            .description_port
            .unwrap_or_else(|| receiver_type.description_port());
        let http = HttpClient::new(&self.host, port, self.http)?;
        match http
            .get_parsed(receiver_type.description_path(), |body| {
                DeviceDescription::parse(body).map(DeviceDescription::into_owned)
            })
            .await
        {
            Ok(description) => Ok(Some(description)),
            Err(error) if error.is_unreachable() => Err(error),
            Err(error) => {
                tracing::warn!(host = %self.host, %error, "no device description, assuming a Denon receiver");
                Ok(None)
            }
        }
    }
}

/// Finds how to read the state, and the friendly name that comes with it.
///
/// AVR-X receivers answer `AppCommand.xml`, unless the firmware lacks it.
/// Then, and for all other receivers, the status pages are used.
async fn detect_update_method(
    http: &HttpClient,
    receiver_type: ReceiverType,
) -> Result<(UpdateMethod, Option<String>), Error> {
    if receiver_type != ReceiverType::Avr {
        let command = appcommand::GET_FRIENDLY_NAME;
        let answer = http
            .post_parsed(
                Endpoint::AppCommand.path(),
                request_body(&[command.cmd])?,
                |body| {
                    let response = Response::parse(body, 1)?;
                    let mut name = None;
                    response.extract(&[command], Zone::Main, |field, value| {
                        if field == Field::FriendlyName {
                            name = value
                                .map(str::trim)
                                .filter(|v| !v.is_empty())
                                .map(str::to_owned);
                        }
                    });
                    Ok(name)
                },
            )
            .await;
        match answer {
            Ok(name) => return Ok((UpdateMethod::AppCommand, name)),
            Err(error) if error.is_unreachable() => return Err(error),
            Err(error) => {
                tracing::info!(%error, "AppCommand is not supported, using the status pages");
            }
        }
    }
    let name = page(http, status_page::zone_path(Zone::Main))
        .await?
        .as_ref()
        .and_then(|body| parse_page(Some(body)))
        .and_then(|page| status_page::friendly_name(&page));
    Ok((UpdateMethod::StatusPages, name))
}

/// The request body for these commands.
fn request_body(commands: &[Cmd<'_>]) -> Result<Bytes, Error> {
    let mut body = Vec::new();
    write_body(commands, &mut body).map_err(|source| Error::Protocol {
        request: "building the AppCommand request".to_owned(),
        source,
    })?;
    Ok(Bytes::from(body))
}

/// Copies optional text.
fn owned(value: Option<&std::borrow::Cow<'_, str>>) -> Option<String> {
    value.map(ToString::to_string)
}

/// Reads a status page. `None` if the receiver has no such page. An error
/// means that the receiver cannot be reached.
async fn page(http: &HttpClient, path: &str) -> Result<Option<Bytes>, Error> {
    match http.get(path).await {
        Ok(body) => Ok(Some(body)),
        Err(error) if error.is_unreachable() => Err(error),
        Err(error) => {
            tracing::debug!(%error, "status page not available");
            Ok(None)
        }
    }
}

/// Checks a status page. An error page counts as no page.
fn parse_page(body: Option<&Bytes>) -> Option<Page<'_>> {
    Page::parse(body?)
        .inspect_err(|error| tracing::debug!(%error, "status page is not usable"))
        .ok()
}
