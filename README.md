# denon-marantz

Async Rust library to monitor and control Denon and Marantz AV receivers.
The HEOS crates read what plays on HEOS devices and control the playback.

## What it does

The client keeps one state of the receiver. Telnet events change it at once.
The web interface fills the parts that telnet does not report. You read the
state with `Avr::snapshot` and watch it with `Avr::changes`. You control the
receiver with typed setters (`avr.zone(..)` and others), on telnet or, without
telnet, over HTTP. Telnet connects again by itself after a drop and reads the
state again.

- Zones: main, zone 2 and zone 3. Power, volume, mute, source, quick select.
- Sound: sound mode, tone control, Audyssey, Dirac Live, tactile transducer,
  speaker preset, channel levels, dialog enhancer and other sound settings.
  For older receivers also Dialog Level Adjust, Audyssey DSX with its stage
  width and height, the Dolby Pro Logic IIz height gain, the Pro Logic mode,
  the surround back mode, dynamic compression and auto flag detect. Front
  height output for Marantz and older Denon receivers.
- Speakers: crossovers, distances, channel level dials, subwoofer output and
  bass settings, read from the web interface.
- Sources: names (user, default and automatic), hidden sources and the
  physical input of each source.
- Video and signal: HDMI output, video mode, scaler, input and output signal.
- Receiver: eco, dimmer, locks, triggers, Bluetooth transmitter, video select,
  remote keys. For older receivers also REC SELECT and InstaPrevue.
- Tuner: band, mode, frequency, presets and preset names. Presets work in the
  form 01 to 56 and in the form A1 to G8 of older receivers; the library uses
  the form that the receiver reports. HD Radio in North
  America models: frequency, multicast channel, presets, signal level and
  station information.
- Network audio of older receivers: next, previous, repeat, random, cursor,
  pages and presets with their names.
- Which settings the receiver offers or blocks now, for example the crossovers
  while a Dirac ART filter is on, or the dialog control in a sound mode that
  has none.
- HDMI control, menu lock, firmware update permission, when the network
  answers, and the streaming services (AirPlay, Spotify Connect, Roon Ready,
  TIDAL Connect, Qobuz Connect).
- Menu settings that only the web interface has: the on-screen display, TV
  format, screen saver, HDMI upscaler, HDCP and the 4K/8K format of each input,
  the levels of the inputs, how two-channel sources are played, what the XLR
  outputs carry (models with XLR outputs only), the language, the firmware
  update settings, usage data and what switches each trigger output.
