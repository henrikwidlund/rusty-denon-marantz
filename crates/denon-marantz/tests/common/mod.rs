//! A fake receiver: a small HTTP server that answers with what a test says.

#![allow(dead_code)] // Each test file uses a part of this.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use denon_marantz::{Avr, AvrBuilder, FollowUpRead, HttpConfig, Pacing};
use denon_marantz_proto::ReceiverType;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;

/// Short timeouts and no pacing, so that tests are quick.
pub(crate) fn config() -> HttpConfig {
    HttpConfig {
        connect_timeout: Duration::from_millis(500),
        request_timeout: Duration::from_millis(500),
        pacing: Pacing::Fixed {
            interval: Duration::ZERO,
        },
        ..HttpConfig::default()
    }
}

/// A request that the fake receiver got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) body: Vec<u8>,
}

/// What the fake receiver does with a request.
pub(crate) enum Reply {
    /// HTTP 200 with this body.
    Body(Vec<u8>),
    /// This status, without a body.
    Status(u16),
    /// Never answer.
    Hang,
    /// Close the connection without an answer.
    Close,
    /// Send these bytes, then close the connection.
    Raw(Vec<u8>),
    /// HTTP 200 with this body, once the flag is true.
    Held(Arc<AtomicBool>, Vec<u8>),
}

pub(crate) struct Fake {
    pub(crate) addr: SocketAddr,
    pub(crate) port: u16,
    pub(crate) seen: Arc<Mutex<Vec<Seen>>>,
    pub(crate) connections: Arc<AtomicUsize>,
}

impl Fake {
    pub(crate) async fn start(handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Self {
        Self::start_on(LOOPBACK, handler).await
    }

    /// Like [`Fake::start`], on the address `bind`.
    pub(crate) async fn start_on(
        bind: &str,
        handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind(bind).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(handler);
        let (seen_for_task, connections_for_task) = (seen.clone(), connections.clone());
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                connections_for_task.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serve(socket, handler.clone(), seen_for_task.clone()));
            }
        });
        Self {
            addr,
            port: addr.port(),
            seen,
            connections,
        }
    }

    pub(crate) fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    pub(crate) fn connection_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

/// Writes an answer with status 200. Returns false if the write failed.
async fn write_body(socket: &mut TcpStream, body: &[u8]) -> bool {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    socket.write_all(head.as_bytes()).await.is_ok() && socket.write_all(body).await.is_ok()
}

async fn serve(
    mut socket: TcpStream,
    handler: Arc<dyn Fn(&Seen) -> Reply + Send + Sync>,
    seen: Arc<Mutex<Vec<Seen>>>,
) {
    let mut buffer = Vec::new();
    loop {
        let Some(request) = read_request(&mut socket, &mut buffer).await else {
            return;
        };
        seen.lock().unwrap().push(request.clone());
        match handler(&request) {
            Reply::Body(body) => {
                if !write_body(&mut socket, &body).await {
                    return;
                }
            }
            Reply::Held(release, body) => {
                while !release.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                if !write_body(&mut socket, &body).await {
                    return;
                }
            }
            Reply::Status(status) => {
                let head = format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\n\r\n");
                if socket.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
            }
            Reply::Hang => tokio::time::sleep(Duration::from_secs(3600)).await,
            Reply::Close => return,
            Reply::Raw(bytes) => {
                let _ = socket.write_all(&bytes).await;
                return;
            }
        }
    }
}

