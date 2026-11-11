# denon-marantz

Async Rust library to monitor and control Denon and Marantz AV receivers.

## What it does

The client keeps one state of the receiver. Telnet events change it at once.
The web interface fills the parts that telnet does not report. You read the
state with `Avr::snapshot` and watch it with `Avr::changes`.

- Zones: main, zone 2 and zone 3. Power, volume, mute, source, quick select.
- Sound: sound mode, tone control, Audyssey, Dirac Live, tactile transducer,
  speaker preset, channel levels, dialog enhancer and other sound settings.
- Speakers: crossovers, distances, channel level dials, subwoofer output and
  bass settings, read from the web interface.
- Sources: names (user, default and automatic), hidden sources and the
  physical input of each source.
- Video and signal: HDMI output, video mode, scaler, input and output signal.
- Receiver: eco, dimmer, locks, triggers, Bluetooth transmitter, tuner, remote
  keys.
- Which settings the receiver offers or blocks now, for example the crossovers
  while a Dirac ART filter is on, or the dialog control in a sound mode that
  has none.
- HDMI control, menu lock, firmware update permission, when the network
  answers, and the streaming services (AirPlay, Spotify Connect, Roon Ready,
  TIDAL Connect, Qobuz Connect).
- Menu settings that only the web interface has: the on-screen display, TV
  format, screen saver, HDMI upscaler, HDCP and the 4K/8K format of each input,
  the levels of the inputs, how two-channel sources are played, what the XLR
  outputs carry (models with XLR outputs only), the language, the firmware update settings, usage data and what
  switches each trigger output.
- What plays: title, artist, album and station of network audio and HD radio,
  band and frequency of the tuner, and the address of the picture. A receiver
  that uses HEOS for network audio does not give this.
- Discovery with SSDP and mDNS.

A command that the receiver would ignore without an answer is not sent when the
state shows why. The call returns an error that names the reason, for example
a Dirac filter that blocks tone control or Audyssey settings.

## Usage

```rust
use std::sync::Arc;
use denon_marantz::{Avr, Zone};
use tokio::sync::broadcast::error::RecvError;

let avr = Arc::new(Avr::builder("192.168.1.20").connect().await?);
let mut changes = avr.changes(); // subscribe before the start, so no change is missed
let started = avr.start().await?; // the zones are in the state when this returns

avr.zone(Zone::Main)?.set_volume_db(-35.0).await?;
let report = started.finished().await; // optional: wait for the rest

loop {
    match changes.recv().await {
        Ok(change) => println!("{change:?}: {:?}", avr.snapshot().zone(Zone::Main)),
        // Some changes were lost. The state is still right, so read it again.
        Err(RecvError::Lagged(_)) => println!("{:?}", avr.snapshot().zone(Zone::Main)),
        Err(RecvError::Closed) => break,
    }
}
```

`start` reads the zones over HTTP and returns. In the background it opens
telnet, asks it first for what HTTP does not give (quick select, toggles, the
sound modes and the Dirac Live slots), and reads the web interface pages that
say which settings are available, the video signal and the channels. A page
that fails is asked once more. Telnet is asked for the signal values that the
other requests did not give. The report says what happened to each part. Use
`StartOptions` to leave telnet closed or to skip the signal pages.

Without `start`, `update()` reads the zones over HTTP and `telnet_connect()`
opens telnet. The connection then fetches the groups in
`QueryGroup::START_ORDER` (core, names, toggles and signal). The other groups
are fetched with `telnet_refresh_group`, or all of them by setting
`TelnetConfig::query_groups` to `QueryGroup::DEFAULT_ORDER`.

The query groups, in the order of `QueryGroup::DEFAULT_ORDER`. Ask for one group
again with `telnet_refresh_group`. The counts are the queries for a Denon and for
a Marantz receiver.

| Group | Default | Values | Denon | Marantz |
|---|---|---|---|---|
| `Core` | yes | Power, source, volume and mute of the zones, sound mode, quick select | 9 | 9 |
| `Names` | yes | Source names and hidden sources, sound mode list, friendly name, Dirac Live slots | 6 | 5 |
| `Toggles` | yes | On and off settings, for example tone control and Dynamic EQ | 19 | 22 |
| `Signal` | yes | Video and audio signal | 14 | 14 |
| `Device` | no | Dimmer, eco, sleep, standby, video and picture settings | 19 | 19 |
| `Sound` | no | Sound settings and levels | 36 | 36 |
| `Zones` | no | Volume settings and levels of the zones | 5 | 5 |
| `Picture` | no | Picture mode and adjustments, older receivers only | 6 | 6 |
| `Speakers` | no | Speaker configuration, crossovers and graphic EQ, Denon only | 3 | 0 |

The web interface (port 11080) is read on request, because it does not
send changes:

```rust
avr.update_speaker_details().await?;
avr.update_source_details().await?;
avr.update_settings_availability().await?;
avr.update_detailed_availability().await?; // surround parameters, HDMI setup, video output
avr.update_signal().await?; // resolution, audio signal and channels in one fast request
avr.update_video_details().await?; // on-screen display, TV format, HDCP, 4K/8K per input
avr.update_general_settings().await?; // language, firmware update, triggers
avr.update_system_settings().await?;
```

