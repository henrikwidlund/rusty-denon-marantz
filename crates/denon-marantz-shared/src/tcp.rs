//! TCP connections and their socket options.

use std::io;
use std::time::Duration;

use socket2::{SockRef, TcpKeepalive};
use tokio::net::TcpStream;

/// Why a connection could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// The connection was not open within the time limit.
    #[error("connect timed out")]
    Timeout,
    /// Opening the connection failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Opens a TCP connection to `host`. Turns off Nagle's algorithm and turns on
/// keepalive.
pub async fn connect(host: &str, port: u16, timeout: Duration) -> Result<TcpStream, ConnectError> {
    let stream = tokio::time::timeout(timeout, TcpStream::connect((host, port)))
        .await
        .map_err(|_| ConnectError::Timeout)??;
    configure(&stream);
    Ok(stream)
}

/// Idle time before the first keepalive probe.
const KEEPALIVE_TIME: Duration = Duration::from_secs(10);
/// Time that sent data can wait for an acknowledgement before the connection
/// fails. Keepalive probes do not run while sent data waits.
#[cfg(any(target_os = "linux", target_os = "android"))]
const USER_TIMEOUT: Duration = Duration::from_secs(25);

/// Turns off Nagle's algorithm and turns on keepalive. On most systems,
/// keepalive finds a device that is gone without closing the connection, for
/// example after a power loss, in about 25 s. Options that the system refuses
/// are skipped.
fn configure(stream: &TcpStream) {
    // Commands are short. Do not wait to fill a packet.
    let _ = stream.set_nodelay(true);
    let socket = SockRef::from(stream);
    let _ = socket.set_tcp_keepalive(&with_probes(TcpKeepalive::new().with_time(KEEPALIVE_TIME)));
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let _ = socket.set_tcp_user_timeout(Some(USER_TIMEOUT));
}

/// Sends a probe every 5 s after the idle time, and fails after 3 probes
/// without an answer.
#[cfg(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "fuchsia",
    target_os = "illumos",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "windows",
))]
const fn with_probes(keepalive: TcpKeepalive) -> TcpKeepalive {
    keepalive
        .with_interval(Duration::from_secs(5))
        .with_retries(3)
}

/// Keeps the probe values of the system. This system has no options for them.
#[cfg(not(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "fuchsia",
    target_os = "illumos",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "netbsd",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
    target_os = "windows",
)))]
const fn with_probes(keepalive: TcpKeepalive) -> TcpKeepalive {
    keepalive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_sets_options_and_gives_the_io_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let stream = connect("127.0.0.1", port, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(stream.nodelay().unwrap());
        // Port 0 cannot be connected to.
        let error = connect("127.0.0.1", 0, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(error, ConnectError::Io(_)), "{error:?}");
    }

    #[tokio::test]
    async fn sets_options() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        configure(&stream);
        let socket = SockRef::from(&stream);
        assert!(stream.nodelay().unwrap());
        assert!(socket.keepalive().unwrap());
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        {
            assert_eq!(socket.tcp_keepalive_time().unwrap(), KEEPALIVE_TIME);
            assert_eq!(
                socket.tcp_keepalive_interval().unwrap(),
                Duration::from_secs(5)
            );
            assert_eq!(socket.tcp_keepalive_retries().unwrap(), 3);
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        assert_eq!(socket.tcp_user_timeout().unwrap(), Some(USER_TIMEOUT));
    }
}