- What plays: title, artist, album and station of network audio and HD radio,
  band and frequency of the tuner, and the address of the picture. A receiver
  that uses HEOS for network audio does not give this: use the
  [HEOS crates](#heos) for it.
- How many quick selects the receiver lists (`Info::quick_select_count`).
- Whether the receiver answers the HDMI output over HTTP
  (`Info::hdmi_output_over_http()`). Receivers from about 2016 do.
- Which `AppCommand` queries the receiver lists (`Info::queries`). Receivers
  from about 2015 list them, and the library sends only those.
- Whether the receiver has a tuner and which bands it has (`Info::tuner`).
  Receivers older than about 2013 do not say. Newer receivers do not say
  either: they list a tuner source also when only the models of some regions
  have a tuner.
- Discovery with SSDP, and with mDNS when the `mdns` feature is on.

A command that the receiver would ignore without an answer is not sent when the
state shows why. The call returns an error that names the reason, for example
a Dirac filter that blocks tone control or Audyssey settings.

## Install

The crates are not on crates.io yet. Use the repository:

```toml
[dependencies]
denon-marantz = { git = "https://github.com/henrikwidlund/rusty-denon-marantz" }
# What plays on HEOS, and the playback
denon-marantz-heos = { git = "https://github.com/henrikwidlund/rusty-denon-marantz" }
```

## Usage

```rust
use std::sync::Arc;
use denon_marantz::{Avr, Zone};
use tokio::sync::broadcast::error::RecvError;

let avr = Arc::new(Avr::builder("192.168.1.20").connect().await?);
let mut changes = avr.changes(); // subscribe before the start, so no change is missed
let mut started = avr.start().await?; // the zones are in the state when this returns

avr.zone(Zone::Main)?.set_volume_db(-35.0).await?;
let report = started.all_synced().await; // optional: wait for the telnet answers too

loop {
    match changes.recv().await {
        Ok(change) => println!("{change:?}: {:?}", avr.snapshot().zone(Zone::Main)),
        // Some changes were lost. The state is still right, so read it again.
        Err(RecvError::Lagged(_)) => println!("{:?}", avr.snapshot().zone(Zone::Main)),
        Err(RecvError::Closed) => break,
    }
}
```

`start` reads the zones over HTTP, opens telnet and returns. If the telnet port
does not answer, it waits for the connect timeout first. In the background it
asks telnet first for what HTTP does not give (quick select, toggles, the sound
modes and the Dirac Live slots). It also reads, one after the other, the video
signal page, the audio signal and the channels, the Audyssey settings, the
video values and the HDMI output, and the web interface pages that say which
settings are available. A page that fails is asked once more. Telnet is asked
for the signal values that the other requests did not give. The report says
what happened to each part. Pass `StartOptions` to `avr.start_with(..)` to
leave telnet or HEOS closed, to skip the signal pages or the settings below, or
to change how long `all_synced` waits. A second `start`
while one still runs gives `Error::StartRunning` and reads nothing.

`start` returns when the zones are read. `basic_synced()` waits until the HTTP
reads and the pages are done. `all_synced()` also waits until the receiver has
answered the telnet queries of the start, and each list answer has its `END`
line, so the values that only telnet gives are in the state. A query without an
answer for `TelnetConfig::answer_quiet` (2 s) counts as not answered, and the
report lists it in `telnet_not_answered`. A query whose answer comes late, but
before the report, is not listed. If `StartOptions::telnet_wait` ends
first, the list has all queries that were still open then. Queries that the
library asks by itself, for example for a signal check, are not listed. Both
are optional: values that arrive later come as events.
Both keep the report, so `basic_synced()` and then `all_synced()` works, and a
wait that was stopped by a timeout can be called again.
Dropping the `Started` value does not stop the start. `Started::stop()` stops
it, also while a page or the slow settings below wait for an answer.
`Started::stopper()` gives a handle that stops it from another task.
When telnet is not connected, the start reads some settings over
`AppCommand0300.xml` that otherwise only telnet gives, for example cinema EQ,
DRC, the Bluetooth output and the zone 2 and 3 tone. Only the queries that
`Deviceinfo.xml` lists are sent, only if it lists version 2 of
`GetSurroundParameter` (the 2016 and newer models), and only for the features
that it lists. The fast ones are read before `basic_synced()` returns, the
slow ones (audio delay, auto lip sync, speaker preset, dialog enhancer,
restorer) after it, and `all_synced()` waits for them. Nothing is read if
the signal request got no answer, and the slow ones are not read if the fast
ones failed. `refresh()` reads them again. `StartOptions::settings(false)`
skips them at the start. A value that the receiver leaves
blank, or marks as not used in the current sound mode, does not change the
state. The receiver can also mark a setting as not used in a mode where it
applies, for example cinema EQ in Dolby Atmos. Then the old value stays until
telnet or a later read gives it.
More groups in `TelnetConfig::query_groups` are asked after the first queries,
and `all_synced()` then takes longer. Commands go before the queries, and the
queries wait 250 ms after a command (`AvrBuilder::background_pause`).

`connect` asks the receiver for its type, its description and how to read
it. To skip these requests, store `avr.info()` and pass it to
`AvrBuilder::known` next time: `connect` then sends nothing, and commands can
go out at once, over HTTP until telnet is open. `start` checks the stored
values first, in the background: `started.info_check()` gives the result
before the pages are read. `Avr::verify` does the same without a start. If
the receiver gives other values, connect again with the new `Info` and store
it. The `serde` feature makes `Info` serializable.

`avr.missing_sources()` tells which HTTP sources the receiver does not have,
for example a page that answered 404. Store it with `Info` and pass it to
`AvrBuilder::missing`, so that a new `Avr` does not ask them again and
`http_reads` gives the same result from the start. It is used only with
`AvrBuilder::known`. Read it again after HTTP reads, because they can find
more missing sources. Drop it when the stored `Info` changes. The library
never removes a missing source, so connect without it to find the sources
again.

If the network of the caller can be down for a moment, for example after the
device wakes up, set `HttpConfig::connect_retry`: a request whose connection
cannot be built is tried again within that time. It was not sent, so this is
safe for commands too. A short `HttpConfig::connect_timeout` makes a lost
connect attempt end sooner. `Error::is_not_sent` tells if a failed request
surely did not reach the receiver, so that sending it again cannot run a
command twice.

Without `start`, `update()` reads the zones over HTTP and `telnet_connect()`
opens telnet. The connection then fetches the groups in
`QueryGroup::START_ORDER` (core, names, toggles and signal). The other groups
are fetched with `telnet_refresh_group`, or all of them by setting
`TelnetConfig::query_groups` to `QueryGroup::DEFAULT_ORDER`.

The receiver does not send the values that depend on a changed value, for
example the Dirac Live filters when the speaker preset changes. The library does
not ask for them: a client that shows such values listens for the event and
asks for exactly those values with `telnet_ask`, for example
`Query::DiracFilter` and `Query::DiracSlots`. A client that does not show them
does not need to. `Ask::Raw` asks a query as text, for a query of another model
that the library does not know. `telnet_refresh_group` asks a whole group.

The query groups, in the order of `QueryGroup::DEFAULT_ORDER`. Ask for one group
again with `telnet_refresh_group`. The counts are the queries for a Denon and for
a Marantz receiver. A receiver from about 2016 lists its features in
`Deviceinfo.xml` (`Info::features`). The queries of a feature that it does not
list are not sent. Queries that the protocol specs of the receiver generation
do not have (`Info::spec_generation`) are not sent either. Queries of zone 2 or
zone 3 are not sent to a receiver whose device information says that it does
not have that zone (`Info::zone_count`).

| Group | Default | Values | Denon | Marantz |
|---|---|---|---|---|
| `Core` | yes | Power, source, volume and mute of the zones, sound mode, quick select | 9 | 9 |
| `Names` | yes | Source names and hidden sources, sound mode list, friendly name, Dirac Live slots and selected filter, quick select names | 7 | 7 |
| `Toggles` | yes | On and off settings, for example tone control and Dynamic EQ | 19 | 23 |
| `Signal` | yes | Video and audio signal | 14 | 14 |
| `Device` | no | Device settings, for example dimmer, eco, sleep, standby, video and picture settings, Bluetooth level | 22 | 22 |
| `Sound` | no | Sound settings and levels | 40 | 39 |
| `Zones` | no | Volume settings, levels, tone, high pass filter, channel mode of the zones, HDMI audio of zone 2 | 14 | 14 |
| `Picture` | no | Picture mode and adjustments, older receivers only | 6 | 6 |
| `Speakers` | no | Speaker configuration, crossovers and graphic EQ, Denon only | 3 | 0 |
| `Older` | no | Settings of receivers from about 2011 to 2015, for example Pro Logic mode, Audyssey DSX and InstaPrevue. Not in the command lists of receivers with HEOS, so not sent to them | 10 | 7 |

To decide what to show, ask `Info`:

- `offers(query)`, `offers_command(zone, &command)` and `offers_line(line)`:
  the receiver may have the value or accept the command. What the receiver
  does not tell counts as present.
- `supports(feature)`: a spec of the receiver generation has the feature.
- `readable_over_http(query)`: an HTTP read gives the value, so it is known
  without telnet.
- `http_reads(query)`: the `Avr` methods that read the value over HTTP, as
  `HttpReads` (`UPDATE`, `REFRESH`, `SIGNAL_AND_SETTINGS`, `SIGNAL_PAGES`).
  A caller that polls checks `intersects` with the methods it calls.
  `Avr::http_reads(query)` also leaves out what the receiver does not have,
  for example a page that answered 404. Ask it after the start.
  `http_reads_without(query, missing)` does the same with stored
  `MissingSources`.
- `has_quick_select(slot)`: the quick select may exist.

The web interface (port 11080) is read on request, because it does not
send changes:

```rust
avr.update_speaker_details().await?;
avr.update_source_details().await?;
avr.update_settings_availability().await?;
avr.update_detailed_availability().await?; // surround parameters, HDMI setup, video output
avr.update_signal_and_settings().await?; // signal, channels, Audyssey and HDMI output in one request
avr.update_video_details().await?; // on-screen display, TV format, HDCP, 4K/8K per input
avr.update_general_settings().await?; // language, firmware update, triggers
avr.update_system_settings().await?;
```

To read all values again, call `avr.refresh().await?`. If telnet is open it asks on
telnet and reads only the zone names over HTTP. If not, it reads everything over
HTTP. Your code does not choose.

To see the video and audio signal now, call `avr.refresh_signal().await?`. If
telnet is open, it asks only telnet. If not, it reads the resolution, the audio
signal and the channels with one fast request (`update_signal_and_settings`) if
the receiver has it, and the HDR and color values from the web interface. Your
code does not choose. Auto low latency mode and quick frame transport come only
from the web interface. To read these pages also while telnet is open, for
example after a page failed at the start, call `refresh_signal_pages`.

Some receivers do not send all signal changes on telnet. Two things fill the
gaps:

1. Hints. When the signal changes, some receivers send the sound mode (`MS`)
   or `SSINFAISSIG`, also with the same value, or a new audio sample rate
   (`SSINFAISFSV`). Telnet asks for the video signal and the channels after
   the first such line, after a random wait (`TelnetConfig::signal_wait_min`
   and `signal_wait_max`, 0.3 to 1.2 s). Answers to queries of this
   connection are not hints. A line with a known input resolution in the
   meantime cancels the video queries, for example the answer to another
   client's query: the receiver sends answers to every telnet client. The
   channels are still asked. When the answer has no input resolution yet
   (`---`), telnet asks for the video signal once more after 1 s.
2. Checks. Some changes come without any line, for example a PCM start
   without a sample rate, or SDR to Dolby Vision with the same audio. So on
   receivers with `CommApiVers` 0301 or higher, while the main zone is on,
   telnet asks for the input resolution, HDR, audio signal, sample rate,
   sound and channels every `TelnetConfig::signal_check` (5 s, `None` turns
   it off). There is no such check without telnet. It leaves out a
   query when its line came since the last check and this connection did not
   ask for it, for example because another client or the web interface asked.
   If the resolution or HDR changed, it asks for the whole video signal. If
   another client asked only for the resolution, the other video values can
   stay old. A query that was not answered three times in a row is left out
   until a line for it comes.

The signal fields keep the values as the receiver sends them. Two methods of
`SignalState` add what the receiver leaves out. `effective_pixel_depth()` gives
the input the output depth when the receiver sends `0` (unknown) for an input
signal, and no value for a side without a video signal, as the web interface
does. `video_color()` joins the color space and BT.2020, for example
`4:4:4/BT.2020`. Receivers report BT.2020 in different ways: in the color
space code (`SSINFSIGCOS` `9` is 4:2:2 with BT.2020), with `SSINFSIGECI` or in
the web interface. If a receiver reports it, BT.2020 is off after the
resolution, the HDR format or the color space changes, until the receiver says
it is on. Telnet asks `SSINFSIGECI ?` after the video queries, and once more
after such a change. If that query cannot be sent, for example while telnet
connects again, BT.2020 stays off until the next change or `refresh_signal()`.
If a receiver does not report it, an HDR format means BT.2020.

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

## Without telnet

The setters work over HTTP too. `avr.send(..)` and the controls use telnet while
it is open, and HTTP otherwise, with `GET /goform/formiPhoneAppDirect.xml`.
A program that does not want telnet never opens it: use
`avr.start_with(StartOptions::new().telnet(false))`. Over HTTP:

- The receiver answers `200` whether or not it did the command, as telnet
  gives no answer either. At a high rate, for example a held key, a command can
  be lost without an error.
- The receiver does not answer a command, as on telnet, and it sends no events.
  Queries (`PW?`) need telnet, and give `Error::QueryNeedsTelnet` whenever
  telnet is not connected, also while it connects again.
- After a command, the values that it changed are read, for example the volume
  and mute after a volume command, tone control after a bass command on the
  main zone, the Audyssey settings after an Audyssey command (not while telnet
  is open, which reports them), the band and frequency after an analog tuner
  command while the tuner plays, and the settings that the start reads over
  `AppCommand0300.xml` (not while telnet is open): the part with the setting
  after a command for one of them, the fast ones after a sound mode command,
  and all after a source command or a quick select of the main zone. The read
  waits until no command came for 500 ms, and not longer than 2 s, so a held
  key causes few reads. The receiver shows a new power state, source or sound
  mode late, so these are read again every 1.5 s until a read shows a change,
  at most 3 times. A read again after a sound mode command or a source command
  of the main zone also reads the fast ones of these settings. Change this with
  `AvrBuilder::follow_up_read`, or turn it off with `None`.
- Commands go before the reads that wait, also those of the web interface.
  A read waits while a command waits or runs, and for 250 ms after it, so
  the receiver does the command first. A read that has started is not
  stopped. The read after a command does not wait for the pause. Change the
  pause with `AvrBuilder::background_pause`. It is the same pause that telnet
  queries wait after a telnet command.
- Settings that no read after a command covers change in the state when you
  read them, for example with `update_signal_and_settings()`.
- The list of sound modes of the receiver needs telnet. Without it, select a
  mode by its name, for example `select_sound_mode("MOVIE")`.

## HEOS

The `denon-marantz-heos` crate is a client for the HEOS CLI on TCP port 1255.
It keeps the state of each player: what plays, the play state, the position
and the repeat and shuffle modes. You read the state with `Heos::players` and
watch it with `Heos::changes`. It also:

- Controls the playback: play, pause, stop, next, previous, repeat and
  shuffle.
- Reads the queue, plays an item of it, removes items and clears it.
- Reads and plays HEOS Favorites and quick selects.
- Plays a stream from a URL, and an input of the device.

The client does not keep or control the volume and mute. On the AVC-A10H, the
HEOS volume is shared by the zones. Use the `denon-marantz` crate for them. For
other HEOS devices, `denon-marantz-heos-proto` has the volume and mute commands
of players and groups, and `Heos::send` sends them.

```rust
use denon_marantz_heos::Heos;
use tokio::sync::broadcast::error::RecvError;

let heos = Heos::connect("192.168.1.20").await?; // the state is read when this returns
// `None` for some time after the device wakes up, see the notes below.
let pid = heos.player_by_ip("192.168.1.20".parse()?).ok_or("no player")?;
let mut changes = heos.changes();
heos.play_preset(pid, 1).await?; // the first HEOS Favorite

loop {
    match changes.recv().await {
        Ok(_) | Err(RecvError::Lagged(_)) => {
            // Reads the state without a copy.
            heos.with_player(pid, |state| {
                println!("{:?}: {:?}", state.play_state, state.current_media());
            });
        }
        Err(RecvError::Closed) => break,
    }
}
```

The client keeps two connections: one for commands and one for events. It
sends one command at a time. A command whose future is dropped before it is
sent is not sent. It connects again after a drop, waits longer after each
failed try, and then reads the state again. If the device refuses to send
change events, it connects again too, and waits longer after each refusal in a
row. A refusal during `Heos::connect` can make the connect fail. `HeosConfig`
has the times. The `denon-marantz-heos-proto` crate has the protocol without
IO.

With the `heos` feature, `Avr` opens HEOS too. The start opens it in the
background on receivers with HEOS (`Info::has_heos`), and neither the start
nor its waits wait for it. If the first connect fails, the status is
`HeosPlayerStatus::Failed` with the reason, and `Avr::heos_start_error` has the
error. `StartOptions::heos` turns this off, and `Avr::heos_connect` opens it
later. `Avr::watch_heos_status` says when the player of the receiver is found
by its address or its serial number, and `Avr::heos` then gives it for
playback. `HeosPlayer::with_state` reads its state without a copy.
`Avr::heos_changes` gives the changes of the players, also after HEOS was
closed and opened again.
Volume and mute stay on telnet and HTTP. The `denon_marantz::heos` module is
the `denon-marantz-heos` crate.

Notes about HEOS:

- The AVC-A10H has one player for all its zones. Its HEOS volume events come
  from the main zone and from zone 2, and it sends no HEOS event for mute.
- A stopped player keeps its last item, also after the receiver changed to a
  source that is not HEOS. `PlayerState::current_media` hides it.
- The device can report the play state as not known. The state is then
  `None`, and the item can be old.
- Pausing a station that cannot pause stops it. `PlayerState::controls` says
  which controls work for the item.
- The device sends no event when the HEOS Favorites change. Read them again
  with `Heos::favorites` when they are needed.
- `AirPlay` shows as an item from local music, with no mark that it is
  `AirPlay`. In the first seconds after it starts, the item has no text and
  the position can jump.
- `Heos::play_stream` sends the URL as it is, and the device loads it. The
  device must be able to reach the URL.
- Reading the HEOS Favorites can take longer than other commands. The device
  first answers that it works on the command (`HeosConfig::slow_command_limit`).
- Text in the payload of answers, such as song names, is used as the device
  sends it. The AVC-A10H sends `&` as it is, not as `%26` as the HEOS CLI
  specification says. Values in the `message` part are decoded.
- After the device wakes up, it can take some time to find its players. A
  player that is not known yet comes later with `Change::Players`. With `Avr`,
  the status is `HeosPlayerStatus::WaitingForPlayer` until then.

## Features of the `denon-marantz` crate

| Feature | Default | Adds |
|---|---|---|
| `telnet` | yes | Live events and commands on port 23. Needs `http`. |
| `http` | yes | AppCommand XML, the web interface and commands over HTTP. |
| `discovery` | yes | Receiver search with SSDP (`discover`, `discover_until`). Needs `http`. |
| `mdns` | no | Receiver search with mDNS too (`discover_mdns`, `discover_all`, `discover_all_until`). Starts a thread for each search. Needs `discovery`. |
| `heos` | no | The HEOS player of the receiver for playback (`Avr::heos`). Needs `http`. |
| `sensitive-logs` | no | Full messages in trace logs. They can have personal data. |
| `serde` | no | `Serialize` and `Deserialize` for `Info` and `MissingSources`, to store them for `AvrBuilder::known` and `AvrBuilder::missing`. |

The `denon-marantz-heos` crate has one feature, `sensitive-logs` (off by
default): full lines in trace logs. They can have personal data.

## Tested

All crates were tested on a Denon AVC-A10H.

## Not tested

These parts follow the protocol specifications and the other tested parts, but
have not run on a receiver:

- Marantz receivers. The commands and queries follow the Marantz protocol
  sheets and the Denon ones, but no Marantz receiver was available.
- Receivers without `AppCommand`, which are read over the status pages. They
  are tested with captured pages and fake receivers.
- Tone control and the other `AppCommand0300.xml` reads (not the settings) on
  receivers older than the 2016 models. A receiver from about 2015 lists in `Deviceinfo.xml` which of these
  queries it answers, and only those are sent. Older receivers do not list
  them, so they are asked as on newer models. If a request with tone control
  fails on such a receiver and works without it, tone control is not asked
  again.
- The settings over `AppCommand0300.xml` on receivers older than the 2016
  models. These list version 1 of `GetSurroundParameter`, `GetHdmiSetup` and
  `GetAudioDelay`. Their other settings queries may answer differently, so no
  settings are read. A receiver counts as new enough when `Deviceinfo.xml`
  lists version 2 of `GetSurroundParameter`.
- Center image, center width, channel expander, dimension and the IMAX
  filters are not read over `AppCommand0300.xml`: no tested receiver gave a
  value for them, so the form of the value is not known. For most other
  settings read over `AppCommand0300.xml`, the lists and ranges come from
  `Deviceinfo.xml`, and no tested receiver gave a value other than 0 or none,
  for example for dialog control, Auro-3D, IMAX, the effect level, the room
  size and panorama. The LFE level is read only when the same answer gives an
  input mode other than the 7.1CH input, which has its own LFE level.
- The sound mode category query `SSSMG ?` on other receivers than the
  AVC-A10H. It is not sent to receivers whose `Deviceinfo.xml` lists the LFE
  level but not the sound mode genres.
- The tuner status page while the tuner plays. All captured pages are from
  other sources and have no band or frequency.
- Receivers that are older than the AVC-A10H. The effect speaker, center image
  and center gain texts (`PSEFF`, `PSCEI`, `PSCEG`) were confirmed by other
  users, not on a receiver here.
- What plays (`refresh_now_playing`, `update_now_playing`,
  `telnet_refresh_now_playing`, the `NSE` and `HD` events). The AVC-A10H does
  not give it, so it is tested with captured pages from other receivers and
  with fake receivers. The `NSA` text is not used.
- The signal pages of the web interface with an FRL rate, and colorimetry
  texts other than `BT.2020`: the FRL rate is only seen as no signal. The values
  of `ALLM` and `QFT` (1 is on) were only seen as off.
- The query `MNZST?` and the `MNZST` event of all zone stereo on other receivers.
- The tactile transducer channel (`CVTTR`) with a connected transducer. The
  AVC-A10H has none, and it ignores `CVTTR` while the transducer is off.
- Bass and treble of zone 2 and zone 3 from -14 to +14 dB in steps of 2 dB.
  The AVC-A10H has -10 to +10 dB in steps of 1 dB.
- The LFE level of a 7.1 channel input (`PSLFL`). The AVC-A10H answers
  `PSLFL ?` only with a 7.1 input, and the setter was not sent to it.
- Favorite 4 (`ZMFAVORITE4`) and the lowest volume as `99` and
  `995`, which receivers from before about 2013 send.
- Balance (`BL`), the DAB tuner (`DA`) and `TPANOFF`. They follow the protocol
  specifications and are tested with unit tests and fake receivers only.
- The features and value ranges in `Deviceinfo.xml` of receivers other than
  the AVC-A10H. They are tested with captured files only.
- The reset of the channel levels with `CVZRL`.
- Sound settings that the receiver ignores in some modes, for example delay
  time, dialog control, Auro-Matic strength and effect level. The receiver
  sends no answer or error, so the state shows no change.
- The settings of older receivers: Pro Logic mode (`PSMODE`), surround back
  mode (`PSSB`), dynamic compression (`PSDCO`), auto flag detect (`PSAFD`),
  Dialog Level Adjust (`PSDIL`), Audyssey DSX (`PSDSX`, `PSSTW`, `PSSTH`), the
  Pro Logic IIz height gain (`PSPHG`), front height output (`PSFH`) and
  InstaPrevue (`MNPRV`). They follow the Denon protocol specifications of 2011
  to 2015 and the Marantz one of 2013, and are tested with unit tests only.
  The AVC-A10H does not answer `PSFH: ?`.
- Tuner presets in the form A1 to G8, and the 36 network presets of 2014 and
  2015 Denon receivers. They follow the protocol specifications.
- Dirac filters other than Dirac ART. Only Dirac ART was tested for locking the
  channel levels and the crossovers.
- The value `WhenOn` of the network control setting: the network answers only
  while the receiver is on. Only `Always` was seen. A receiver needs some time
  after power on before the network answers.
- The XLR output assignment (`XlrAssign`) on a model that has XLR outputs. The
  AVC-A10H has none and answers the page with zeros.
- Receivers without the web interface on port 11080. The library waits up to
  one second to connect for each page call, see
  `AvrBuilder::web_interface_config`.
- HEOS: other HEOS devices, for example speakers, groups of players, and
  playing an input of another player. The AVC-A10H has one player and no
  groups.
- HEOS quick selects: the receiver accepts the call, but the effect was not
  checked.
- HEOS volume and mute commands of players and groups in
  `denon-marantz-heos-proto`. They follow the HEOS CLI specification and are
  tested with unit tests only.
- HEOS: connecting again after a lost connection, and the heartbeat. They are
  tested with a fake device.
- TCP keepalive on the telnet and HEOS connections. A test checks that the
  options are set. Finding a receiver that loses power was not tested.

## Notes about receivers

- Some changes made in the receiver menu or with its remote are pushed on
  telnet, some are not. Changes from the web interface and the apps are.
- Telnet answers in standby when network control is `Always`.
- The protocol specification says to wait 1 second after `PWON` before the next
  command (4 seconds for older models). A receiver that wakes up ignores
  commands that come too soon. After a power-on command while the state shows
  no zone on, the library holds the next telnet line for
  `TelnetConfig::power_on_hold` (0.5 seconds by default). A slower model may
  need a longer time.
- `PWON` (the whole receiver) also switches on zone 2 and zone 3, and `ZMON` only
  the main zone. When the receiver comes on, it sets the volume to its power-on
  level, so the volume after a power on is not the one before.
- The receiver can ignore a source command that comes a few seconds after
  another source command, on telnet and over HTTP. This was seen with an input
  that switches a trigger output (to power on a device), and not with an input
  that does nothing else.
- The receiver shows a new power state, source or sound mode in its `AppCommand`
  answers up to a few seconds late.
- A network restart of the receiver drops telnet after some time, and the
  receiver then accepts and resets the connection for a few seconds. The
  library tries again every `TelnetConfig::reconnect_min` (0.5 seconds by
  default) until it stays connected.
- A Dirac ART filter locks the channel levels, the dial page (HTTP status 500)
  and the crossovers.
- The library only reads from the web interface. It never writes to it.

## Crates

| Crate | Role |
|---|---|
| `denon-marantz` | Async client (tokio). |
| `denon-marantz-proto` | Protocol without IO. |
| `denon-marantz-discovery` | SSDP messages and mDNS service records, without IO. |
| `denon-marantz-heos` | Async HEOS client (tokio). |
| `denon-marantz-heos-proto` | HEOS protocol without IO. |
| `denon-marantz-shared` | Internal helpers of `denon-marantz` and `denon-marantz-heos`. No stable API. |

The `denon-marantz` crate re-exports the types that most programs need, for
example `Zone` and `Volume`. All others are in `denon_marantz::proto`.
The `denon-marantz-heos` crate does the same with `denon_marantz_heos::proto`.

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) shows how the crates and the
connections fit together.

