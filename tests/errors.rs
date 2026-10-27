use cli_command_kit::Mode;
use remarkable_cli::{
    error::{Error, after_commit},
    output::Output,
    runtime,
};
use remarkable_cloud::{Error as CloudError, ErrorKind, UpdateState};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

#[tokio::test]
async fn native_dns_connection_and_body_failures_are_classified_without_message_matching() {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let dns = client.get("http://bad..invalid/").send().await.unwrap_err();
    assert!(dns.is_dns(), "{dns:?}");
    assert_eq!(Error::Cloud(CloudError::Http(dns)).exit_status(), 5);
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let refused = client
        .get(format!("http://{address}/"))
        .send()
        .await
        .unwrap_err();
    assert_eq!(Error::Cloud(CloudError::Http(refused)).exit_status(), 5);
    for response in [
        b"".as_slice(),
        b"HTTP/1.1 418 Teapot\r\nContent-Length: 99\r\n\r\ncut",
    ] {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        let bytes = response.to_vec();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = socket.accept().unwrap();
            let mut input = [0; 4096];
            let read = stream.read(&mut input).unwrap();
            assert!(input[..read].starts_with(b"GET /"));
            stream.write_all(&bytes).unwrap();
        });
        let result = client.get(format!("http://{address}/")).send().await;
        let error = match result {
            Err(error) => Error::Cloud(CloudError::Http(error)),
            Ok(response) => Error::Cloud(CloudError::Status(remarkable_cloud::StatusError {
                op: "read".into(),
                status_code: response.status().as_u16(),
                body: "cut".into(),
                kind: None,
                body_error: Some(response.bytes().await.unwrap_err()),
            })),
        };
        assert_eq!(error.exit_status(), 5, "{error:?}");
        server.join().unwrap();
    }
    let invalid = client.get(":invalid").send().await.unwrap_err();
    assert_eq!(Error::Cloud(CloudError::Http(invalid)).exit_status(), 1);
}

#[test]
fn reports_keep_typed_candidates_and_joined_errors_human_readable() {
    let joined = Error::Multiple(vec![Error::Message("first".into()), Error::Cancelled]);
    assert_eq!(joined.to_string(), "first\ncommand cancelled");
    let candidates = [
        remarkable_cloud::AmbiguousCandidate {
            item: Default::default(),
            folder_path: "Folder".into(),
            unreachable: false,
        },
        remarkable_cloud::AmbiguousCandidate {
            item: Default::default(),
            folder_path: "".into(),
            unreachable: true,
        },
        remarkable_cloud::AmbiguousCandidate {
            item: Default::default(),
            folder_path: "Detached".into(),
            unreachable: true,
        },
    ]
    .to_vec();
    let error = Error::Multiple(vec![
        Error::Cloud(CloudError::Ambiguous(
            remarkable_cloud::AmbiguousNameError {
                query: "name".into(),
                name: "name".into(),
                candidates,
            },
        ))
        .context("resolve"),
    ]);
    let mut bytes = Vec::new();
    runtime::report(
        &after_commit(UpdateState::Committed, error),
        &Output::new(Mode::Agent),
        &mut bytes,
    )
    .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(text.matches("ERR:").count(), 1);
    assert!(text.contains("\t/Folder\tyes"));
    assert!(text.contains("\t-\tno"));
    assert!(text.contains("\tDetached\tno"));
    assert_eq!(
        Error::Cloud(CloudError::AttemptTimeout(Duration::ZERO)).exit_status(),
        5
    );
    assert_eq!(
        Error::Cloud(CloudError::Io(std::io::Error::from(
            std::io::ErrorKind::NotFound
        )))
        .exit_status(),
        3
    );
    let domain = |kind| CloudError::Domain {
        kind,
        message: "context".into(),
    };
    assert_eq!(
        Error::Cloud(CloudError::Multiple(vec![
            domain(ErrorKind::ItemNotFound),
            domain(ErrorKind::Unauthorized)
        ]))
        .exit_status(),
        4
    );
    assert_eq!(Error::Cloud(CloudError::Multiple(vec![])).exit_status(), 1);
    assert_eq!(
        Error::Render(remarkable_render::Error::Io(std::io::Error::from(
            std::io::ErrorKind::NotFound
        )))
        .exit_status(),
        3
    );
}