async fn read_request(socket: &mut TcpStream, buffer: &mut Vec<u8>) -> Option<Seen> {
    let head_end = loop {
        if let Some(position) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break position + 4;
        }
        let mut chunk = [0_u8; 1024];
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split(' ');
    let (method, path) = (first.next()?.to_owned(), first.next()?.to_owned());
    let length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len() < head_end + length {
        let mut chunk = [0_u8; 1024];
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = buffer[head_end..head_end + length].to_vec();
    buffer.drain(..head_end + length);
    Some(Seen { method, path, body })
}

// ---------------------------------------------------------------------------
// A receiver for `Avr`

/// The address that the fakes listen on.
pub(crate) const LOOPBACK: &str = "127.0.0.1:0";

/// The IPv6 address that the fakes can listen on.
pub(crate) const LOOPBACK_V6: &str = "[::1]:0";

pub(crate) const DESCRIPTION: &[u8] =
    include_bytes!("../../../denon-marantz-proto/tests/data/xml/AVR-X1600H-description.xml");
pub(crate) const AVR_X_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0301</CommApiVers><ModelName>AVC-X3700H</ModelName>\
    <DeviceZones>2</DeviceZones><MacAddress>0123456789AB</MacAddress></Device_Info>";
/// The device information of a receiver that lists the settings queries
/// of `AppCommand0300.xml` and no signal queries.
pub(crate) const AVR_X_SETTINGS_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0301</CommApiVers><ModelName>AVC-A10H</ModelName>\
    <DeviceZones>2</DeviceZones><DeviceCapabilities><Setup>\
    <StatusInfo><Commands></Commands></StatusInfo>\
    <SurroundParameter><Commands><GetSurroundParameter ver=\"2\">1</GetSurroundParameter>\
    </Commands></SurroundParameter>\
    <BTTX><Commands><GetBTTX ver=\"1\">1</GetBTTX></Commands></BTTX>\
    <Zone2Setup><Commands><GetBassTreble ver=\"1\">1</GetBassTreble></Commands></Zone2Setup>\
    <AudioDelay><Commands><GetAudioDelay ver=\"2\">1</GetAudioDelay></Commands></AudioDelay>\
    <Restorer><Commands><GetRestorerMode ver=\"1\">1</GetRestorerMode></Commands></Restorer>\
    </Setup></DeviceCapabilities></Device_Info>";
/// Like [`AVR_X_SETTINGS_INFO`], with version 1 of `GetSurroundParameter`
/// and `GetAudioDelay`, as receivers older than the 2016 models list them.
pub(crate) const OLDER_SETTINGS_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0300</CommApiVers><ModelName>AVR-X4100W</ModelName>\
    <DeviceZones>2</DeviceZones><DeviceCapabilities><Setup>\
    <StatusInfo><Commands></Commands></StatusInfo>\
    <SurroundParameter><Commands><GetSurroundParameter ver=\"1\">1</GetSurroundParameter>\
    </Commands></SurroundParameter>\
    <BTTX><Commands><GetBTTX ver=\"1\">1</GetBTTX></Commands></BTTX>\
    <Zone2Setup><Commands><GetBassTreble ver=\"1\">1</GetBassTreble></Commands></Zone2Setup>\
    <AudioDelay><Commands><GetAudioDelay ver=\"1\">1</GetAudioDelay></Commands></AudioDelay>\
    <Restorer><Commands><GetRestorerMode ver=\"1\">1</GetRestorerMode></Commands></Restorer>\
    </Setup></DeviceCapabilities></Device_Info>";
/// Like [`AVR_X_SETTINGS_INFO`], and it also lists the video signal query.
pub(crate) const AVR_X_SETTINGS_AND_SIGNAL_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0301</CommApiVers><ModelName>AVC-A10H</ModelName>\
    <DeviceZones>2</DeviceZones><DeviceCapabilities><Setup>\
    <StatusInfo><Commands><GetVideoInfo ver=\"1\">1</GetVideoInfo></Commands></StatusInfo>\
    <SurroundParameter><Commands><GetSurroundParameter ver=\"2\">1</GetSurroundParameter>\
    </Commands></SurroundParameter>\
    <BTTX><Commands><GetBTTX ver=\"1\">1</GetBTTX></Commands></BTTX>\
    <Restorer><Commands><GetRestorerMode ver=\"1\">1</GetRestorerMode></Commands></Restorer>\
    </Setup></DeviceCapabilities></Device_Info>";
/// Answers to the settings requests of `AppCommand0300.xml` for
/// [`AVR_X_SETTINGS_INFO`].
const SETTINGS_FAST: &[u8] = b"<?xml version=\"1.0\" encoding=\"utf-8\" ?><rx>\
    <cmd><name>GetSurroundParameter</name><list>\
    <param name=\"cinemaeq\" control=\"2\">1</param>\
    <param name=\"dyncomp\" control=\"1\">0</param>\
    <param name=\"imax\" control=\"0\"></param></list></cmd>\
    <cmd><name>GetSurroundParameter</name><list>\
    <param name=\"sw\" control=\"0\"></param></list></cmd>\
    <cmd><name>GetBTTX</name><list><param name=\"output\" control=\"1\">2</param></list></cmd>\
    <cmd><name>GetBassTreble</name><list>\
    <param name=\"z2bass\" control=\"2\">-2</param>\
    <param name=\"z2treble\" control=\"2\">0</param></list></cmd></rx>";
const SETTINGS_SLOW: &[u8] = b"<?xml version=\"1.0\" encoding=\"utf-8\" ?><rx>\
    <cmd><name>GetAudioDelay</name><list>\
    <param name=\"audiodelay\" control=\"2\">139</param>\
    <param name=\"autolipsync\" control=\"2\">1</param></list></cmd>\
    <cmd><name>GetRestorerMode</name><list>\
    <param name=\"mode\" control=\"1\">0</param></list></cmd></rx>";
/// The device information of a receiver that lists all queries, also
/// `GetOutputSettings`.
pub(crate) const AVR_X_OUTPUT_SETTINGS_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0301</CommApiVers><ModelName>AVC-A10H</ModelName>\
    <DeviceZones>2</DeviceZones><DeviceCapabilities><Setup>\
    <ToneControl><Functions><GetToneControl>1</GetToneControl></Functions></ToneControl>\
    <Audyssey><Commands><GetAudyssey ver=\"2\">1</GetAudyssey></Commands></Audyssey>\
    <StatusInfo><Commands><GetInputSignal ver=\"1\">1</GetInputSignal>\
    <GetActiveSpeaker ver=\"1\">1</GetActiveSpeaker><GetVideoInfo ver=\"1\">1</GetVideoInfo>\
    <GetAudioInfo ver=\"1\">1</GetAudioInfo></Commands></StatusInfo>\
    <OutputSettings><Commands><GetOutputSettings ver=\"2\">1</GetOutputSettings>\
    </Commands></OutputSettings></Setup></DeviceCapabilities></Device_Info>";
/// The device information of a receiver that lists only `GetAudioInfo`, and
/// no tone control.
pub(crate) const AUDIO_INFO_ONLY_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0300</CommApiVers><ModelName>AVR-X2100W</ModelName>\
    <DeviceZones>2</DeviceZones><DeviceCapabilities><Setup>\
    <StatusInfo><Commands><GetAudioInfo ver=\"1\">1</GetAudioInfo></Commands></StatusInfo>\
    </Setup></DeviceCapabilities></Device_Info>";
/// The item of an `AppCommand0300.xml` answer to `GetOutputSettings`, with
/// `value` as the HDMI output. `2` is HDMI 2.
pub(crate) fn output_settings_item(value: &str) -> String {
    format!(
        "<cmd><name>GetOutputSettings</name><list>\
        <param name=\"videoout\" control=\"2\">{value}</param></list></cmd>"
    )
}
/// The device information of a receiver that supports Dirac Live.
pub(crate) const AVR_X_DIRAC_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0301</CommApiVers><ModelName>AVC-A10H</ModelName>\
    <DeviceZones>2</DeviceZones><Caps><x><DiracLive/></x></Caps></Device_Info>";
/// The answer to the update request, one item for each command.
pub(crate) const UPDATE_ANSWER: &[u8] = b"<rx>\
    <cmd><zone1>ON</zone1><zone2>OFF</zone2></cmd>\
    <cmd><list><listvalue><zone>Main</zone><value>60</value></listvalue>\
        <listvalue><zone>Zone2</zone><value>0</value></listvalue></list></cmd>\
    <cmd><value>2</value></cmd>\
    <cmd><mode>1</mode></cmd>\
    <cmd><zone1><source>MPLAY</source></zone1><zone2><source>CD</source></zone2></cmd>\
    <cmd><zone1><volume>-36.5</volume><limit>-10</limit></zone1>\
        <zone2><volume>--</volume><limit>OFF</limit></zone2></cmd>\
    <cmd><zone1>off</zone1><zone2>ON</zone2></cmd>\
    <cmd><surround>DOLBY  ATMOS</surround></cmd>\
    <cmd><functionrename>\
        <list><name>Media Player</name><rename>Apple TV </rename></list>\
        <list><name>CD</name><rename></rename></list></functionrename></cmd>\
    <cmd><functiondelete>\
        <list><name>Media Player</name><FuncName>Media Player</FuncName><use>1</use></list>\
        <list><name>CD</name><FuncName>CD</FuncName><use>0</use></list></functiondelete></cmd>\
    <cmd><zone1>MAIN ZONE </zone1><zone2>Kitchen</zone2></cmd>\
    <cmd><friendlyname>Denon AVR</friendlyname></cmd>\
    </rx>";

/// The names of the queries of the update request, in the order of the items of
/// [`UPDATE_ANSWER`].
const UPDATE_NAMES: [&str; 12] = [
    "GetAllZonePowerStatus",
    "GetAutoStandby",
    "GetDimmer",
    "GetECO",
    "GetAllZoneSource",
    "GetAllZoneVolume",
    "GetAllZoneMuteStatus",
    "GetSurroundModeStatus",
    "GetRenameSource",
    "GetDeletedSource",
    "GetZoneName",
    "GetFriendlyName",
];

/// The answer to an `AppCommand` request that asks for some of the queries of
/// the update: the items of [`UPDATE_ANSWER`] for the queries in the request.
/// A request that asks for something else gets the whole answer.
pub(crate) fn update_answer_for(request: &[u8]) -> Vec<u8> {
    let request = String::from_utf8_lossy(request);
    let mut asked: Vec<(usize, usize)> = UPDATE_NAMES
        .iter()
        .enumerate()
        .filter_map(|(index, name)| request.find(&format!(">{name}<")).map(|at| (at, index)))
        .collect();
    asked.sort_unstable();
    if asked.is_empty() {
        return UPDATE_ANSWER.to_vec();
    }
    let answer = std::str::from_utf8(UPDATE_ANSWER).unwrap();
    let body = &answer["<rx>".len()..answer.len() - "</rx>".len()];
    let items: Vec<&str> = body.split_inclusive("</cmd>").collect();
    let mut out = String::from("<rx>");
    for (_, index) in asked {
        out.push_str(items[index].trim());
    }
    out.push_str("</rx>");
    out.into_bytes()
}

/// The answer to the update request of a 2016 receiver: the tone control item
/// comes after the others.
pub(crate) fn update_answer_with_tone() -> Vec<u8> {
    let head = &UPDATE_ANSWER[..UPDATE_ANSWER.len() - b"</rx>".len()];
    [
        head,
        b"<cmd><status>1</status><adjust>1</adjust><basslevel>+3dB</basslevel>\
        <bassvalue>9</bassvalue><treblelevel>-6dB</treblelevel>\
        <treblevalue>0</treblevalue></cmd></rx>",
    ]
    .concat()
}

/// The request that a connect sends to read the friendly name. The update
/// request has the same command with others.
pub(crate) fn is_friendly_name_request(body: &[u8]) -> bool {
    let body = String::from_utf8_lossy(body);
    body.contains("GetFriendlyName") && !body.contains("GetAllZonePowerStatus")
}

pub(crate) fn friendly_name_answer(name: &str) -> Vec<u8> {
    format!("<rx><cmd><friendlyname> {name} </friendlyname></cmd></rx>").into_bytes()
}

/// True if the request body asks for the zone names and nothing else.
pub(crate) fn is_zone_names_request(body: &[u8]) -> bool {
    let body = String::from_utf8_lossy(body);
    body.contains("GetZoneName")
        && !body.contains("GetAllZonePowerStatus")
        && !body.contains("GetAutoStandby")
}

pub(crate) fn zone_names_answer() -> Vec<u8> {
    b"<rx><cmd><zone1>Cinema </zone1><zone2>Kitchen</zone2><zone3>Porch</zone3></cmd></rx>".to_vec()
}

/// True if the request body asks for the zone names, the auto standby, the
/// dimmer and the eco mode, as the refresh does when telnet has no device group.
pub(crate) fn is_zone_names_with_device_request(body: &[u8]) -> bool {
    let body = String::from_utf8_lossy(body);
    body.contains("GetZoneName")
        && body.contains("GetAutoStandby")
        && !body.contains("GetAllZonePowerStatus")
}

pub(crate) fn zone_names_with_device_answer() -> Vec<u8> {
    b"<rx><cmd><zone1>Cinema </zone1><zone2>Kitchen</zone2><zone3>Porch</zone3></cmd>\
    <cmd><list><listvalue><zone>Main</zone><value>60</value></listvalue>\
        <listvalue><zone>Zone2</zone><value>0</value></listvalue></list></cmd>\
    <cmd><value>2</value></cmd>\
    <cmd><mode>1</mode></cmd></rx>"
        .to_vec()
}

/// An AVR-X receiver that has all the pages.
pub(crate) async fn avr_x_receiver() -> Fake {
    avr_x_receiver_with_info(AVR_X_INFO).await
}

/// Like [`avr_x_receiver`], with this device information.
pub(crate) async fn avr_x_receiver_with_info(info: &'static [u8]) -> Fake {
    avr_x_receiver_with_pages(info, &[]).await
}

/// Like [`avr_x_receiver`], and it answers `GET` for the paths in `pages`.
pub(crate) async fn avr_x_receiver_with_pages(
    info: &'static [u8],
    pages: &'static [(&'static str, &'static [u8])],
) -> Fake {
    avr_x_receiver_with(
        LOOPBACK,
        info,
        pages,
        Arc::new(AtomicBool::new(false)),
        SettingsReplies::Answer,
    )
    .await
}

/// Like [`avr_x_receiver`], on the address `bind`.
pub(crate) async fn avr_x_receiver_on(bind: &str) -> Fake {
    avr_x_receiver_with(
        bind,
        AVR_X_INFO,
        &[],
        Arc::new(AtomicBool::new(false)),
        SettingsReplies::Answer,
    )
    .await
}

/// Like [`avr_x_receiver`]. It does not answer `AppCommand` requests while
/// `hang` is true.
pub(crate) async fn avr_x_receiver_that_can_hang(hang: Arc<AtomicBool>) -> Fake {
    avr_x_receiver_with(LOOPBACK, AVR_X_INFO, &[], hang, SettingsReplies::Answer).await
}

/// Like [`avr_x_receiver_with_info`] with [`AVR_X_SETTINGS_INFO`]. It answers
/// the slow settings once `release` is true.
pub(crate) async fn avr_x_receiver_with_held_settings(release: Arc<AtomicBool>) -> Fake {
    avr_x_receiver_with_settings(AVR_X_SETTINGS_INFO, SettingsReplies::HeldSlow(release)).await
}

/// How the fake receiver answers the requests to `AppCommand0300.xml`.
pub(crate) enum SettingsReplies {
    /// Both parts of the settings at once.
    Answer,
    /// The slow part of the settings once the flag is true.
    HeldSlow(Arc<AtomicBool>),
    /// The fast part of the settings with HTTP 500.
    FastFails,
    /// Never the fast part of the settings.
    FastHangs,
    /// Both parts of the settings at once, and never the signal.
    SignalHangs,
}

/// Like [`avr_x_receiver_with_info`], with these answers to
/// `AppCommand0300.xml`.
pub(crate) async fn avr_x_receiver_with_settings(
    info: &'static [u8],
    settings: SettingsReplies,
) -> Fake {
    avr_x_receiver_with(
        LOOPBACK,
        info,
        &[],
        Arc::new(AtomicBool::new(false)),
        settings,
    )
    .await
}

async fn avr_x_receiver_with(
    bind: &str,
    info: &'static [u8],
    pages: &'static [(&'static str, &'static [u8])],
    hang: Arc<AtomicBool>,
    settings: SettingsReplies,
) -> Fake {
    Fake::start_on(bind, move |seen| {
        match (seen.method.as_str(), seen.path.as_str()) {
            ("POST", "/goform/AppCommand.xml") if hang.load(Ordering::SeqCst) => Reply::Hang,
            ("GET", "/goform/Deviceinfo.xml") => Reply::Body(info.to_vec()),
            ("GET", "/description.xml") => Reply::Body(DESCRIPTION.to_vec()),
            ("GET", path) if pages.iter().any(|(page, _)| *page == path) => {
                let (_, body) = pages.iter().find(|(page, _)| *page == path).unwrap();
                Reply::Body(body.to_vec())
            }
            ("POST", "/goform/AppCommand.xml") if is_zone_names_request(&seen.body) => {
                Reply::Body(zone_names_answer())
            }
            ("POST", "/goform/AppCommand.xml") if is_zone_names_with_device_request(&seen.body) => {
                Reply::Body(zone_names_with_device_answer())
            }
            ("POST", "/goform/AppCommand.xml") if is_friendly_name_request(&seen.body) => {
                Reply::Body(friendly_name_answer("Living room"))
            }
            ("POST", "/goform/AppCommand.xml") if contains(&seen.body, "GetToneControl") => {
                Reply::Body(update_answer_with_tone())
            }
            ("POST", "/goform/AppCommand.xml") => Reply::Body(update_answer_for(&seen.body)),
            ("POST", "/goform/AppCommand0300.xml") if contains(&seen.body, "GetBTTX") => {
                match settings {
                    SettingsReplies::FastFails => Reply::Status(500),
                    SettingsReplies::FastHangs => Reply::Hang,
                    _ => Reply::Body(SETTINGS_FAST.to_vec()),
                }
            }
            ("POST", "/goform/AppCommand0300.xml") if contains(&seen.body, "GetRestorerMode") => {
                match &settings {
                    SettingsReplies::HeldSlow(release) => {
                        Reply::Held(Arc::clone(release), SETTINGS_SLOW.to_vec())
                    }
                    _ => Reply::Body(SETTINGS_SLOW.to_vec()),
                }
            }
            ("POST", "/goform/AppCommand0300.xml")
                if matches!(settings, SettingsReplies::SignalHangs) =>
            {
                Reply::Hang
            }
            _ => Reply::Status(404),
        }
    })
    .await
}

/// Returns true if the request body has `text`.
pub(crate) fn contains(body: &[u8], text: &str) -> bool {
    String::from_utf8_lossy(body).contains(text)
}

/// The bodies of the requests to `AppCommand0300.xml`.
pub(crate) fn appcommand0300_requests(fake: &Fake) -> Vec<String> {
    fake.requests()
        .into_iter()
        .filter(|seen| seen.path == "/goform/AppCommand0300.xml")
        .map(|seen| String::from_utf8_lossy(&seen.body).into_owned())
        .collect()
}

/// The answer of `AppCommand0300.xml` to the five queries of the update, in
/// the order that the library sends them. `audyssey` is the list of the
/// Audyssey parameters.
pub(crate) fn appcommand0300_answer(audyssey: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\" ?><rx>\
        <cmd><name>GetAudyssey</name><list>{audyssey}</list></cmd>\
        <cmd><name>GetVideoInfo</name><list>\
        <param name=\"videooutput\" control=\"1\">TV 1</param>\
        <param name=\"hdmisigin\" control=\"1\">4K50</param>\
        <param name=\"hdmisigout\" control=\"1\">---</param></list></cmd>\
        <cmd><name>GetAudioInfo</name><list>\
        <param name=\"inputmode\" control=\"1\">HDMI</param>\
        <param name=\"output\" control=\"1\">Speaker</param>\
        <param name=\"signal\" control=\"1\">Dolby Atmos </param>\
        <param name=\"sound\" control=\"1\">Stereo</param>\
        <param name=\"fs\" control=\"1\">44.1 kHz</param></list></cmd>\
        <cmd><name>GetInputSignal</name><list>\
        <param name=\"inputsiga1\" control=\"2\">FL</param>\
        <param name=\"inputsigb1\" control=\"2\">FR</param>\
        <param name=\"inputsigc1\" control=\"1\">C</param></list></cmd>\
        <cmd><name>GetActiveSpeaker</name><list>\
        <param name=\"activespa1\" control=\"2\">FL</param>\
        <param name=\"activespb1\" control=\"2\">FR</param>\
        <param name=\"activespc1\" control=\"2\">SW</param>\
        <param name=\"activespd1\" control=\"2\">FHL</param></list></cmd></rx>"
    )
}

