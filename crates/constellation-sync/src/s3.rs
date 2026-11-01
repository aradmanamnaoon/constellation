//! Opt-in S3-compatible HTTPS conditional-object adapter.
//!
//! The transport is intentionally *disabled* until it has executed an actual
//! create-only and concurrent If-Match probe against its configured service.
//! A local mock can test behavior but cannot certify any production endpoint.
//! The service remains outside the trust boundary: host-controlled trusted
//! checkpoints are still needed to detect a rollback.

use std::{
    fmt,
    io::Read,
    net::{IpAddr, SocketAddr},
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_LENGTH, ETAG};
use sha2::{Digest, Sha256};
use url::{Host, Url};
use zeroize::Zeroizing;

use crate::{
    CasResult, PutResult, RevisionEnvelope, RevisionId, RevisionStore, StoreError,
    MAX_ENVELOPE_BYTES,
};

const MAX_URL: usize = 1024;
const MAX_RESPONSE: usize = MAX_ENVELOPE_BYTES;
const MAX_HEAD: usize = 64;
const MAX_ETAG: usize = 128;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

type HmacSha256 = Hmac<Sha256>;

/// Path-style S3 configuration: `https://domain/bucket/prefix/...`.
/// The IP is pinned by the user/host after independent resolution. This crate
/// performs **no DNS lookup**, preventing resolver rebinding or private DNS.
#[derive(Clone)]
pub struct S3Endpoint {
    origin: String,
    host: String,
    pinned_public_ip: IpAddr,
    bucket: String,
    prefix: String,
    region: String,
}

#[cfg(test)]
#[path = "s3_http_tests.rs"]
mod http_tests;

impl fmt::Debug for S3Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("S3Endpoint([redacted])")
    }
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_object_component(value: &str) -> bool {
    // A SHA-256 content ID is 64 hex characters (one longer than an S3
    // bucket-label limit). The object-key grammar is intentionally distinct.
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let a = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || a[0] == 0
                || a[0] >= 224
                || a[0] == 100 && (64..=127).contains(&a[1])
                || a[0] == 192 && (a[1] == 0 || a[1] == 88 && a[2] == 99)
                || a[0] == 198 && (a[1] == 18 || a[1] == 19)
                || a[0] == 203 && a[1] == 0 && a[2] == 113
                || a[0] == 198 && a[1] == 51 && a[2] == 100
                || a[0] == 192 && a[1] == 0 && a[2] == 2)
        }
        IpAddr::V6(v6) => {
            let a = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (a[0] & 0xe000) != 0x2000
                || (a[0] & 0xfe00) == 0xfc00
                || (a[0] & 0xffc0) == 0xfe80
                || (a[0] & 0xffc0) == 0xfec0
                || (a[0] == 0x2001 && a[1] == 0)
                || (a[0] == 0x2002)
                || (a[0] & 0xff00) == 0x2000 && a[1] == 0x0db8
                || v6.to_ipv4_mapped().is_some()
                || v6.to_ipv4().is_some())
        }
    }
}

impl S3Endpoint {
    /// Allows only HTTPS on port 443, an unambiguous public DNS hostname and
    /// a separately supplied *public* fixed IP. No credentials in URLs.
    pub fn new(
        base: &str,
        pinned_public_ip: IpAddr,
        bucket: &str,
        prefix: &str,
        region: &str,
    ) -> Result<Self, StoreError> {
        if base.is_empty()
            || base.len() > MAX_URL
            || base.trim() != base
            || base.contains(['%', '@', '\\', '?', '#'])
            || !public_ip(pinned_public_ip)
            || !valid_segment(bucket)
            || !valid_segment(region)
            || prefix.split('/').any(|part| !valid_segment(part))
        {
            return Err(StoreError::Failed);
        }
        let parsed = Url::parse(base).map_err(|_| StoreError::Failed)?;
        if parsed.scheme() != "https"
            || parsed.port_or_known_default() != Some(443)
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(StoreError::Failed);
        }
        let Some(Host::Domain(host)) = parsed.host() else {
            return Err(StoreError::Failed);
        };
        if host.len() > 253
            || !host.contains('.')
            || host.ends_with(".local")
            || host.ends_with(".internal")
            || host.ends_with(".localhost")
            || host.split('.').any(|part| {
                !valid_segment(part)
                    || part.starts_with('-')
                    || part.ends_with('-')
                    || part.contains('_')
            })
            || host.eq_ignore_ascii_case("localhost")
        {
            return Err(StoreError::Failed);
        }
        // URL parsers normalize raw dot segments and case; do not silently
        // accept a different user-supplied location before pinning the host.
        if ![
            format!("https://{host}"),
            format!("https://{host}/"),
            format!("https://{host}:443"),
            format!("https://{host}:443/"),
        ]
        .iter()
        .any(|candidate| candidate == base)
        {
            return Err(StoreError::Failed);
        }
        Ok(Self {
            origin: format!("https://{host}"),
            host: host.to_owned(),
            pinned_public_ip,
            bucket: bucket.to_owned(),
            prefix: prefix.to_owned(),
            region: region.to_owned(),
        })
    }

    fn path(&self, key: &str) -> String {
        format!("/{}/{}/{}", self.bucket, self.prefix, key)
    }
}

