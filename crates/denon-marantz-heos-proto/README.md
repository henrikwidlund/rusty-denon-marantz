# denon-marantz-heos-proto

Protocol for the HEOS CLI of Denon and Marantz products.

- Commands are text lines. The crate builds them.
- Answers and events are JSON lines. The crate parses them.

The crate does no I/O.

Most users want the
[`denon-marantz-heos`](https://crates.io/crates/denon-marantz-heos) crate,
which runs this crate with tokio. It re-exports this crate as
`denon_marantz_heos::proto`.

The [crate documentation](https://docs.rs/denon-marantz-heos-proto) has an
example.

Minimum supported Rust version: 1.100.

## License

MIT.