/// The item of the command `name` in [`appcommand0300_answer`] with no
/// Audyssey parameters.
pub(crate) fn appcommand0300_item(name: &str) -> String {
    let answer = appcommand0300_answer("");
    let start = answer.find(&format!("<cmd><name>{name}</name>")).unwrap();
    let end = start + answer[start..].find("</cmd>").unwrap() + "</cmd>".len();
    answer[start..end].to_owned()
}

/// The items of an `AppCommand0300.xml` answer that `request` asks for, as the
/// receiver does. An answer that has only asked items stays as it is, also
/// its error items.
fn only_asked(reply: Reply, request: &[u8]) -> Reply {
    let Reply::Body(body) = reply else {
        return reply;
    };
    let (Ok(answer), Ok(request)) = (std::str::from_utf8(&body), std::str::from_utf8(request))
    else {
        return Reply::Body(body);
    };
    let items: Vec<&str> = answer
        .split("<cmd>")
        .skip(1)
        .filter_map(|item| item.split("</cmd>").next())
        .collect();
    let asked = |item: &str| {
        item.split("<name>")
            .nth(1)
            .and_then(|rest| rest.split("</name>").next())
            .is_some_and(|name| request.contains(&format!("<name>{name}</name>")))
    };
    if items.iter().all(|item| asked(item)) {
        return Reply::Body(body);
    }
    let mut kept = String::from("<?xml version=\"1.0\" encoding=\"utf-8\" ?><rx>");
    for item in items.iter().filter(|item| asked(item)) {
        kept.push_str("<cmd>");
        kept.push_str(item);
        kept.push_str("</cmd>");
    }
    kept.push_str("</rx>");
    Reply::Body(kept.into_bytes())
}