/// Session-only credentials. Contents are not included in Debug output.
pub struct S3Credentials {
    access_key: Zeroizing<String>,
    secret_key: Zeroizing<String>,
    session_token: Option<Zeroizing<String>>,
}

impl fmt::Debug for S3Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("S3Credentials([redacted])")
    }
}

impl S3Credentials {
    pub fn new(
        access_key: String,
        secret_key: String,
        session_token: Option<String>,
    ) -> Result<Self, StoreError> {
        let allowed =
            |v: &str| !v.is_empty() && v.len() <= 8192 && v.bytes().all(|c| c.is_ascii_graphic());
        if !allowed(&access_key)
            || !allowed(&secret_key)
            || session_token
                .as_deref()
                .is_some_and(|token| !allowed(token))
        {
            return Err(StoreError::Failed);
        }
        Ok(Self {
            access_key: Zeroizing::new(access_key),
            secret_key: Zeroizing::new(secret_key),
            session_token: session_token.map(Zeroizing::new),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S3Condition<'a> {
    None,
    CreateOnly,
    IfMatch(&'a str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S3Method {
    Get,
    Put,
}

pub struct S3Request<'a> {
    pub method: S3Method,
    pub key: &'a str,
    pub condition: S3Condition<'a>,
    pub body: &'a [u8],
    pub max_response_bytes: usize,
}

pub struct S3Response {
    pub status: u16,
    pub etag: Option<String>,
    pub body: Vec<u8>,
}

/// Intended for reviewed conditional-object transports. The HTTPS
/// implementation below is the only production transport supplied by crate.
pub trait S3ObjectTransport: Send + Sync {
    fn send(&self, request: S3Request<'_>) -> Result<S3Response, StoreError>;
}

/// A real HTTPS transport with pinned public destination, strict TLS hostname
/// checking, Signature V4, disabled proxy inheritance and redirects.
pub struct HttpsS3Transport {
    endpoint: S3Endpoint,
    credentials: S3Credentials,
    client: reqwest::blocking::Client,
}

impl fmt::Debug for HttpsS3Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HttpsS3Transport([redacted])")
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn mac(key: &[u8], data: &[u8]) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    let mut hmac = HmacSha256::new_from_slice(key).map_err(|_| StoreError::Failed)?;
    hmac.update(data);
    Ok(Zeroizing::new(hmac.finalize().into_bytes().to_vec()))
}

fn valid_etag(value: &str) -> bool {
    value.len() >= 3
        && value.len() <= MAX_ETAG
        && value.starts_with('"')
        && value.ends_with('"')
        && value[1..value.len() - 1]
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
}

impl HttpsS3Transport {
    pub fn new(endpoint: S3Endpoint, credentials: S3Credentials) -> Result<Self, StoreError> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .pool_max_idle_per_host(0)
            .resolve(
                &endpoint.host,
                SocketAddr::new(endpoint.pinned_public_ip, 443),
            )
            .build()
            .map_err(|_| StoreError::Failed)?;
        Ok(Self {
            endpoint,
            credentials,
            client,
        })
    }

    fn sign(
        &self,
        method: S3Method,
        path: &str,
        condition: S3Condition<'_>,
        body: &[u8],
    ) -> Result<Vec<(String, String)>, StoreError> {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let date = &timestamp[..8];
        let body_digest = sha_hex(body);
        let mut headers = vec![
            ("host".to_owned(), self.endpoint.host.clone()),
            ("x-amz-content-sha256".to_owned(), body_digest.clone()),
            ("x-amz-date".to_owned(), timestamp.clone()),
        ];
        match condition {
            S3Condition::None => {}
            S3Condition::CreateOnly => headers.push(("if-none-match".into(), "*".into())),
            S3Condition::IfMatch(etag) => {
                if !valid_etag(etag) {
                    return Err(StoreError::Failed);
                }
                headers.push(("if-match".into(), etag.into()));
            }
        }
        if let Some(token) = &self.credentials.session_token {
            headers.push(("x-amz-security-token".into(), token.to_string()));
        }
        headers.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let signed = headers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");
        let canonical = headers
            .iter()
            .map(|(k, v)| format!("{k}:{}\n", v.trim()))
            .collect::<String>();
        let verb = if method == S3Method::Get {
            "GET"
        } else {
            "PUT"
        };
        let request = format!("{verb}\n{path}\n\n{canonical}\n{signed}\n{body_digest}");
        let scope = format!("{date}/{}/s3/aws4_request", self.endpoint.region);
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
            sha_hex(request.as_bytes())
        );
        let k0 =
            Zeroizing::new(format!("AWS4{}", self.credentials.secret_key.as_str()).into_bytes());
        let k_date = mac(&k0, date.as_bytes())?;
        let k_region = mac(&k_date, self.endpoint.region.as_bytes())?;
        let k_service = mac(&k_region, b"s3")?;
        let k_signing = mac(&k_service, b"aws4_request")?;
        let signature = hex(&mac(&k_signing, to_sign.as_bytes())?);
        headers.push((
            "authorization".into(),
            format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}", self.credentials.access_key.as_str()),
        ));
        Ok(headers)
    }
}

