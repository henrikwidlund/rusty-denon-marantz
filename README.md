# denon-marantz

Async Rust library to monitor and control Denon and Marantz AV receivers.

> Work in progress. Nothing is published and the API will change.

## Planned support

- Telnet (port 23): live events and commands.
- HTTP: AppCommand XML for status and settings.
- Zones: main, zone 2, zone 3.
- Power, volume, mute, input, sound mode, tone control.
- Audyssey, Dirac and receiver settings.
- Discovery with SSDP and mDNS.

## Crates

| Crate | Role |
|---|---|
| `denon-marantz` | Async client (tokio). |
| `denon-marantz-proto` | Protocol without IO. |
| `denon-marantz-discovery` | SSDP and mDNS parsing without IO. |

Minimum supported Rust version: 1.88.

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo deny check
```

## Credits

This is a Rust port of [denonavr](https://github.com/henrikwidlund/denonavr)
(MIT), a Python library by Oliver Götz and contributors. The protocol
knowledge and command tables come from that project.

This project is not made or supported by Denon or Marantz. Denon and Marantz
are trademarks of their respective owners. Other names are the property of
their respective owners.

## License

MIT ([LICENSE](LICENSE)).