/// A fake 2016 receiver. `answer` gives the reply to each request of
/// `AppCommand0300.xml`, with the number of the request, from 0.
pub(crate) async fn receiver_with_signal_answers(
    tone: bool,
    answer: impl Fn(usize) -> Reply + Send + Sync + 'static,
) -> Fake {
    receiver_with_info_and_signal_answers(AVR_X_INFO, tone, answer).await
}

/// Like [`receiver_with_signal_answers`], with this device information.
pub(crate) async fn receiver_with_info_and_signal_answers(
    info: &'static [u8],
    tone: bool,
    answer: impl Fn(usize) -> Reply + Send + Sync + 'static,
) -> Fake {
    let asked = AtomicUsize::new(0);
    Fake::start(
        move |seen| match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", "/goform/Deviceinfo.xml") => Reply::Body(info.to_vec()),
            ("POST", "/goform/AppCommand0300.xml") => {
                only_asked(answer(asked.fetch_add(1, Ordering::Relaxed)), &seen.body)
            }
            ("POST", _) if is_friendly_name_request(&seen.body) => {
                Reply::Body(friendly_name_answer("X"))
            }
            ("POST", _) if tone => Reply::Body(update_answer_with_tone()),
            ("POST", _) => Reply::Body(UPDATE_ANSWER.to_vec()),
            _ => Reply::Status(404),
        },
    )
    .await
}

