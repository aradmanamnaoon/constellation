//! Real loopback HTTP/1.1 S3 conditional-object fixture. These tests exercise
//! the HTTP request/response boundary, not the pinned-TLS SigV4 transport or
//! any real cloud provider.
use super::*;
use crate::{DeviceId, DeviceRoster, SyncCoordinator, SyncError};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};

const PASSPHRASE: &[u8] = b"independent local http s3 test passphrase 2026";
// The Argon2 integration tests run concurrently and can briefly saturate the
// Windows host. This is a TEST fixture deadline, not the production HTTPS
// transport timeout. Keep each request bounded without treating CPU contention
// as a network protocol failure.
const FIXTURE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct HttpState {
    objects: BTreeMap<String, Vec<u8>>,
    ignored_conditions: bool,
    corrupt_create: bool,
    lose_head_ack_once: bool,
    writes: usize,
    rejected: usize,
}

struct HttpFixture {
    addr: SocketAddr,
    state: Arc<Mutex<HttpState>>,
    done: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl HttpFixture {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(HttpState::default()));
        let done = Arc::new(AtomicBool::new(false));
        let shared = Arc::clone(&state);
        let shutdown = Arc::clone(&done);
        let handle = std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !shutdown.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let shared = Arc::clone(&shared);
                        workers.push(std::thread::spawn(move || {
                            handle_connection(stream, shared)
                        }));
                        assert!(workers.len() <= 250, "bounded fixture connections");
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(err) => panic!("local HTTP fixture accept: {err}"),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            addr,
            state,
            done,
            handle: Some(handle),
        }
    }

    fn transport(&self) -> LocalHttpTransport {
        LocalHttpTransport { addr: self.addr }
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        self.handle.take().unwrap().join().unwrap();
    }
}

fn read_message(stream: &mut TcpStream, max_body: usize) -> std::io::Result<(String, Vec<u8>)> {
    stream.set_read_timeout(Some(FIXTURE_TIMEOUT))?;
    let mut bytes = Vec::new();
    let separator;
    loop {
        if bytes.len() > 8192 {
            return Err(std::io::Error::other("headers too long"));
        }
        let mut chunk = [0u8; 512];
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Err(std::io::Error::other("truncated headers"));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            separator = offset + 4;
            break;
        }
    }
    let headers = std::str::from_utf8(&bytes[..separator])
        .map_err(|_| std::io::Error::other("invalid HTTP headers"))?
        .to_owned();
    let length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|n| n.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    if length > max_body || bytes.len() - separator > length {
        return Err(std::io::Error::other("body limit"));
    }
    let mut body = bytes.split_off(separator);
    while body.len() < length {
        let mut chunk = [0u8; 4096];
        let max_chunk = (length - body.len()).min(chunk.len());
        let count = stream.read(&mut chunk[..max_chunk])?;
        if count == 0 {
            return Err(std::io::Error::other("truncated body"));
        }
        body.extend_from_slice(&chunk[..count]);
    }
    Ok((headers, body))
}