Minimum supported Rust version: 1.100.

## Examples

Run them from the repository root. Replace the address with the one of your
receiver.

```sh
cargo run -p denon-marantz --example discover
cargo run -p denon-marantz --example status -- 192.168.1.20
cargo run -p denon-marantz --example watch -- 192.168.1.20
cargo run -p denon-marantz --example start -- 192.168.1.20
cargo run -p denon-marantz --example remote -- 192.168.1.20 volume -35
cargo run -p denon-marantz-heos --example heos_watch -- 192.168.1.20
cargo run -p denon-marantz-heos --example heos_remote -- 192.168.1.20
```

`http_check` tests commands over HTTP on a real receiver. It changes settings and
can switch power, so read the header of the file before you run it.

`status` prints the serial number of the receiver, and `discover` prints the
serial number and the MAC address of each receiver. Remove them before you
share the output. `start` shows how long `Avr::start` takes and what it read.
`remote` runs one command, the header of the file lists them.

`heos_watch` prints what changes. `heos_remote` reads commands from
the standard input, one per line, the header of the file lists them. Their
output can have personal data, for example a title that plays.

## Logging

The crates use `tracing`. Set `RUST_LOG=denon_marantz=debug` to see the
connect steps and the pages that could not be read. `trace` shows each
message. The `sensitive-logs` feature adds full message text.
For HEOS, use `RUST_LOG=denon_marantz_heos=debug`.