/// A fake 2016 receiver. It answers the Audyssey query with `audyssey`, or
/// with a 404 if that is `None`.
pub(crate) async fn receiver_with_audyssey(audyssey: Option<&'static str>, tone: bool) -> Fake {
    receiver_with_signal_answers(tone, move |_| {
        audyssey.map_or(Reply::Status(404), |body| {
            Reply::Body(body.as_bytes().to_vec())
        })
    })
    .await
}

/// A fake web interface that answers `GET` for the paths in `pages` and 404
/// for all others.
pub(crate) async fn web_interface_with_pages(
    pages: &'static [(&'static str, &'static [u8])],
) -> Fake {
    Fake::start(move |seen| {
        pages
            .iter()
            .find(|(path, _)| *path == seen.path)
            .map_or(Reply::Status(404), |(_, body)| Reply::Body(body.to_vec()))
    })
    .await
}

/// A read after commands that comes soon, so that tests are quick.
pub(crate) const fn quick_follow_up() -> FollowUpRead {
    FollowUpRead {
        quiet: Duration::from_millis(60),
        longest_wait: Duration::from_millis(200),
        retry_every: Duration::from_millis(100),
        retries: 1,
    }
}

/// The bodies of the requests after the first `known` requests that `keep`
/// selects.
pub(crate) fn bodies_after(fake: &Fake, known: usize, keep: impl Fn(&Seen) -> bool) -> Vec<String> {
    fake.requests()
        .into_iter()
        .skip(known)
        .filter(|seen| keep(seen))
        .map(|seen| String::from_utf8_lossy(&seen.body).into_owned())
        .collect()
}

