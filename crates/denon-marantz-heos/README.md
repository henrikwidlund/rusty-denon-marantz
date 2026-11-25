# denon-marantz-heos

Async client (tokio) for the HEOS CLI of Denon and Marantz products.

- Reads what plays, the play state, the position, and the repeat and shuffle
  modes.
- Controls playback.
- Reads and plays the queue, HEOS Favorites and quick selects.
- Plays streams and inputs.

The client does not keep or control the volume and mute. For a receiver, use
telnet or HTTP for them. For other HEOS devices, `Heos::send` can send the
volume and mute commands of the protocol crate. The protocol is in the
[`denon-marantz-heos-proto`](https://crates.io/crates/denon-marantz-heos-proto)
crate, also as `denon_marantz_heos::proto`.

You can use this crate alone for HEOS players. The
[`denon-marantz`](https://crates.io/crates/denon-marantz) crate re-exports it
as `denon_marantz::heos` (feature `heos`).

Feature `sensitive-logs`: full lines in trace logs. They can contain personal
data.

Minimum supported Rust version: 1.100.

## License

MIT.