fn handle_connection(mut stream: TcpStream, shared: Arc<Mutex<HttpState>>) {
    let Ok((headers, body)) = read_message(&mut stream, MAX_ENVELOPE_BYTES) else {
        return;
    };
    let mut lines = headers.lines();
    let Some(start) = lines.next() else { return };
    let parts: Vec<&str> = start.split_whitespace().collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" || !parts[1].starts_with('/') {
        return;
    }
    let mut condition = None;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("if-none-match: ") {
            condition = Some(format!("none:{}", value.trim()));
        }
        if let Some(value) = lower.strip_prefix("if-match: ") {
            condition = Some(format!("match:{}", value.trim()));
        }
    }
    let mut state = shared.lock().unwrap();
    let previous = state.objects.get(parts[1]).cloned();
    let (code, response, etag) = match parts[0] {
        "GET" => match previous {
            Some(bytes) => (200, bytes.clone(), Some(format!("\"{}\"", sha_hex(&bytes)))),
            None => (404, Vec::new(), None),
        },
        "PUT" => {
            let allowed = state.ignored_conditions
                || match condition.as_deref() {
                    Some("none:*") => previous.is_none(),
                    Some(expected) if expected.starts_with("match:") => previous
                        .as_ref()
                        .is_some_and(|data| format!("\"{}\"", sha_hex(data)) == expected[6..]),
                    _ => false,
                };
            if !allowed {
                state.rejected += 1;
                (412, Vec::new(), None)
            } else {
                state.writes += 1;
                let stored = if state.corrupt_create && parts[1].starts_with("/revisions/") {
                    b"corrupted-after-ack".to_vec()
                } else {
                    body
                };
                let etag = Some(format!("\"{}\"", sha_hex(&stored)));
                state.objects.insert(parts[1].to_owned(), stored);
                if parts[1] == "/head" && state.lose_head_ack_once {
                    state.lose_head_ack_once = false;
                    return; // Actual write committed; socket closes without reply.
                }
                (200, Vec::new(), etag)
            }
        }
        _ => (400, Vec::new(), None),
    };
    drop(state);
    let mut response_headers = format!(
        "HTTP/1.1 {code} Result\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.len()
    );
    if let Some(etag) = etag {
        response_headers.push_str(&format!("ETag: {etag}\r\n"));
    }
    response_headers.push_str("\r\n");
    let _ = stream.write_all(response_headers.as_bytes());
    let _ = stream.write_all(&response);
}

#[derive(Clone)]
struct LocalHttpTransport {
    addr: SocketAddr,
}

impl S3ObjectTransport for LocalHttpTransport {
    fn send(&self, request: S3Request<'_>) -> Result<S3Response, StoreError> {
        let mut stream = TcpStream::connect_timeout(&self.addr, FIXTURE_TIMEOUT)
            .map_err(|_| StoreError::Failed)?;
        stream
            .set_read_timeout(Some(FIXTURE_TIMEOUT))
            .map_err(|_| StoreError::Failed)?;
        let method = match request.method {
            S3Method::Get => "GET",
            S3Method::Put => "PUT",
        };
        let condition = match request.condition {
            S3Condition::None => String::new(),
            S3Condition::CreateOnly => "If-None-Match: *\r\n".to_string(),
            S3Condition::IfMatch(etag) => format!("If-Match: {etag}\r\n"),
        };
        let header = format!(
            "{method} /{} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n{condition}\r\n",
            request.key,
            request.body.len()
        );
        stream
            .write_all(header.as_bytes())
            .map_err(|_| StoreError::Failed)?;
        stream
            .write_all(request.body)
            .map_err(|_| StoreError::Failed)?;
        let (headers, body) = read_message(&mut stream, request.max_response_bytes)
            .map_err(|_| StoreError::Failed)?;
        let code = headers
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|status| status.parse::<u16>().ok())
            .ok_or(StoreError::Failed)?;
        let etag = headers
            .lines()
            .find_map(|line| line.strip_prefix("ETag: "))
            .map(str::to_owned);
        Ok(S3Response {
            status: code,
            etag,
            body,
        })
    }
}