/// The settings for an `Avr` that talks to `fake`, with no pause after a
/// command.
pub(crate) fn builder(fake: &Fake, receiver_type: ReceiverType) -> AvrBuilder {
    let builder = Avr::builder("127.0.0.1")
        .http_config(config())
        .background_pause(Duration::ZERO)
        .probes(&[(receiver_type, fake.port)])
        .description_port(fake.port);
    #[cfg(feature = "heos")]
    let builder = builder.heos_config(no_heos());
    builder
}

/// HEOS settings that fail fast, so that a start does not wait for HEOS: the
/// port refuses connections.
#[cfg(feature = "heos")]
pub(crate) fn no_heos() -> denon_marantz::heos::HeosConfig {
    denon_marantz::heos::HeosConfig {
        port: CLOSED_PORT,
        connect_timeout: Duration::from_millis(200),
        command_timeout: Duration::from_millis(200),
        ..denon_marantz::heos::HeosConfig::default()
    }
}

/// Returns true for a query text that a receiver of [`AVR_X_INFO`] or of
/// another info with `<DeviceZones>2` gets: no zone 3 queries.
pub(crate) fn fake_receiver_gets(text: &str) -> bool {
    denon_marantz_proto::telnet::query_for_text(text)
        .is_none_or(|query| query.zone() != denon_marantz::Zone::Zone3)
}

