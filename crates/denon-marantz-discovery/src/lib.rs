//! Sans-IO SSDP and mDNS parsing to find Denon and Marantz AV receivers.
//!
//! The crate has no sockets. It builds the requests and reads the answers. The
//! caller sends and receives the datagrams.

pub mod ssdp;