To read all values again, call `avr.refresh().await?`. If telnet is open it asks on
telnet and reads only the zone names over HTTP. If not, it reads everything over
HTTP. Your code does not choose.

To see the video and audio signal now, call `avr.refresh_signal().await?`. It
reads the resolution, the audio signal and the channels with one fast request
(`update_signal`) if the receiver has it, the HDR and color values from the web
interface, and asks telnet for the rest if it is open. Your code does not choose.

To see what plays, call `avr.refresh_now_playing().await?`. It asks on telnet if
the connection is open and reads the status page if not, so your code does not
choose. The values reach `snapshot()` and `changes()` either way. Nothing asks
again by itself: call it again to see changes, for example on a timer.

The names say which way a call uses. `update_*` reads over HTTP, `telnet_*` uses
telnet, and `refresh*` picks the way for you. The setters are in the controls
(`avr.zone(..)`, `sound()`, `device()` and others). The values that the setters
take are in `denon_marantz::proto`.

Each call says what happened to each page. A page that the receiver does not
have or refuses is not an error. A receiver that cannot be reached is an error,
see `Error::is_unreachable`.

## Features of the `denon-marantz` crate

| Feature | Default | Adds |
|---|---|---|
| `telnet` | yes | Live events and commands on port 23. Needs `http`. |
| `http` | yes | AppCommand XML and the web interface. |
| `discovery` | yes | Receiver search with SSDP. Needs `http`. |
| `sensitive-logs` | no | Full messages in trace logs. They can have personal data. |

## Tested

Tested on a Denon AVC-A10H.

## Not tested

These parts follow the protocol specifications and the other tested parts, but
have not run on a receiver:

- Receivers that are older than the AVC-A10H. The effect speaker, center image
  and center gain texts (`PSEFF`, `PSCEI`, `PSCEG`) were confirmed by other
  users, not on a receiver here.
- What plays (`refresh_now_playing`, `update_now_playing`,
  `telnet_refresh_now_playing`, the `NSE` and `HD` events). The AVC-A10H does not give it, so it is tested with captured pages
  from other receivers and with fake receivers. The `NSA` text is not used.
- The signal pages of the web interface with an HDR signal or an FRL rate: the
  text of these values is only seen as no signal. The values of `ALLM` and
  `QFT` (1 is on) were only seen as off.
- The reset of the channel levels with `CVZRL`.
- Sound settings that the receiver ignores in some modes, for example delay
  time, dialog control, Auro-Matic strength and effect level. The receiver
  sends no answer or error, so the state shows no change.
- Dirac filters other than Dirac ART. Only Dirac ART was tested for locking the
  channel levels and the crossovers.
- The value `WhenOn` of the network control setting: the network answers only
  while the receiver is on. Only `Always` was seen. A receiver needs some time
  after power on before the network answers.
- The XLR output assignment (`XlrAssign`) on a model that has XLR outputs. The
  AVC-A10H has none and answers the page with zeros.
- Receivers without the web interface on port 11080. The library gives an error
  after one second for each page call, see `AvrBuilder::web_interface_config`.

## Notes about receivers

- Some changes made in the receiver menu or with its remote are pushed on
  telnet, some are not. Changes from the web interface and the apps are.
- Telnet answers in standby when network control is `Always`.
- A Dirac ART filter locks the channel levels, the dial page (HTTP status 500)
  and the crossovers.
- The library only reads from the web interface. It never writes to it.

## Crates

| Crate | Role |
|---|---|
| `denon-marantz` | Async client (tokio). |
| `denon-marantz-proto` | Protocol without IO. |
| `denon-marantz-discovery` | SSDP and mDNS parsing without IO. |

The `denon-marantz` crate re-exports the types that most programs need, for
example `Zone` and `Volume`. All others are in `denon_marantz::proto`.

Minimum supported Rust version: 1.88.

## Examples

Run them from the repository root. Replace the address with the one of your
receiver.

```sh
cargo run -p denon-marantz --example discover
cargo run -p denon-marantz --example status -- 192.168.1.20
cargo run -p denon-marantz --example watch -- 192.168.1.20
cargo run -p denon-marantz --example start -- 192.168.1.20
cargo run -p denon-marantz --example remote -- 192.168.1.20 volume -35
```

`status` prints the serial number of the receiver. Remove it before you share
the output. `start` shows how long `Avr::start` takes and what it read. `remote`
runs one command, the header of the file lists them.

## Logging

The crates use `tracing`. Set `RUST_LOG=denon_marantz=debug` to see the
connect steps and the pages that could not be read. `trace` shows each
message. The `sensitive-logs` feature adds full message text.

## Development

```sh
cargo fmt --all --check
RUSTFLAGS="-D warnings" cargo clippy --locked --workspace --all-targets --all-features
cargo test --locked --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps --all-features
cargo deny check
```

Fuzz targets for the parsers are in `fuzz/`. Benchmarks use `criterion`.

## Credits

This is a Rust port of [denonavr](https://github.com/henrikwidlund/denonavr)
(MIT), a Python library by Oliver Götz and contributors. The protocol
knowledge and command tables come from that project.

This project is not made or supported by Denon or Marantz. Denon and Marantz
are trademarks of their respective owners. Other names are the property of
their respective owners.

## License

MIT ([LICENSE](LICENSE)).
