# denon-marantz-discovery

Helpers to find Denon and Marantz AV receivers with SSDP and mDNS.

- SSDP: builds the requests and reads the answers.
- mDNS: names the service, reads the text records of an answer and chooses
  the address to use.

The crate has no sockets. The caller sends and receives the datagrams.

Most users want the [`denon-marantz`](https://crates.io/crates/denon-marantz)
crate, which uses this crate to find receivers.

Minimum supported Rust version: 1.100.

## License

MIT.