#[test]
fn real_tcp_http_s3_create_and_competing_device_cas_preserve_both_encrypted_branches() {
    let fixture = HttpFixture::new();
    let store = RemoteS3Store::new_disabled(fixture.transport());
    assert!(!store.supports_verified_cas());
    store.verify_service_contract().unwrap();
    let sync = SyncCoordinator::new(store);
    let root = sync
        .prepare(
            &DeviceId::new("owner").unwrap(),
            None,
            b"private root",
            PASSPHRASE,
        )
        .unwrap();
    sync.publish(&root, None, PASSPHRASE).unwrap();
    let id = root.id();
    let a = sync
        .prepare(
            &DeviceId::new("a").unwrap(),
            Some(id),
            b"private A",
            PASSPHRASE,
        )
        .unwrap();
    let b = sync
        .prepare(
            &DeviceId::new("b").unwrap(),
            Some(id),
            b"private B",
            PASSPHRASE,
        )
        .unwrap();
    sync.storage().put_if_absent(a.id(), &a.to_bytes()).unwrap();
    sync.storage().put_if_absent(b.id(), &b.to_bytes()).unwrap();
    let outcomes = std::thread::scope(|scope| {
        let x = scope.spawn(|| sync.storage().compare_and_swap_head(Some(id), a.id()));
        let y = scope.spawn(|| sync.storage().compare_and_swap_head(Some(id), b.id()));
        (x.join().unwrap().unwrap(), y.join().unwrap().unwrap())
    });
    assert_eq!(
        usize::from(outcomes.0 == CasResult::Swapped)
            + usize::from(outcomes.1 == CasResult::Swapped),
        1
    );
    assert!(sync.fetch_revision(a.id(), PASSPHRASE).is_ok());
    assert!(sync.fetch_revision(b.id(), PASSPHRASE).is_ok());
    let winning = sync.storage().head().unwrap().unwrap();
    let mut roster = DeviceRoster::new();
    roster.authorize(DeviceId::new("a").unwrap()).unwrap();
    roster.authorize(DeviceId::new("b").unwrap()).unwrap();
    let recovered = sync
        .recover_after_checkpoint(id, &roster, PASSPHRASE, 1)
        .unwrap();
    assert_eq!(recovered.current.id, winning);
    assert_eq!(recovered.revisions_since_checkpoint, vec![winning]);
    assert!(fixture.state.lock().unwrap().rejected >= 2);
}

#[test]
fn real_tcp_http_s3_ignoring_conditions_and_corrupt_put_are_rejected() {
    let broken = HttpFixture::new();
    broken.state.lock().unwrap().ignored_conditions = true;
    let unsafe_store = RemoteS3Store::new_disabled(broken.transport());
    assert!(unsafe_store.verify_service_contract().is_err());
    assert!(!unsafe_store.supports_verified_cas());

    let fixture = HttpFixture::new();
    let store = RemoteS3Store::new_disabled(fixture.transport());
    store.verify_service_contract().unwrap();
    fixture.state.lock().unwrap().corrupt_create = true;
    let sync = SyncCoordinator::new(store);
    let candidate = sync
        .prepare(
            &DeviceId::new("device").unwrap(),
            None,
            b"keep private",
            PASSPHRASE,
        )
        .unwrap();
    assert!(matches!(
        sync.publish(&candidate, None, PASSPHRASE),
        Err(SyncError::Store(StoreError::ImmutableConflict))
    ));
    assert_eq!(sync.storage().head().unwrap(), None);
}

#[test]
fn real_tcp_http_s3_lost_conditional_head_ack_never_blindly_retries() {
    let fixture = HttpFixture::new();
    let store = RemoteS3Store::new_disabled(fixture.transport());
    store.verify_service_contract().unwrap();
    let sync = SyncCoordinator::new(store);
    let original = sync
        .prepare(&DeviceId::new("owner").unwrap(), None, b"root", PASSPHRASE)
        .unwrap();
    sync.publish(&original, None, PASSPHRASE).unwrap();
    let next = sync
        .prepare(
            &DeviceId::new("other").unwrap(),
            Some(original.id()),
            b"next",
            PASSPHRASE,
        )
        .unwrap();
    fixture.state.lock().unwrap().lose_head_ack_once = true;
    assert!(matches!(
        sync.publish(&next, Some(original.id()), PASSPHRASE),
        Err(SyncError::Store(StoreError::Failed))
    ));
    assert_eq!(sync.storage().head().unwrap(), Some(next.id()));
    assert!(matches!(
        sync.publish_from_checkpoint(
            &next,
            original.id(),
            &{
                let mut r = DeviceRoster::new();
                r.authorize(DeviceId::new("other").unwrap()).unwrap();
                r
            },
            PASSPHRASE
        ),
        Err(SyncError::RollbackDetected)
    ));
    assert_eq!(
        sync.fetch_revision(next.id(), PASSPHRASE)
            .unwrap()
            .snapshot
            .as_slice(),
        b"next"
    );
}
