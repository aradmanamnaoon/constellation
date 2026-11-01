use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use constellation_provider_probe::{
    build_safe_client, completion_endpoint, prepare, probe, ProbeRequest, ProbeResult, ProbeStatus,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::runtime::Builder;
use tokio::time::timeout;

fn run<T>(f: impl std::future::Future<Output = T>) -> T {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(f)
}

fn request<'a>(url: &'a str, key: Option<&'a str>) -> ProbeRequest<'a> {
    ProbeRequest {
        base_url: url,
        model_id: "fixture-model",
        api_key: key,
    }
}

fn response(status: &str, body: &str, additional_headers: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{additional_headers}\r\n{body}",
        body.len()
    )
    .into_bytes()
}

async fn serve_one(bytes: Vec<u8>) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind localhost");
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let server = tokio::spawn(async move {
        let (mut stream, _) = timeout(Duration::from_secs(3), listener.accept())
            .await
            .expect("probe must connect")
            .expect("accept");
        let mut received = Vec::new();
        let mut chunk = [0u8; 2048];
        loop {
            let n = timeout(Duration::from_secs(3), stream.read(&mut chunk))
                .await
                .expect("request read deadline")
                .expect("read request");
            if n == 0 {
                break;
            }
            received.extend_from_slice(&chunk[..n]);
            if let Some(headers_end) = received.windows(4).position(|part| part == b"\r\n\r\n") {
                let header_text =
                    String::from_utf8_lossy(&received[..headers_end]).to_ascii_lowercase();
                let content_length = header_text
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("content-length: "))
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                if received.len() >= headers_end + 4 + content_length {
                    break;
                }
            }
            assert!(received.len() < 64 * 1024, "probe sent oversized request");
        }
        stream.write_all(&bytes).await.expect("send fixture");
        String::from_utf8_lossy(&received).into_owned()
    });
    (url, server)
}

async fn scenario(status: &str, body: &str, additional_headers: &str) -> (ProbeResult, String) {
    let (url, server) = serve_one(response(status, body, additional_headers)).await;
    let result = probe(request(&format!("{url}/v1"), Some("fixture-private-key"))).await;
    let captured = server.await.expect("server joined");
    (result, captured)
}

