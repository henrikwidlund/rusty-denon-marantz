//! The HTTP client against a fake receiver.

#![cfg(feature = "http")]

mod common;

use std::time::{Duration, Instant};

use bytes::Bytes;
use common::{Fake, Reply, config};
use denon_marantz::{Error, HttpClient, HttpConfig, Pacing};

fn client(fake: &Fake, config: HttpConfig) -> HttpClient {
    HttpClient::new("127.0.0.1", fake.port, config).unwrap()
}

#[tokio::test]
async fn get_returns_the_body() {
    let fake = Fake::start(|_| Reply::Body(b"<rx/>".to_vec())).await;
    let body = client(&fake, config())
        .get("/goform/Deviceinfo.xml")
        .await
        .unwrap();
    assert_eq!(&body[..], b"<rx/>");
    let seen = fake.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (seen[0].method.as_str(), seen[0].path.as_str()),
        ("GET", "/goform/Deviceinfo.xml")
    );
}

#[tokio::test]
async fn post_sends_the_body() {
    let fake = Fake::start(|seen| Reply::Body(seen.body.clone())).await;
    let sent = Bytes::from_static(b"<tx><cmd id=\"1\">GetECO</cmd></tx>");
    let body = client(&fake, config())
        .post("/goform/AppCommand.xml", sent.clone())
        .await
        .unwrap();
    assert_eq!(body, sent);
    let seen = fake.requests();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].body, sent.to_vec());
}

#[tokio::test]
async fn query_in_the_path_is_sent() {
    let fake = Fake::start(|_| Reply::Body(Vec::new())).await;
    client(&fake, config())
        .get("/goform/formMainZone_MainZoneXml.xml?ZoneName=ZONE2")
        .await
        .unwrap();
    assert_eq!(
        fake.requests()[0].path,
        "/goform/formMainZone_MainZoneXml.xml?ZoneName=ZONE2"
    );
}

#[tokio::test]
async fn connections_are_reused() {
    let fake = Fake::start(|_| Reply::Body(b"x".to_vec())).await;
    let client = client(&fake, config());
    for _ in 0..4 {
        client.get("/a").await.unwrap();
    }
    assert_eq!(fake.requests().len(), 4);
    assert_eq!(fake.connection_count(), 1);
}