impl S3ObjectTransport for HttpsS3Transport {
    fn send(&self, request: S3Request<'_>) -> Result<S3Response, StoreError> {
        // Keys are adapter-generated ASCII paths only; never accept arbitrary
        // URLs, percent encodings, query strings or traversal.
        if request.key.is_empty()
            || request.key.len() > 180
            || request
                .key
                .split('/')
                .any(|piece| !valid_object_component(piece))
            || request.max_response_bytes > MAX_RESPONSE
            || request.body.len() > MAX_ENVELOPE_BYTES
            || request.method == S3Method::Put && request.condition == S3Condition::None
            || request.method == S3Method::Get
                && (!request.body.is_empty() || request.condition != S3Condition::None)
        {
            return Err(StoreError::Failed);
        }
        let path = self.endpoint.path(request.key);
        let headers = self.sign(request.method, &path, request.condition, request.body)?;
        let url = format!("{}{}", self.endpoint.origin, path);
        let method = match request.method {
            S3Method::Get => reqwest::Method::GET,
            S3Method::Put => reqwest::Method::PUT,
        };
        let mut builder = self
            .client
            .request(method, &url)
            .header("accept-encoding", "identity")
            .header("cache-control", "no-store");
        for (name, value) in headers {
            builder = builder.header(name, value);
        }
        if request.method == S3Method::Put {
            builder = builder.body(request.body.to_vec());
        }
        let response = builder.send().map_err(|_| StoreError::Failed)?;
        let status = response.status().as_u16();
        if !matches!(status, 200 | 201 | 404 | 412) {
            return Err(StoreError::Failed);
        }
        // S3 may return XML error bodies for 404 and 412. Do not consume,
        // parse, expose or allocate them; the HTTP status is sufficient.
        if matches!(status, 404 | 412) {
            return Ok(S3Response {
                status,
                etag: parse_etag(response.headers())?,
                body: Vec::new(),
            });
        }
        let max = if status == 200 && request.method == S3Method::Get {
            request.max_response_bytes
        } else {
            512
        };
        if response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<usize>().ok())
            .is_some_and(|size| size > max)
        {
            return Err(StoreError::SizeLimit);
        }
        let etag = parse_etag(response.headers())?;
        let mut body = Vec::new();
        response
            .take((max + 1) as u64)
            .read_to_end(&mut body)
            .map_err(|_| StoreError::Failed)?;
        if body.len() > max {
            return Err(StoreError::SizeLimit);
        }
        Ok(S3Response { status, etag, body })
    }
}

fn parse_etag(headers: &HeaderMap) -> Result<Option<String>, StoreError> {
    if headers.get_all(ETAG).iter().count() > 1 {
        return Err(StoreError::Failed);
    }
    headers
        .get(ETAG)
        .map(|value: &HeaderValue| {
            let text = value.to_str().map_err(|_| StoreError::Failed)?;
            if !valid_etag(text) {
                return Err(StoreError::Failed);
            }
            Ok(text.to_string())
        })
        .transpose()
}

fn head_payload(value: &[u8]) -> Result<RevisionId, StoreError> {
    if value.len() != MAX_HEAD {
        return Err(StoreError::Failed);
    }
    RevisionId::from_str(std::str::from_utf8(value).map_err(|_| StoreError::Failed)?)
        .map_err(|_| StoreError::Failed)
}

fn get<T: S3ObjectTransport>(
    transport: &T,
    key: &str,
    max_bytes: usize,
) -> Result<Option<(Vec<u8>, String)>, StoreError> {
    let reply = transport.send(S3Request {
        method: S3Method::Get,
        key,
        condition: S3Condition::None,
        body: &[],
        max_response_bytes: max_bytes,
    })?;
    match (reply.status, reply.etag) {
        (404, None) if reply.body.is_empty() => Ok(None),
        (200, Some(etag)) if reply.body.len() <= max_bytes && valid_etag(&etag) => {
            Ok(Some((reply.body, etag)))
        }
        _ => Err(StoreError::Failed),
    }
}

fn conditional_put<T: S3ObjectTransport>(
    transport: &T,
    key: &str,
    bytes: &[u8],
    cond: S3Condition<'_>,
) -> Result<bool, StoreError> {
    let reply = transport.send(S3Request {
        method: S3Method::Put,
        key,
        body: bytes,
        condition: cond,
        max_response_bytes: 512,
    })?;
    match reply.status {
        200 | 201 if reply.etag.as_deref().is_some_and(valid_etag) && reply.body.is_empty() => {
            Ok(true)
        }
        412 if reply.etag.is_none() && reply.body.is_empty() => Ok(false),
        _ => Err(StoreError::Failed),
    }
}

/// Disabled until the *actual configured service* passes a destructive but
/// isolated concurrency probe under a uniquely generated probe prefix.
/// Enabling is explicitly invoked by the host; there is no auto-start/sync.
pub struct RemoteS3Store<T: S3ObjectTransport> {
    transport: Arc<T>,
    verified: AtomicBool,
    revoked: AtomicBool,
    proof_lock: Mutex<()>,
}

impl<T: S3ObjectTransport> RemoteS3Store<T> {
    pub fn new_disabled(transport: T) -> Self {
        Self {
            transport: Arc::new(transport),
            verified: AtomicBool::new(false),
            revoked: AtomicBool::new(false),
            proof_lock: Mutex::new(()),
        }
    }

