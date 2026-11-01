//! Localhost-only regression for ambient HTTP proxy bypass. This integration
//! test binary contains one test, so its temporary environment changes cannot
//! race another test in the same process. Cargo runs other integration-test
//! binaries in separate processes. All original variables are restored.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use constellation_provider_probe::{probe, ProbeRequest, ProbeStatus};
use tokio::runtime::Builder;

struct ScopedProxyEnvironment {
    originals: Vec<(&'static str, Option<OsString>)>,
}

impl ScopedProxyEnvironment {
    fn override_with(proxy: &str) -> Self {
        let keys: &[&str] = if cfg!(windows) {
            &["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"]
        } else {
            &[
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "NO_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
                "no_proxy",
            ]
        };
        let originals = keys
            .iter()
            .map(|&key| (key, std::env::var_os(key)))
            .collect();
        for &key in keys {
            if key.eq_ignore_ascii_case("no_proxy") {
                // Disable the normal implicit localhost proxy exception.
                std::env::set_var(key, "");
            } else {
                std::env::set_var(key, proxy);
            }
        }
        Self { originals }
    }
}

impl Drop for ScopedProxyEnvironment {
    fn drop(&mut self) {
        for (key, original) in &self.originals {
            match original {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn serve_one_if_contacted(
    listener: TcpListener,
    done: Arc<AtomicBool>,
    status: &'static str,
    body: &'static str,
) -> Option<String> {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while !done.load(Ordering::Acquire) && Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 4096];
                loop {
                    let n = stream.read(&mut chunk).ok()?;
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..n]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                        let content_length = headers
                            .lines()
                            .find_map(|line| line.trim().strip_prefix("content-length: "))
                            .and_then(|length| length.parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= end + 4 + content_length {
                            break;
                        }
                    }
                    assert!(request.len() < 16 * 1024, "request fixture must be bounded");
                }
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).ok()?;
                return Some(String::from_utf8_lossy(&request).into_owned());
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("unexpected local listener error: {error}"),
        }
    }
    None
}

#[test]
fn hostile_proxy_environment_cannot_receive_probe_request_or_token() {
    let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let target_url = format!("http://{}/v1", target.local_addr().unwrap());
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let scoped_environment = ScopedProxyEnvironment::override_with(&proxy_url);
    let done = Arc::new(AtomicBool::new(false));
    let target_done = Arc::clone(&done);
    let proxy_done = Arc::clone(&done);
    let target_server = thread::spawn(move || {
        serve_one_if_contacted(
            target,
            target_done,
            "200 OK",
            r#"{"choices":[{"message":{"content":"local fixture"}}]}"#,
        )
    });
    let proxy_server = thread::spawn(move || {
        serve_one_if_contacted(
            proxy,
            proxy_done,
            "401 Unauthorized",
            r#"{"error":"ambient proxy would have received the token"}"#,
        )
    });

    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let result = runtime.block_on(probe(ProbeRequest {
        base_url: &target_url,
        model_id: "proxy-regression-model",
        api_key: Some("local-test-bearer-placeholder"),
    }));
    thread::sleep(Duration::from_millis(100));
    done.store(true, Ordering::Release);
    let target_request = target_server.join().unwrap();
    let proxy_request = proxy_server.join().unwrap();
    drop(scoped_environment);

    assert_eq!(result.status, ProbeStatus::Compatible);
    assert_eq!(result.http_status, Some(200));
    assert!(
        target_request
            .as_deref()
            .is_some_and(|received| received.starts_with("POST /v1/chat/completions HTTP/1.1\r\n")),
        "explicit localhost target must receive the request"
    );
    assert!(
        target_request.as_deref().is_some_and(|received| received
            .to_ascii_lowercase()
            .contains("authorization: bearer local-test-bearer-placeholder")),
        "fixture credential must reach the intended local origin only"
    );
    assert!(
        proxy_request.is_none(),
        "proxy must not receive a request or any credential"
    );
}
