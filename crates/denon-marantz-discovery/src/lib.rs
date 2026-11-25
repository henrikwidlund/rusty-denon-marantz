//! Sans-IO SSDP and mDNS helpers to find Denon and Marantz AV receivers.
//!
//! The crate has no sockets. For SSDP it builds the requests and reads the
//! answers. For mDNS it names the service, reads what the text records of an
//! answer tell and chooses the address to use. The caller sends and receives
//! the datagrams.

pub mod mdns;
pub mod ssdp;