    /// Permanently disable this adapter instance for future operations.
    /// Revocation is process-local and does not invalidate cloud credentials or
    /// cancel a request that entered the transport before this call.
    pub fn revoke_session(&self) {
        self.revoked.store(true, Ordering::SeqCst);
        self.verified.store(false, Ordering::SeqCst);
    }

    /// Writes a handful of small uniquely named probe objects. Does not clean
    /// them up (S3 delete would alter the proof surface). Never retries writes.
    /// All other clients must use the same service's native conditional PUT.
    pub fn verify_service_contract(&self) -> Result<(), StoreError> {
        // Concurrent probes must not re-enable an adapter after another
        // unsuccessful probe cleared verification. Serialize the entire
        // explicit check and fail closed if lock state is uncertain.
        let _proof_lock = self.proof_lock.lock().map_err(|_| {
            self.verified.store(false, Ordering::SeqCst);
            StoreError::Poisoned
        })?;
        if self.revoked.load(Ordering::SeqCst) {
            return Err(StoreError::UnsupportedCas);
        }
        self.verified.store(false, Ordering::SeqCst);
        let mut random = [0_u8; 32];
        getrandom::getrandom(&mut random).map_err(|_| StoreError::Failed)?;
        let nonce = hex(&random);
        let immutable = format!("probe/{nonce}/immutable");
        let head = format!("probe/{nonce}/head");
        let before = b"initial";
        if !conditional_put(
            self.transport.as_ref(),
            &immutable,
            before,
            S3Condition::CreateOnly,
        )? {
            return Err(StoreError::Failed);
        }
        if conditional_put(
            self.transport.as_ref(),
            &immutable,
            b"replacement",
            S3Condition::CreateOnly,
        )? {
            return Err(StoreError::UnsupportedCas);
        }
        if get(self.transport.as_ref(), &immutable, 32)?.is_none_or(|(bytes, _)| bytes != before) {
            return Err(StoreError::Failed);
        }
        if !conditional_put(
            self.transport.as_ref(),
            &head,
            before,
            S3Condition::CreateOnly,
        )? {
            return Err(StoreError::Failed);
        }
        let (original, etag) =
            get(self.transport.as_ref(), &head, 32)?.ok_or(StoreError::Failed)?;
        if original != before {
            return Err(StoreError::Failed);
        }
        let transport = self.transport.clone();
        let (left, right) = thread::scope(|scope| {
            let first = scope.spawn(|| {
                conditional_put(
                    transport.as_ref(),
                    &head,
                    b"left",
                    S3Condition::IfMatch(&etag),
                )
            });
            let second = scope.spawn(|| {
                conditional_put(
                    self.transport.as_ref(),
                    &head,
                    b"right",
                    S3Condition::IfMatch(&etag),
                )
            });
            (first.join(), second.join())
        });
        let left = left.map_err(|_| StoreError::Failed)??;
        let right = right.map_err(|_| StoreError::Failed)??;
        if left == right {
            return Err(StoreError::UnsupportedCas);
        }
        let winning = if left {
            b"left".as_slice()
        } else {
            b"right".as_slice()
        };
        if get(self.transport.as_ref(), &head, 32)?
            .is_none_or(|(bytes, new_etag)| bytes != winning || new_etag == etag)
        {
            return Err(StoreError::Failed);
        }
        if conditional_put(
            self.transport.as_ref(),
            &head,
            b"stale",
            S3Condition::IfMatch(&etag),
        )? {
            return Err(StoreError::UnsupportedCas);
        }
        if self.revoked.load(Ordering::SeqCst) {
            return Err(StoreError::UnsupportedCas);
        }
        self.verified.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn require_proof(&self) -> Result<(), StoreError> {
        if self.verified.load(Ordering::SeqCst) && !self.revoked.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(StoreError::UnsupportedCas)
        }
    }
}

impl<T: S3ObjectTransport> RevisionStore for RemoteS3Store<T> {
    fn supports_verified_cas(&self) -> bool {
        self.verified.load(Ordering::SeqCst) && !self.revoked.load(Ordering::SeqCst)
    }

    fn head(&self) -> Result<Option<RevisionId>, StoreError> {
        self.require_proof()?;
        get(self.transport.as_ref(), "head", MAX_HEAD)?
            .map(|(bytes, _)| head_payload(&bytes))
            .transpose()
    }

    fn get_revision(
        &self,
        id: RevisionId,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        self.require_proof()?;
        if max_bytes > MAX_ENVELOPE_BYTES {
            return Err(StoreError::SizeLimit);
        }
        let key = format!("revisions/{}", id.to_hex());
        get(self.transport.as_ref(), &key, max_bytes)?
            .map(|(bytes, _)| {
                if RevisionEnvelope::from_bytes(&bytes)
                    .map_err(|_| StoreError::Failed)?
                    .id()
                    != id
                {
                    return Err(StoreError::Failed);
                }
                Ok(bytes)
            })
            .transpose()
    }