#[tokio::test]
async fn error_status_is_an_error() {
    let fake = Fake::start(|seen| match seen.path.as_str() {
        "/forbidden" => Reply::Status(403),
        "/redirect" => Reply::Status(302),
        _ => Reply::Status(404),
    })
    .await;
    let client = client(&fake, config());
    let error = client.get("/missing").await.unwrap_err();
    assert!(
        matches!(error, Error::Status { status: 404, .. }),
        "{error:?}"
    );
    assert!(error.is_status() && !error.is_forbidden());
    assert!(
        error.to_string().contains("GET http://127.0.0.1:"),
        "{error}"
    );
    assert!(error.to_string().contains("/missing"), "{error}");

    let error = client.get("/forbidden").await.unwrap_err();
    assert!(error.is_forbidden() && error.is_status(), "{error:?}");

    // Redirects are not followed.
    let error = client.get("/redirect").await.unwrap_err();
    assert!(
        matches!(error, Error::Status { status: 302, .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn silence_is_a_timeout() {
    let fake = Fake::start(|_| Reply::Hang).await;
    let started = Instant::now();
    let error = client(&fake, config()).get("/slow").await.unwrap_err();
    assert!(error.is_timeout(), "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(error.request().unwrap().contains("/slow"));
}

#[tokio::test]
async fn closed_connection_is_an_invalid_response() {
    let fake = Fake::start(|_| Reply::Close).await;
    let error = client(&fake, config()).get("/x").await.unwrap_err();
    assert!(error.is_invalid_response(), "{error:?}");
}

#[tokio::test]
async fn garbage_is_an_invalid_response() {
    let fake = Fake::start(|_| Reply::Raw(b"this is not http\r\n\r\n".to_vec())).await;
    let error = client(&fake, config()).get("/x").await.unwrap_err();
    assert!(error.is_invalid_response(), "{error:?}");
}

#[tokio::test]
async fn a_body_that_ends_early_is_a_network_error() {
    // The head promises 100 bytes, and the connection ends after 5.
    let fake = Fake::start(|_| {
        Reply::Raw(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort".to_vec())
    })
    .await;
    let error = client(&fake, config()).get("/x").await.unwrap_err();
    assert!(error.is_network(), "{error:?}");
    assert!(error.is_unreachable());
}

#[tokio::test]
async fn a_forbidden_page_is_its_own_error() {
    let fake = Fake::start(|_| Reply::Status(403)).await;
    let error = client(&fake, config()).get("/x").await.unwrap_err();
    assert!(matches!(error, Error::Forbidden { .. }), "{error:?}");
    assert!(error.is_status());
    assert!(!error.is_unreachable());
}

#[tokio::test]
async fn refused_connection_is_a_network_error() {
    let fake = Fake::start(|_| Reply::Body(Vec::new())).await;
    let port = fake.port;
    drop(fake);
    // The listener task keeps running until the runtime ends, so use a port
    // that nothing listens on.
    let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let free_port = unused.local_addr().unwrap().port();
    drop(unused);
    assert_ne!(port, free_port);
    let client = HttpClient::new("127.0.0.1", free_port, config()).unwrap();
    let error = client.get("/x").await.unwrap_err();
    // Windows reports a refused connection after about two seconds, so the
    // short timeout of the test can come first.
    assert!(
        error.is_network() || (cfg!(windows) && error.is_timeout()),
        "{error:?}"
    );
}

#[tokio::test]
async fn large_answers_are_refused() {
    let fake = Fake::start(|_| Reply::Body(vec![b'a'; 2000])).await;
    let small = HttpConfig {
        max_response_bytes: 1000,
        ..config()
    };
    let error = client(&fake, small).get("/big").await.unwrap_err();
    assert!(
        matches!(error, Error::ResponseTooLarge { limit: 1000, .. }),
        "{error:?}"
    );
    assert!(error.is_invalid_response());

    let ok = HttpConfig {
        max_response_bytes: 2000,
        ..config()
    };
    assert_eq!(client(&fake, ok).get("/big").await.unwrap().len(), 2000);
}

#[tokio::test]
async fn requests_are_spaced_by_the_pacer() {
    let fake = Fake::start(|_| Reply::Body(Vec::new())).await;
    let paced = HttpConfig {
        pacing: Pacing::Fixed {
            interval: Duration::from_millis(100),
        },
        ..config()
    };
    let client = client(&fake, paced);
    let started = Instant::now();
    for _ in 0..4 {
        client.get("/x").await.unwrap();
    }
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn invalid_host_and_path_are_errors() {
    assert!(matches!(
        HttpClient::new("bad host", 80, config()),
        Err(Error::InvalidRequest(_))
    ));
    let fake = Fake::start(|_| Reply::Body(Vec::new())).await;
    let error = client(&fake, config())
        .get("no slash \n")
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidRequest(_)), "{error:?}");
}

#[tokio::test]
async fn ipv6_hosts_are_accepted() {
    let client = HttpClient::new("::1", 8080, config()).unwrap();
    assert_eq!(client.authority(), "[::1]:8080");
    let client = HttpClient::new("[::1]", 80, config()).unwrap();
    assert_eq!(client.authority(), "[::1]:80");
    assert_eq!(
        HttpClient::new("denon.local", 80, config())
            .unwrap()
            .authority(),
        "denon.local:80"
    );
}

#[tokio::test]
async fn parsed_answers() {
    use denon_marantz_proto::DeviceInfo;

    let fake = Fake::start(|seen| match seen.path.as_str() {
        "/html" => Reply::Body(b"<html><body>Form is not defined</body></html>".to_vec()),
        _ => Reply::Body(b"<Device_Info><ModelName>AVR-X1</ModelName></Device_Info>".to_vec()),
    })
    .await;
    let client = client(&fake, config());
    let parse = |body: &[u8]| {
        DeviceInfo::parse(body).map(|info| info.model_name.map(std::borrow::Cow::into_owned))
    };

    assert_eq!(
        client.get_parsed("/info", parse).await.unwrap().as_deref(),
        Some("AVR-X1")
    );

    let error = client.get_parsed("/html", parse).await.unwrap_err();
    assert!(matches!(error, Error::Protocol { .. }), "{error:?}");
    assert!(error.is_invalid_response());
    assert!(
        error.to_string().contains("GET http://127.0.0.1:"),
        "{error}"
    );
    assert!(error.to_string().contains("/html"), "{error}");

    // Errors of the request come before the parser.
    let closed = Fake::start(|_| Reply::Status(404)).await;
    let error = self::client(&closed, config())
        .get_parsed("/info", parse)
        .await
        .unwrap_err();
    assert!(error.is_status(), "{error:?}");
}

#[tokio::test]
async fn parsed_post_answers() {
    let fake = Fake::start(|seen| Reply::Body(seen.body.clone())).await;
    let length = client(&fake, config())
        .post_parsed(
            "/goform/AppCommand.xml",
            Bytes::from_static(b"<tx/>"),
            |body| Ok(body.len()),
        )
        .await
        .unwrap();
    assert_eq!(length, 5);
    let error = client(&fake, config())
        .post_parsed(
            "/goform/AppCommand.xml",
            Bytes::from_static(b"<tx"),
            |body| denon_marantz_proto::appcommand::Response::parse(body, 1).map(|_| ()),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Protocol { .. }), "{error:?}");
    assert!(error.to_string().contains("POST http://"), "{error}");
}
