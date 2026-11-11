//! Sans-IO SSDP and mDNS helpers to find Denon and Marantz AV receivers.
//!
//! The crate has no sockets. For SSDP it builds the requests and reads the
//! answers. For mDNS it names the service and reads what the text records of
//! an answer tell. The caller sends and receives the datagrams.

pub mod mdns;
pub mod ssdp;
