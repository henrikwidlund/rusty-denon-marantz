# denon-marantz

Async Rust library to monitor and control Denon and Marantz AV receivers.

> Work in progress. Nothing is published and the API will change.

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
  while a Dirac ART filter is on.
- HDMI control, menu lock, firmware update permission, when the network
  answers, and the streaming services (AirPlay, Spotify Connect, Roon Ready,
  TIDAL Connect, Qobuz Connect).
- Discovery with SSDP and mDNS.

A command that the receiver would ignore without an answer is not sent when the
state shows why. The call returns an error that names the reason, for example
a Dirac filter that blocks tone control or Audyssey settings.

## Usage

```rust
use denon_marantz::Avr;
use denon_marantz_proto::Zone;

let avr = Avr::builder("192.168.1.20").connect().await?;
avr.update().await?;
avr.telnet_connect().await?;

avr.zone(Zone::Main)?.set_volume_db(-35.0).await?;

let mut changes = avr.changes();
while let Ok(change) = changes.recv().await {
    println!("{change:?}: {:?}", avr.snapshot().zone(Zone::Main));
}
```

The web interface (port 11080) is read on request, because it does not
send changes:

```rust
avr.update_speaker_details().await?;
avr.update_source_details().await?;
avr.update_settings_availability().await?;
avr.update_system_settings().await?;
```

Each call says what happened to each page. A page that the receiver does not
have or refuses is not an error. A receiver that cannot be reached is an error,
see `Error::is_unreachable`.

## Features of the `denon-marantz` crate

| Feature | Default | Adds |
|---|---|---|
| `telnet` | yes | Live events and commands on port 23. |
| `http` | yes | AppCommand XML and the web interface. |
| `discovery` | yes | Receiver search with SSDP. |
| `sensitive-logs` | no | Full messages in trace logs. They can have personal data. |

## Tested

Tested on a Denon AVC-A10H.

## Not tested

These parts follow the protocol specifications and the other tested parts, but
have not run on a receiver:

- Receivers that are older than the AVC-A10H. The effect speaker, center image
  and center gain texts (`PSEFF`, `PSCEI`, `PSCEG`) were confirmed by other
  users, not on a receiver here.
- `NSE` and `NSA` now playing text on telnet. The AVC-A10H does not answer
  them (it gives now playing info through HEOS), so they are for other receivers.
- The reset of the channel levels with `CVZRL`.
- Sound settings that the receiver ignores in some modes, for example delay
  time, dialog control, Auro-Matic strength and effect level. The receiver
  sends no answer or error, so the state shows no change.
- Dirac filters other than Dirac ART. Only Dirac ART was tested for locking the
  channel levels and the crossovers.
- The value `WhenOn` of the network control setting: the network answers only
  while the receiver is on. Only `Always` was seen. A receiver needs some time
  after power on before the network answers.
- Receivers without the web interface on port 11080. The library gives an error
  after one second for each page call, see `AvrBuilder::ajax_config`.

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

Minimum supported Rust version: 1.88.

## Examples

Run them from the repository root. Replace the address with the one of your
receiver.

```sh
cargo run -p denon-marantz --example discover
cargo run -p denon-marantz --example status -- 192.168.1.20
cargo run -p denon-marantz --example watch -- 192.168.1.20
cargo run -p denon-marantz --example remote -- 192.168.1.20 volume -35
```

`status` prints the serial number of the receiver. Remove it before you share
the output. `remote` runs one command, the header of the file lists them.

## Logging

The crates use `tracing`. Set `RUST_LOG=denon_marantz=debug` to see the
connect steps and the pages that could not be read. `trace` shows each
message. The `sensitive-logs` feature adds full message text.

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
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