    fn put_if_absent(&self, id: RevisionId, bytes: &[u8]) -> Result<PutResult, StoreError> {
        self.require_proof()?;
        if bytes.len() > MAX_ENVELOPE_BYTES {
            return Err(StoreError::SizeLimit);
        }
        if RevisionEnvelope::from_bytes(bytes)
            .map_err(|_| StoreError::Failed)?
            .id()
            != id
        {
            return Err(StoreError::ImmutableConflict);
        }
        let key = format!("revisions/{}", id.to_hex());
        if conditional_put(
            self.transport.as_ref(),
            &key,
            bytes,
            S3Condition::CreateOnly,
        )? {
            // A success status/ETag alone is not a verified immutable write:
            // an incompatible service may ACK but ignore the body or write
            // something else after the initial probe passed.
            if get(self.transport.as_ref(), &key, MAX_ENVELOPE_BYTES)?
                .is_none_or(|(stored, _)| stored != bytes)
            {
                return Err(StoreError::ImmutableConflict);
            }
            return Ok(PutResult::Created);
        }
        match get(self.transport.as_ref(), &key, MAX_ENVELOPE_BYTES)? {
            Some((current, _)) if current == bytes => Ok(PutResult::AlreadyPresent),
            Some(_) => Err(StoreError::ImmutableConflict),
            None => Err(StoreError::Failed),
        }
    }

