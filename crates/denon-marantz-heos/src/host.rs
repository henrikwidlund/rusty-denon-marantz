//! Host names and IPv6 addresses.

use std::fmt;

/// Removes the brackets around an IPv6 address: `[::1]` becomes `::1`.
pub(crate) fn bare(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
}

/// Like [`bare`], for an owned host.
pub(crate) fn into_bare(host: String) -> String {
    match bare(&host) {
        inner if inner.len() == host.len() => host,
        inner => inner.to_owned(),
    }
}

/// Shows a host for a URL or a `host:port` text. An IPv6 address gets
/// brackets. The host must be without brackets, see [`into_bare`].
pub(crate) struct UrlHost<'a>(pub(crate) &'a str);

impl fmt::Display for UrlHost<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.contains(':') {
            write!(f, "[{}]", self.0)
        } else {
            f.write_str(self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brackets_are_removed_only_around_the_host() {
        assert_eq!(into_bare("[::1]".to_owned()), "::1");
        assert_eq!(into_bare("::1".to_owned()), "::1");
        assert_eq!(into_bare("192.168.1.10".to_owned()), "192.168.1.10");
        assert_eq!(into_bare("[".to_owned()), "[");
        assert_eq!(into_bare("[]".to_owned()), "");
        assert_eq!(bare("[fe80::1]"), "fe80::1");
    }

    #[test]
    fn only_ipv6_addresses_get_brackets() {
        assert_eq!(UrlHost("fe80::1").to_string(), "[fe80::1]");
        assert_eq!(UrlHost("192.168.1.10").to_string(), "192.168.1.10");
        assert_eq!(UrlHost("receiver.local").to_string(), "receiver.local");
    }
}
