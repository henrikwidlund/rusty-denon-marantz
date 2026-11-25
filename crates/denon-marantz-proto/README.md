# denon-marantz-proto

Protocol for Denon and Marantz AV receivers.

- Telnet: builds commands and parses the lines that the receiver sends.
- AppCommand XML and the pages of the web interface: builds requests and
  parses answers.
- State: keeps the receiver state from parsed lines and answers.

The crate does no I/O.

Most users want the [`denon-marantz`](https://crates.io/crates/denon-marantz)
crate, which runs this crate with tokio. It re-exports this crate as
`denon_marantz::proto`.

Minimum supported Rust version: 1.100.

## License

MIT.