    fn compare_and_swap_head(
        &self,
        expected: Option<RevisionId>,
        next: RevisionId,
    ) -> Result<CasResult, StoreError> {
        self.require_proof()?;
        if self.get_revision(next, MAX_ENVELOPE_BYTES)?.is_none() {
            return Err(StoreError::Failed);
        }
        let original = get(self.transport.as_ref(), "head", MAX_HEAD)?;
        let observed = original
            .as_ref()
            .map(|(body, _)| head_payload(body))
            .transpose()?;
        if observed != expected {
            return Ok(CasResult::Conflict { current: observed });
        }
        let next_body = next.to_hex();
        let condition = original
            .as_ref()
            .map_or(S3Condition::CreateOnly, |(_, etag)| {
                S3Condition::IfMatch(etag)
            });
        if conditional_put(
            self.transport.as_ref(),
            "head",
            next_body.as_bytes(),
            condition,
        )? {
            return Ok(CasResult::Swapped);
        }
        let current = get(self.transport.as_ref(), "head", MAX_HEAD)?
            .map(|(body, _)| head_payload(&body))
            .transpose()?;
        Ok(CasResult::Conflict { current })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        net::IpAddr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Barrier, Mutex,
        },
    };

    use super::*;
    use crate::{DeviceId, DeviceRoster, PublishOutcome, SyncCoordinator, SyncError};

    const PASSPHRASE: &[u8] = b"local adversarial fixture high entropy test passphrase";

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Honest,
        IgnoresPreconditions,
        MissingEtag,
        Redirect,
        Malformed,
        Oversized,
        CorruptRead,
    }

    #[derive(Default)]
    struct FixtureState {
        objects: BTreeMap<String, Vec<u8>>,
        writes: usize,
        conditional_writes: usize,
        unconditional_writes: usize,
    }

    #[derive(Clone)]
    struct Fixture {
        state: Arc<Mutex<FixtureState>>,
        mode: Mode,
        fail_after_write_once: Arc<AtomicBool>,
        fail_after_head_once: Arc<AtomicBool>,
    }

    impl Fixture {
        fn new(mode: Mode) -> Self {
            Self {
                state: Arc::new(Mutex::new(FixtureState::default())),
                mode,
                fail_after_write_once: Arc::new(AtomicBool::new(false)),
                fail_after_head_once: Arc::new(AtomicBool::new(false)),
            }
        }

        fn current(&self, key: &str) -> Option<Vec<u8>> {
            self.state.lock().unwrap().objects.get(key).cloned()
        }

        fn wrote_unconditionally(&self) -> bool {
            self.state.lock().unwrap().unconditional_writes != 0
        }
    }

    impl S3ObjectTransport for Fixture {
        fn send(&self, request: S3Request<'_>) -> Result<S3Response, StoreError> {
            if self.mode == Mode::Redirect {
                return Ok(S3Response {
                    status: 307,
                    etag: None,
                    body: vec![],
                });
            }
            if self.mode == Mode::Malformed {
                return Ok(S3Response {
                    status: 200,
                    etag: Some("W/\"weak\"".into()),
                    body: vec![],
                });
            }
            if self.mode == Mode::Oversized {
                return Err(StoreError::SizeLimit);
            }
            let mut state = self.state.lock().map_err(|_| StoreError::Poisoned)?;
            match request.method {
                S3Method::Get => match state.objects.get(request.key) {
                    Some(data) => {
                        let bytes = if self.mode == Mode::CorruptRead {
                            b"malicious replace".to_vec()
                        } else {
                            data.clone()
                        };
                        if bytes.len() > request.max_response_bytes {
                            return Err(StoreError::SizeLimit);
                        }
                        Ok(S3Response {
                            status: 200,
                            etag: (self.mode != Mode::MissingEtag)
                                .then(|| format!("\"{}\"", sha_hex(data))),
                            body: bytes,
                        })
                    }
                    None => Ok(S3Response {
                        status: 404,
                        etag: None,
                        body: vec![],
                    }),
                },
                S3Method::Put => {
                    if request.condition == S3Condition::None {
                        state.unconditional_writes += 1;
                    } else {
                        state.conditional_writes += 1;
                    }
                    let current = state.objects.get(request.key);
                    let accepted = self.mode == Mode::IgnoresPreconditions
                        || match request.condition {
                            S3Condition::None => true,
                            S3Condition::CreateOnly => current.is_none(),
                            S3Condition::IfMatch(old) => current
                                .is_some_and(|bytes| format!("\"{}\"", sha_hex(bytes)) == old),
                        };
                    if !accepted {
                        return Ok(S3Response {
                            status: 412,
                            etag: None,
                            body: vec![],
                        });
                    }
                    state
                        .objects
                        .insert(request.key.to_string(), request.body.to_vec());
                    state.writes += 1;
                    if self.fail_after_write_once.swap(false, Ordering::SeqCst)
                        || request.key == "head"
                            && self.fail_after_head_once.swap(false, Ordering::SeqCst)
                    {
                        // The remote operation executed, but the client never
                        // received the authoritative acknowledgment.
                        return Err(StoreError::Failed);
                    }
                    Ok(S3Response {
                        status: 200,
                        etag: (self.mode != Mode::MissingEtag)
                            .then(|| format!("\"{}\"", sha_hex(request.body))),
                        body: vec![],
                    })
                }
            }
        }
    }

    fn verified_fixture() -> (Fixture, RemoteS3Store<Fixture>) {
        let fixture = Fixture::new(Mode::Honest);
        let store = RemoteS3Store::new_disabled(fixture.clone());
        assert!(!store.supports_verified_cas());
        store.verify_service_contract().unwrap();
        assert!(store.supports_verified_cas());
        assert!(!fixture.wrote_unconditionally());
        (fixture, store)
    }

    #[test]
    fn endpoint_disallows_private_addresses_dns_tricks_and_unsafe_urls() {
        let public: IpAddr = "8.8.8.8".parse().unwrap();
        for ip in [
            "127.0.0.1",
            "10.0.0.7",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.1.1",
            "100.64.0.1",
            "192.0.2.10",
            "198.51.100.5",
            "203.0.113.5",
            "224.0.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "2001:db8::5",
            "::ffff:127.0.0.1",
        ] {
            assert!(
                S3Endpoint::new(
                    "https://store.example.com",
                    ip.parse().unwrap(),
                    "private-bucket",
                    "constellation",
                    "us-east-1"
                )
                .is_err(),
                "{ip}"
            );
        }
        for base in [
            "http://store.example.com",
            "https://localhost",
            "https://example.internal",
            "https://example.local",
            "https://user:pass@store.example.com",
            "https://store.example.com@evil.com",
            "https://store.example.com/path",
            "https://store.example.com?url=evil",
            "https://store.example.com#fragment",
            "https://store.example.com:8443",
            "https://127.0.0.1",
            "https://2130706433",
            "https://store.example.com/%2e%2e",
            "https://store.example.com/.",
            "https://store.example.com/../",
            "https://store.example.com\\@evil.com",
            "https://store..example.com",
        ] {
            assert!(
                S3Endpoint::new(base, public, "private-bucket", "constellation", "us-east-1")
                    .is_err(),
                "{base}"
            );
        }
        assert!(S3Endpoint::new(
            "https://store.example.com",
            public,
            "private-bucket",
            "constellation",
            "us-east-1"
        )
        .is_ok());
        assert!(S3Endpoint::new(
            "https://store.example.com",
            public,
            "../bucket",
            "constellation",
            "us-east-1"
        )
        .is_err());
        let invalid = S3Credentials::new("id".into(), "secret\r\nHost:evil".into(), None);
        assert!(invalid.is_err());
        let creds = S3Credentials::new("id".into(), "private-secret".into(), None).unwrap();
        assert_eq!(format!("{creds:?}"), "S3Credentials([redacted])");
    }

    #[test]
    fn unproven_or_lying_service_never_allows_publish() {
        for mode in [
            Mode::IgnoresPreconditions,
            Mode::MissingEtag,
            Mode::Redirect,
            Mode::Malformed,
            Mode::Oversized,
            Mode::CorruptRead,
        ] {
            let fixture = Fixture::new(mode);
            let store = RemoteS3Store::new_disabled(fixture.clone());
            assert_eq!(store.head(), Err(StoreError::UnsupportedCas));
            assert!(store.verify_service_contract().is_err());
            assert!(!store.supports_verified_cas());
            assert!(!fixture.wrote_unconditionally());
            let coordinator = SyncCoordinator::new(store);
            let candidate = coordinator
                .prepare(
                    &DeviceId::new("fixture").unwrap(),
                    None,
                    b"private snapshot",
                    PASSPHRASE,
                )
                .unwrap();
            assert!(matches!(
                coordinator.publish(&candidate, None, PASSPHRASE),
                Err(SyncError::ConditionalWriteUnsupported)
            ));
        }
    }

    #[test]
    fn conditional_immutable_store_rejects_replay_and_preserves_encrypted_bytes() {
        let (fixture, store) = verified_fixture();
        let sync = SyncCoordinator::new(store);
        let original = sync
            .prepare(
                &DeviceId::new("laptop").unwrap(),
                None,
                b"encrypted dataset snapshot",
                PASSPHRASE,
            )
            .unwrap();
        assert!(matches!(
            sync.publish(&original, None, PASSPHRASE).unwrap(),
            PublishOutcome::Committed { .. }
        ));
        assert_eq!(
            sync.fetch_revision(original.id(), PASSPHRASE)
                .unwrap()
                .snapshot
                .as_slice(),
            b"encrypted dataset snapshot"
        );
        assert!(matches!(
            sync.publish(&original, None, PASSPHRASE),
            Err(SyncError::Replay)
        ));
        assert_eq!(
            sync.verified_head(Some(original.id()), PASSPHRASE)
                .unwrap()
                .unwrap()
                .id,
            original.id()
        );
        assert!(matches!(
            sync.verified_head(None, PASSPHRASE),
            Err(SyncError::RollbackDetected)
        ));
        let stored = fixture
            .current(&format!("revisions/{}", original.id()))
            .unwrap();
        assert_eq!(stored, original.to_bytes());
        assert!(!fixture.wrote_unconditionally());
    }

    #[test]
    fn competing_head_writers_exactly_one_wins_and_both_branches_survive() {
        let (_fixture, store) = verified_fixture();
        let sync = SyncCoordinator::new(store);
        let original = sync
            .prepare(
                &DeviceId::new("initial").unwrap(),
                None,
                b"initial",
                PASSPHRASE,
            )
            .unwrap();
        sync.publish(&original, None, PASSPHRASE).unwrap();
        let a = sync
            .prepare(
                &DeviceId::new("a").unwrap(),
                Some(original.id()),
                b"edit-a",
                PASSPHRASE,
            )
            .unwrap();
        let b = sync
            .prepare(
                &DeviceId::new("b").unwrap(),
                Some(original.id()),
                b"edit-b",
                PASSPHRASE,
            )
            .unwrap();
        sync.storage().put_if_absent(a.id(), &a.to_bytes()).unwrap();
        sync.storage().put_if_absent(b.id(), &b.to_bytes()).unwrap();
        let parent_id = original.id();
        let a_id = a.id();
        let b_id = b.id();
        let barrier = Arc::new(Barrier::new(2));
        let store = sync.storage();
        let outcomes = thread::scope(|scope| {
            let b1 = barrier.clone();
            let first = scope.spawn(move || {
                b1.wait();
                store.compare_and_swap_head(Some(parent_id), a_id)
            });
            let b2 = barrier.clone();
            let second = scope.spawn(move || {
                b2.wait();
                store.compare_and_swap_head(Some(parent_id), b_id)
            });
            (
                first.join().unwrap().unwrap(),
                second.join().unwrap().unwrap(),
            )
        });
        assert_eq!(
            usize::from(outcomes.0 == CasResult::Swapped)
                + usize::from(outcomes.1 == CasResult::Swapped),
            1
        );
        assert!(sync.fetch_revision(a.id(), PASSPHRASE).is_ok());
        assert!(sync.fetch_revision(b.id(), PASSPHRASE).is_ok());
        assert!(sync.verified_head(Some(original.id()), PASSPHRASE).is_err());
    }

    #[test]
    fn acknowledged_error_after_remote_write_never_automatically_retries() {
        let (fixture, store) = verified_fixture();
        let sync = SyncCoordinator::new(store);
        let candidate = sync
            .prepare(
                &DeviceId::new("test").unwrap(),
                None,
                b"snapshot",
                PASSPHRASE,
            )
            .unwrap();
        fixture.fail_after_write_once.store(true, Ordering::SeqCst);
        assert!(matches!(
            sync.publish(&candidate, None, PASSPHRASE),
            Err(SyncError::Store(StoreError::Failed))
        ));
        assert!(fixture
            .current(&format!("revisions/{}", candidate.id()))
            .is_some());
        assert_eq!(sync.storage().head().unwrap(), None);
        assert!(!fixture.wrote_unconditionally());
        // Retrying at the coordinator level becomes an explicit Replay. The
        // host must resolve ambiguity against trusted checkpoint/history.
        assert!(matches!(
            sync.publish(&candidate, None, PASSPHRASE),
            Err(SyncError::Replay)
        ));
    }

    #[test]
    fn remote_rollback_and_corrupted_revision_never_pass_trusted_checkpoint() {
        let (fixture, store) = verified_fixture();
        let sync = SyncCoordinator::new(store);
        let first = sync
            .prepare(&DeviceId::new("first").unwrap(), None, b"first", PASSPHRASE)
            .unwrap();
        sync.publish(&first, None, PASSPHRASE).unwrap();
        let second = sync
            .prepare(
                &DeviceId::new("second").unwrap(),
                Some(first.id()),
                b"second",
                PASSPHRASE,
            )
            .unwrap();
        sync.publish(&second, Some(first.id()), PASSPHRASE).unwrap();
        assert_eq!(
            sync.verified_head(Some(second.id()), PASSPHRASE)
                .unwrap()
                .unwrap()
                .id,
            second.id()
        );
        fixture
            .state
            .lock()
            .unwrap()
            .objects
            .insert("head".into(), first.id().to_hex().into_bytes());
        assert!(matches!(
            sync.verified_head(Some(second.id()), PASSPHRASE),
            Err(SyncError::RollbackDetected)
        ));
        fixture.state.lock().unwrap().objects.insert(
            format!("revisions/{}", second.id()),
            b"tampered bytes".to_vec(),
        );
        assert!(sync.fetch_revision(second.id(), PASSPHRASE).is_err());
    }

    #[test]
    fn lost_head_cas_ack_is_ambiguous_and_requires_trusted_reconciliation() {
        let (fixture, store) = verified_fixture();
        let sync = SyncCoordinator::new(store);
        let revision = sync
            .prepare(
                &DeviceId::new("one").unwrap(),
                None,
                b"revision",
                PASSPHRASE,
            )
            .unwrap();
        sync.storage()
            .put_if_absent(revision.id(), &revision.to_bytes())
            .unwrap();
        fixture.fail_after_head_once.store(true, Ordering::SeqCst);
        assert_eq!(
            sync.storage().compare_and_swap_head(None, revision.id()),
            Err(StoreError::Failed)
        );
        // A network failure is not proof of rollback or success: the server
        // committed but the writer never received its acknowledgment.
        assert_eq!(sync.storage().head().unwrap(), Some(revision.id()));
        assert!(matches!(
            sync.verified_head(None, PASSPHRASE),
            Err(SyncError::RollbackDetected)
        ));
        assert!(!fixture.wrote_unconditionally());
    }

    #[test]
    fn ambiguous_remote_commit_recovery_follows_trusted_ancestor_without_republishing() {
        let (fixture, store) = verified_fixture();
        let sync = SyncCoordinator::new(store);
        let root = sync
            .prepare(&DeviceId::new("owner").unwrap(), None, b"root", PASSPHRASE)
            .unwrap();
        sync.publish(&root, None, PASSPHRASE).unwrap();
        let candidate = sync
            .prepare(
                &DeviceId::new("laptop").unwrap(),
                Some(root.id()),
                b"unacknowledged-but-committed",
                PASSPHRASE,
            )
            .unwrap();
        let mut roster = DeviceRoster::new();
        roster.authorize(DeviceId::new("laptop").unwrap()).unwrap();
        fixture.fail_after_head_once.store(true, Ordering::SeqCst);
        assert!(matches!(
            sync.publish_from_checkpoint(&candidate, root.id(), &roster, PASSPHRASE),
            Err(SyncError::Store(StoreError::Failed))
        ));
        assert_eq!(sync.storage().head().unwrap(), Some(candidate.id()));
        let evidence = sync
            .recover_after_checkpoint(root.id(), &roster, PASSPHRASE, 1)
            .unwrap();
        assert_eq!(evidence.revisions_since_checkpoint, vec![candidate.id()]);
        assert_eq!(
            evidence.current.snapshot.as_slice(),
            b"unacknowledged-but-committed"
        );
        assert!(matches!(
            sync.publish_from_checkpoint(&candidate, root.id(), &roster, PASSPHRASE),
            Err(SyncError::RollbackDetected)
        ));
        assert!(!fixture.wrote_unconditionally());
    }

    #[test]
    fn revoke_local_s3_session_prevents_reproof_or_future_writes() {
        let (fixture, store) = verified_fixture();
        let before = fixture.state.lock().unwrap().writes;
        store.revoke_session();
        assert!(!store.supports_verified_cas());
        assert_eq!(store.head(), Err(StoreError::UnsupportedCas));
        assert_eq!(
            store.verify_service_contract(),
            Err(StoreError::UnsupportedCas)
        );
        assert!(!store.supports_verified_cas());
        let sync = SyncCoordinator::new(store);
        let candidate = sync
            .prepare(
                &DeviceId::new("owner").unwrap(),
                None,
                b"private",
                PASSPHRASE,
            )
            .unwrap();
        assert!(matches!(
            sync.publish(&candidate, None, PASSPHRASE),
            Err(SyncError::ConditionalWriteUnsupported)
        ));
        assert_eq!(fixture.state.lock().unwrap().writes, before);
        assert!(!fixture.wrote_unconditionally());
    }

    #[test]
    fn sigv4_signing_binds_conditional_headers_without_exposing_secret_in_debug() {
        let endpoint = S3Endpoint::new(
            "https://s3.example.com",
            "8.8.8.8".parse().unwrap(),
            "bucket-name",
            "constellation",
            "us-east-1",
        )
        .unwrap();
        let credentials = S3Credentials::new(
            "test-id".into(),
            "secret-never-log".into(),
            Some("token-never-log".into()),
        )
        .unwrap();
        let transport = HttpsS3Transport::new(endpoint, credentials).unwrap();
        assert!(!format!("{transport:?}").contains("secret-never-log"));
        let headers = transport
            .sign(
                S3Method::Put,
                "/bucket-name/constellation/head",
                S3Condition::IfMatch("\"abc\""),
                b"new",
            )
            .unwrap();
        assert!(headers
            .iter()
            .any(|(key, value)| key == "if-match" && value == "\"abc\""));
        let authorization = headers
            .iter()
            .find(|(key, _)| key == "authorization")
            .unwrap();
        assert!(authorization.1.contains(
            "SignedHeaders=host;if-match;x-amz-content-sha256;x-amz-date;x-amz-security-token"
        ));
        assert!(!headers
            .iter()
            .any(|(_, value)| value.contains("secret-never-log")));
        assert!(transport
            .sign(
                S3Method::Put,
                "/bucket-name/constellation/head",
                S3Condition::IfMatch("W/\"weak\""),
                b"new"
            )
            .is_err());
    }
}