/// The Denon queries of the group that the fake receiver gets, see
/// [`fake_receiver_gets`]. Not filtered by spec generation.
pub(crate) fn fake_group_queries(
    group: denon_marantz_proto::telnet::QueryGroup,
) -> impl Iterator<Item = &'static str> {
    denon_marantz_proto::telnet::group_queries(denon_marantz_proto::Brand::Denon, group)
        .filter(|text| fake_receiver_gets(text))
}

/// Waits until `condition` is true. Fails the test after two seconds.
pub(crate) async fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..200 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

// ---------------------------------------------------------------------------
// A fake telnet server

enum Control {
    Send(Vec<u8>),
    Close,
}

/// A fake telnet port. It sends and closes on command and records what the
/// client sends. The commands go to the newest connection.
pub(crate) struct FakeTelnet {
    pub(crate) port: u16,
    connections: Arc<AtomicUsize>,
    closed_by_client: Arc<AtomicUsize>,
    received: Arc<Mutex<Vec<u8>>>,
    control: Arc<Mutex<Option<mpsc::UnboundedSender<Control>>>>,
    /// Commands that came before the first connection was accepted.
    backlog: Arc<Mutex<Vec<Control>>>,
}

impl FakeTelnet {
    pub(crate) async fn start() -> Self {
        Self::start_on(LOOPBACK).await
    }