Logs of one `Avr` are in an `avr` span with the host and an `id` that is
unique among the `avr` spans of the process: the connect, its HTTP requests,
the telnet task, the HEOS connection, the start, the check of stored values,
the reads after commands and the reads of `update`, `refresh` and the
`update_*` and `refresh_*` methods. A HEOS client logs in a `heos` span with
the same fields. The span of a HEOS client that an `Avr` opens is in the `avr`
span, if logs of `denon_marantz` are on. The spans have the error level and the
crate name as target, so a filter for the whole crate turns them on at every
level. A filter for one module only, for example `denon_marantz::telnet`, does
not. Free functions such as `discover` and `identify` log without a span when
user code calls them.

## Development

```sh
cargo fmt --all --check
RUSTFLAGS="-D warnings" cargo clippy --locked --workspace --all-targets --all-features
cargo test --locked --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps --all-features
cargo deny check
cargo audit
for f in "" telnet http discovery mdns telnet,http serde http,serde heos telnet,heos; do
  RUSTFLAGS="-D warnings" cargo clippy --locked -p denon-marantz --no-default-features --features "$f" --all-targets
  cargo check --locked -p denon-marantz --no-default-features --features "$f"
done
cargo fmt --manifest-path fuzz/Cargo.toml --check
RUSTFLAGS="-D warnings" cargo clippy --locked --manifest-path fuzz/Cargo.toml --all-targets
```

Fuzz targets for the parsers are in `fuzz/`. Benchmarks use `criterion`.

## Credits

This library is not a port, but parts of it are based on
[denonavr](https://github.com/ol-iver/denonavr) (MIT), a Python library by
Oliver Götz and contributors. Some of the logic for how the receivers work
comes from that project, for example how `AppCommand` requests are built.
The HEOS crates follow the HEOS CLI protocol specification.

This project is not made or supported by Denon or Marantz. Denon and Marantz
are trademarks of their respective owners. Other names are the property of
their respective owners.

## License

MIT ([LICENSE](LICENSE)).