#[test]
fn successful_fixture_proves_only_openai_chat_shape_with_small_request() {
    run(async {
        let (result, received) = scenario(
            "200 OK",
            r#"{"choices":[{"message":{"role":"assistant","content":"OK"}}]}"#,
            "",
        )
        .await;
        assert_eq!(result.status, ProbeStatus::Compatible);
        assert_eq!(result.http_status, Some(200));
        assert!(received.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        assert!(
            received.contains("authorization: Bearer fixture-private-key")
                || received.contains("Authorization: Bearer fixture-private-key")
        );
        assert!(received.contains(r#""model":"fixture-model""#));
        assert!(received.contains(r#""max_tokens":1"#));
        assert!(received.contains(r#""stream":false"#));
        assert!(!received.contains("previous conversation"));
    });
}

#[test]
fn unauthorized_and_permission_denied_are_distinct_from_unreachable() {
    run(async {
        for code in ["401 Unauthorized", "403 Forbidden"] {
            let (result, _) = scenario(code, r#"{"error":"secret server details"}"#, "").await;
            assert_eq!(result.status, ProbeStatus::Unauthorized);
            assert_eq!(result.http_status, code[..3].parse::<u16>().ok());
            assert!(!format!("{result:?}").contains("secret server details"));
        }
    });
}

#[test]
fn invalid_json_and_missing_shape_are_separate_failures() {
    run(async {
        let (invalid, _) = scenario("200 OK", r#"{"choices":??}"#, "").await;
        assert_eq!(invalid.status, ProbeStatus::BadJson);
        let (wrong_shape, _) = scenario("200 OK", r#"{"models":[{"id":"x"}]}"#, "").await;
        assert_eq!(wrong_shape.status, ProbeStatus::Unsupported);
        let (empty, _) = scenario(
            "200 OK",
            r#"{"choices":[{"message":{"content":"   "}}]}"#,
            "",
        )
        .await;
        assert_eq!(empty.status, ProbeStatus::Unsupported);
    });
}

#[test]
fn unsupported_rate_limited_and_server_errors_have_distinct_statuses() {
    run(async {
        for (http, expected) in [
            ("404 Not Found", ProbeStatus::Unsupported),
            ("405 Method Not Allowed", ProbeStatus::Unsupported),
            ("429 Too Many Requests", ProbeStatus::RateLimited),
            ("502 Bad Gateway", ProbeStatus::ProviderError),
        ] {
            let (result, _) = scenario(http, "{}", "").await;
            assert_eq!(result.status, expected, "{http}");
        }
    });
}

#[test]
fn redirect_is_rejected_and_target_is_not_contacted() {
    run(async {
        let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let location = format!(
            "http://127.0.0.1:{}/steal",
            target.local_addr().unwrap().port()
        );
        let (result, _) = scenario(
            "307 Temporary Redirect",
            "{}",
            &format!("Location: {location}\r\n"),
        )
        .await;
        assert_eq!(result.status, ProbeStatus::Unsupported);
        assert_eq!(result.http_status, Some(307));
        assert!(timeout(Duration::from_millis(150), target.accept())
            .await
            .is_err());
    });
}

#[test]
fn oversized_reply_is_rejected_without_ever_exposing_body() {
    run(async {
        let sensitive = "PRIVATE_PROVIDER_RESPONSE".repeat(1200);
        let (result, _) = scenario("200 OK", &sensitive, "").await;
        assert_eq!(result.status, ProbeStatus::Unsupported);
        assert!(!format!("{result:?}").contains("PRIVATE_PROVIDER_RESPONSE"));
    });
}

#[test]
fn unreachable_localhost_is_reported_without_server_content() {
    run(async {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let result = probe(request(&format!("http://127.0.0.1:{port}/v1"), None)).await;
        assert_eq!(result.status, ProbeStatus::Unreachable);
        assert_eq!(result.http_status, None);
    });
}

#[test]
fn nonresponding_server_cannot_hold_probe_indefinitely() {
    run(async {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let held = tokio::spawn(async move {
            let (_connection, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(8)).await;
        });
        let now = Instant::now();
        let result = probe(request(&format!("http://localhost:{port}/v1"), None)).await;
        assert_eq!(result.status, ProbeStatus::Unreachable);
        assert!(now.elapsed() < Duration::from_secs(7));
        held.abort();
    });
}

#[test]
fn invalid_configurations_are_pure_and_never_contact_server() {
    run(async {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let valid = format!("http://127.0.0.1:{port}/v1");
        let invalid = [
            format!("http://user:password@127.0.0.1:{port}/v1"),
            format!("{valid}?token=private"),
            format!("{valid}#fragment"),
            format!("{valid}/../private"),
            format!("{valid}/%2e%2e/private"),
            format!("{valid}/foo//bar"),
            format!("{valid}/chat/completions"),
            "http://127.0.0.1:0/v1".to_string(),
            "http://169.254.169.254/latest/meta-data".into(),
            "http://192.168.1.1/v1".into(),
            "https://10.1.2.3/v1".into(),
            "http://example.com/v1".into(),
            "file:///etc/passwd".into(),
            "http://localhost.evil.test/v1".into(),
            "http://localhost\\@evil.example/v1".into(),
        ];
        for url in invalid {
            let result = probe(request(&url, None)).await;
            assert_eq!(result.status, ProbeStatus::InvalidConfiguration, "{url}");
        }
        assert!(timeout(Duration::from_millis(150), listener.accept())
            .await
            .is_err());
    });
}

#[test]
fn pure_validation_does_not_require_network() {
    let prepared = prepare(&request("https://example.com/custom/v1/", None));
    assert!(prepared.is_ok());
    assert_eq!(
        prepare(&ProbeRequest {
            base_url: "https://example.com/v1",
            model_id: "",
            api_key: None
        })
        .unwrap_err(),
        ProbeStatus::InvalidConfiguration
    );
    assert_eq!(
        prepare(&request("https://example.com/v1", Some("bad\r\nBearer"))).unwrap_err(),
        ProbeStatus::InvalidConfiguration
    );
}

#[test]
fn factory_reuses_probe_endpoint_and_can_only_prove_one_explicit_local_request() {
    run(async {
        let (base_url, server) = serve_one(response(
            "200 OK",
            r#"{"choices":[{"message":{"content":"hello"}}]}"#,
            "",
        ))
        .await;
        let base_url = format!("{base_url}/custom/v1/");
        let checked = completion_endpoint(&base_url).expect("same parser as probe");
        assert_eq!(checked.path(), "/custom/v1/chat/completions");
        let client = build_safe_client(&base_url)
            .await
            .expect("safe local client");
        let response = client
            .post(checked)
            .json(&serde_json::json!({"model":"fixture-model"}))
            .send()
            .await
            .expect("loopback response");
        assert_eq!(response.status().as_u16(), 200);
        let captured = server.await.unwrap();
        assert!(captured.starts_with("POST /custom/v1/chat/completions HTTP/1.1\r\n"));
    });
}

#[test]
fn factory_rejects_unsafe_https_literals_without_dns_or_http() {
    run(async {
        for bad in [
            "https://10.10.0.5/v1",
            "https://169.254.169.254/latest",
            "https://192.168.1.1/v1",
            "https://[fc00::42]/v1",
            "https://192.0.2.12/v1",
            "https://example.com/%2e%2e/private",
            "https://example.com/v1?secret=abc",
        ] {
            assert_eq!(
                build_safe_client(bad).await.err(),
                Some(ProbeStatus::InvalidConfiguration),
                "{bad}"
            );
            assert_eq!(
                completion_endpoint(bad).err(),
                Some(ProbeStatus::InvalidConfiguration),
                "{bad}"
            );
        }
        // Explicit loopback stays allowed under HTTPS as well as localhost
        // HTTP. Certificate trust is still verified when a request is made.
        assert!(completion_endpoint("https://127.0.0.1/v1").is_ok());
        assert!(completion_endpoint("https://[::1]/v1").is_ok());
    });
}

#[test]
fn client_factory_is_inert_until_callers_send_a_request() {
    run(async {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let endpoint = format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        let _client = build_safe_client(&endpoint).await.expect("safe client");
        assert!(timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err());
    });
}

#[test]
fn configured_url_rejects_ambiguous_authorities_paths_and_query_injection() {
    let invalid = [
        // Legacy/non-canonical IPv4 spelling must not disguise localhost.
        "http://2130706433/v1",
        "http://0x7f000001/v1",
        "http://0177.0.0.1/v1",
        "http://127.1/v1",
        "http://127.000.000.001/v1",
        // Redirects through URL parsing, credential tricks, and remote HTTP.
        "http://localhost.evil.example/v1",
        "http://127.0.0.1.evil.example/v1",
        "http://user@localhost/v1",
        "http://user:pass@localhost/v1",
        "https://user:pass@example.com/v1",
        "http://example.com/v1",
        "https://example.com:65536/v1",
        "http://localhost:0/v1",
        "ftp://example.com/v1",
        "file:///etc/passwd",
        "http:///v1",
        // URL parser normalizations could move a token to a different path.
        "http://localhost/v1/./nested",
        "http://localhost/v1/../hidden",
        "http://localhost/v1/%2e%2e/hidden",
        "http://localhost/v1/%2fadmin",
        "http://localhost/v1/%5cadmin",
        "http://localhost/v1/%00",
        "http://localhost/v1//nested",
        "http://localhost/v1\\hidden",
        "http://localhost/v1/chat/completions",
        "http://localhost/v1?api_key=secret",
        "http://localhost/v1#fragment",
        "http://localhost/v1?empty=",
        "http://localhost/v1#",
        // WHATWG URL parsing strips some ASCII controls; reject raw input.
        "http://local\thost/v1",
        "http://localhost/v\t1",
        "http://localhost/v\n1",
        "http://localhost/v\r1",
    ];
    for raw in invalid {
        assert_eq!(
            completion_endpoint(raw).err(),
            Some(ProbeStatus::InvalidConfiguration),
            "unsafe canonicalization accepted URL {raw:?}"
        );
        assert_eq!(
            prepare(&request(raw, None)).unwrap_err(),
            ProbeStatus::InvalidConfiguration,
            "prepare route drift for {raw:?}"
        );
    }
}

#[test]
fn configuration_bounds_are_enforced_without_logging_or_network_calls() {
    for (model, key) in [
        ("", None),
        (" ", None),
        ("model\ninject", None),
        ("model\tinject", None),
        ("model ", None),
        ("a", Some("")),
        ("a", Some("bearer\nX-Secret: leaked")),
        ("a", Some("bearer\r\nInjected: yes")),
        ("a", Some("with space")),
        ("a", Some("tab\t")),
    ] {
        assert_eq!(
            prepare(&ProbeRequest {
                base_url: "http://localhost/v1",
                model_id: model,
                api_key: key,
            })
            .unwrap_err(),
            ProbeStatus::InvalidConfiguration
        );
    }
    for (url, model, key) in [
        ("http://localhost/v1".repeat(200), "model".to_string(), None),
        ("http://localhost/v1".to_string(), "m".repeat(257), None),
        (
            "http://localhost/v1".to_string(),
            "model".to_string(),
            Some("k".repeat(8193)),
        ),
    ] {
        assert_eq!(
            prepare(&ProbeRequest {
                base_url: &url,
                model_id: &model,
                api_key: key.as_deref(),
            })
            .unwrap_err(),
            ProbeStatus::InvalidConfiguration
        );
    }
    for good in [
        "http://127.0.0.1:65535",
        "http://localhost/v1",
        "http://[::1]:4321/custom/v1",
        "https://public.example/v1",
    ] {
        assert!(completion_endpoint(good).is_ok(), "{good}");
    }
}