    /// Like [`FakeTelnet::start`], on the address `bind`.
    pub(crate) async fn start_on(bind: &str) -> Self {
        let listener = TcpListener::bind(bind).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let fake = Self {
            port,
            connections: Arc::new(AtomicUsize::new(0)),
            closed_by_client: Arc::new(AtomicUsize::new(0)),
            received: Arc::new(Mutex::new(Vec::new())),
            control: Arc::new(Mutex::new(None)),
            backlog: Arc::new(Mutex::new(Vec::new())),
        };
        let (connections, closed, received, control, backlog) = (
            fake.connections.clone(),
            fake.closed_by_client.clone(),
            fake.received.clone(),
            fake.control.clone(),
            fake.backlog.clone(),
        );
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                connections.fetch_add(1, Ordering::SeqCst);
                let (sender, commands) = mpsc::unbounded_channel();
                // The lock order is the same as in `FakeTelnet::control`.
                let mut newest = control.lock().unwrap();
                for command in backlog.lock().unwrap().drain(..) {
                    let _ = sender.send(command);
                }
                *newest = Some(sender);
                drop(newest);
                tokio::spawn(serve_telnet(
                    socket,
                    commands,
                    received.clone(),
                    closed.clone(),
                ));
            }
        });
        fake
    }

    /// Sends bytes to the newest connection.
    pub(crate) fn send(&self, bytes: &[u8]) {
        self.control(Control::Send(bytes.to_vec()));
    }

    /// Closes the newest connection from this side.
    pub(crate) fn close(&self) {
        self.control(Control::Close);
    }

    fn control(&self, command: Control) {
        match self.control.lock().unwrap().as_ref() {
            Some(sender) => {
                let _ = sender.send(command);
            }
            None => self.backlog.lock().unwrap().push(command),
        }
    }

    /// Connections that were opened.
    pub(crate) fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    /// Connections that the client closed.
    pub(crate) fn closed_by_client(&self) -> usize {
        self.closed_by_client.load(Ordering::SeqCst)
    }

    /// Everything that the client sent, as text.
    pub(crate) fn received(&self) -> String {
        String::from_utf8_lossy(&self.received.lock().unwrap()).into_owned()
    }
}

async fn serve_telnet(
    mut socket: TcpStream,
    mut control: mpsc::UnboundedReceiver<Control>,
    received: Arc<Mutex<Vec<u8>>>,
    closed_by_client: Arc<AtomicUsize>,
) {
    let mut buffer = [0_u8; 1024];
    loop {
        tokio::select! {
            read = socket.read(&mut buffer) => match read {
                Ok(0) | Err(_) => {
                    closed_by_client.fetch_add(1, Ordering::SeqCst);
                    return;
                }
                Ok(count) => received.lock().unwrap().extend_from_slice(&buffer[..count]),
            },
            command = control.recv() => match command {
                Some(Control::Send(bytes)) => {
                    if socket.write_all(&bytes).await.is_err() {
                        return;
                    }
                }
                Some(Control::Close) | None => return,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// A fake SSDP port

/// Answers each datagram that it gets with the same datagrams. It records the
/// datagrams.
pub(crate) struct FakeSsdp {
    pub(crate) addr: SocketAddrV4,
    requests: Arc<Mutex<Vec<String>>>,
}

impl FakeSsdp {
    pub(crate) async fn start(answers: Vec<Vec<u8>>) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, socket.local_addr().unwrap().port());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        tokio::spawn(async move {
            let mut buffer = [0_u8; 2048];
            while let Ok((length, sender)) = socket.recv_from(&mut buffer).await {
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[..length]).into_owned());
                for answer in &answers {
                    let _ = socket.send_to(answer, sender).await;
                }
            }
        });
        Self { addr, requests }
    }

    /// The search requests that came.
    pub(crate) fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

/// An answer to a search with this location.
pub(crate) fn ssdp_answer(location: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: {location}\r\n\
         ST: upnp:rootdevice\r\nUSN: uuid:1234::upnp:rootdevice\r\n\r\n"
    )
    .into_bytes()
}

/// The device information of a device that is not an AV receiver.
pub(crate) const SOUND_BAR_INFO: &[u8] = b"<Device_Info><CommApiVers>0301</CommApiVers>\
    <ModelName>Denon Home</ModelName><CategoryName>SOUND BAR</CategoryName></Device_Info>";

/// The device information of an AV receiver with a category.
pub(crate) const AV_RECEIVER_INFO: &[u8] = b"<Device_Info><CommApiVers>0301</CommApiVers>\
    <ModelName>AVC-X3700H</ModelName><CategoryName>AV RECEIVER</CategoryName></Device_Info>";

/// A local port that refuses connections. Test servers bind port 0 and get a
/// port from the ephemeral range, so no test can take this port.
pub(crate) const CLOSED_PORT: u16 = 1;

/// A second port like [`CLOSED_PORT`], for tests that need two of them.
pub(crate) const OTHER_CLOSED_PORT: u16 = 2;
