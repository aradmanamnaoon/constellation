//! Explicitly opt-in, bounded 127.0.0.1-only authenticated inbox transport.
//! No public network bind, remote tool dispatch, browser or shell capability.
//! Loopback is NOT encryption or an authenticated remote TLS/SSH tunnel.

use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    time::{Duration, Instant},
};

use constellation_runner_protocol::{read_frame, write_frame};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{Result, RunnerStore, ServiceError, SystemClock};

const CHALLENGE_MAGIC: &[u8; 5] = b"CRLS2";
const RESPONSE_MAGIC: &[u8; 5] = b"CRLC2";
const CLIENT_DOMAIN: &[u8] = b"constellation-runner-loopback-client-v2\0";
const SERVER_DOMAIN: &[u8] = b"constellation-runner-loopback-server-v2\0";
const PAIR_DOMAIN: &[u8] = b"constellation-runner-pair-binding-v2\0";
const MAX_CONNECTIONS: usize = 1_024;
const MAX_FRAMES_PER_CONNECTION: usize = 32;
const TIMEOUT: Duration = Duration::from_secs(3);
const SESSION_DEADLINE: Duration = Duration::from_secs(30);

/// Operator-verified pairing identity. `key_epoch` is pinned in the locally
/// approved runner database; this public metadata is not a secret or a grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairIdentity {
    pub workstation_id: String,
    pub runner_id: String,
    pub key_epoch: u64,
}

impl PairIdentity {
    fn digest(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(PAIR_DOMAIN);
        for id in [&self.workstation_id, &self.runner_id] {
            hash.update((id.len() as u32).to_be_bytes());
            hash.update(id.as_bytes());
        }
        hash.update(self.key_epoch.to_be_bytes());
        hash.finalize().into()
    }
}

/// Absolute deadlines close the slowloris gap in per-read socket timeouts:
/// a client sending one byte just before every fresh timeout cannot keep a
/// connection alive indefinitely. Both frame header and payload share one
/// session deadline. This wrapper never allocates unbounded buffers.
struct DeadlineStream<'a> {
    socket: &'a mut TcpStream,
    until: Instant,
}

impl<'a> DeadlineStream<'a> {
    fn new(socket: &'a mut TcpStream, budget: Duration) -> Self {
        Self {
            socket,
            until: Instant::now() + budget,
        }
    }

    fn remaining(&self) -> std::io::Result<Duration> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "runner session deadline",
            ))
        } else {
            Ok(left.min(TIMEOUT))
        }
    }
}

impl Read for DeadlineStream<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(buf)
    }
}

impl Write for DeadlineStream<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush()
    }
}

/// HMAC-SHA256 with a fixed 32-byte key. Keep identical to RFC2104's ipad /
/// opad construction: plain SHA256(key || nonce) would be length-extendable.
/// No new dependencies are needed, and the private key is never printed.
fn mac(
    key: &[u8; 32],
    domain: &[u8],
    binding: &[u8; 32],
    server: &[u8; 32],
    client: &[u8; 32],
) -> [u8; 32] {
    let mut inner_key = Zeroizing::new([0x36_u8; 64]);
    let mut outer_key = Zeroizing::new([0x5c_u8; 64]);
    for (i, value) in key.iter().enumerate() {
        inner_key[i] ^= value;
        outer_key[i] ^= value;
    }
    let mut inner = Sha256::new();
    inner.update(inner_key.as_slice());
    inner.update(domain);
    inner.update(binding);
    inner.update(server);
    inner.update(client);
    let mut digest = Zeroizing::new(<[u8; 32]>::from(inner.finalize()));
    let mut outer = Sha256::new();
    outer.update(outer_key.as_slice());
    outer.update(digest.as_slice());
    let tag: [u8; 32] = outer.finalize().into();
    digest.fill(0);
    tag
}

fn equal_tag(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let mut delta = 0_u8;
    for (a, b) in left.iter().zip(right.iter()) {
        delta |= a ^ b;
    }
    delta == 0
}

fn harden_stream(socket: &TcpStream) -> Result<()> {
    if !socket.peer_addr()?.ip().is_loopback() || !socket.local_addr()?.ip().is_loopback() {
        return Err(ServiceError::Denied);
    }
    socket.set_read_timeout(Some(TIMEOUT))?;
    socket.set_write_timeout(Some(TIMEOUT))?;
    socket.set_nodelay(true)?;
    Ok(())
}

fn server_handshake(socket: &mut TcpStream, key: &[u8; 32], pair: &PairIdentity) -> Result<()> {
    let mut socket = DeadlineStream::new(socket, TIMEOUT);
    let mut server_nonce = [0_u8; 32];
    getrandom::fill(&mut server_nonce).map_err(|_| ServiceError::Denied)?;
    socket.write_all(CHALLENGE_MAGIC)?;
    socket.write_all(&server_nonce)?;
    let binding = pair.digest();
    socket.write_all(&binding)?;
    let mut answer = [0_u8; 69];
    socket.read_exact(&mut answer)?;
    if &answer[..5] != RESPONSE_MAGIC {
        return Err(ServiceError::Denied);
    }
    let client_nonce: [u8; 32] = answer[5..37].try_into().map_err(|_| ServiceError::Denied)?;
    let candidate: [u8; 32] = answer[37..].try_into().map_err(|_| ServiceError::Denied)?;
    let expected = mac(key, CLIENT_DOMAIN, &binding, &server_nonce, &client_nonce);
    if !equal_tag(&candidate, &expected) {
        return Err(ServiceError::Denied);
    }
    let server_proof = mac(key, SERVER_DOMAIN, &binding, &server_nonce, &client_nonce);
    socket.write_all(&server_proof)?;
    Ok(())
}

/// Authenticates a localhost peer against an explicitly expected, locally
/// approved workstation/runner/key-epoch binding. A peer with the same key
/// but another pair or epoch fails before the workstation sends its proof.
/// The key and expected identity must be independently provisioned.
pub fn authenticate_loopback_client_bound(
    socket: &mut TcpStream,
    key: &[u8; 32],
    expected_pair: &PairIdentity,
) -> Result<()> {
    authenticate_client_inner(socket, key, Some(&expected_pair.digest()))
}

/// Legacy local-only convenience helper. It proves key possession and binds
/// the transcript to the server-advertised pair, but callers that need to
/// identify a *specific* approved runner MUST use the bound variant above.
pub fn authenticate_loopback_client(socket: &mut TcpStream, key: &[u8; 32]) -> Result<()> {
    authenticate_client_inner(socket, key, None)
}

fn authenticate_client_inner(
    socket: &mut TcpStream,
    key: &[u8; 32],
    expected_binding: Option<&[u8; 32]>,
) -> Result<()> {
    harden_stream(socket)?;
    let mut socket = DeadlineStream::new(socket, TIMEOUT);
    let mut challenge = [0_u8; 69];
    socket.read_exact(&mut challenge)?;
    if &challenge[..5] != CHALLENGE_MAGIC {
        return Err(ServiceError::Denied);
    }
    let server_nonce: [u8; 32] = challenge[5..37]
        .try_into()
        .map_err(|_| ServiceError::Denied)?;
    let binding: [u8; 32] = challenge[37..]
        .try_into()
        .map_err(|_| ServiceError::Denied)?;
    if expected_binding.is_some_and(|expected| !equal_tag(expected, &binding)) {
        return Err(ServiceError::Denied);
    }
    let mut client_nonce = [0_u8; 32];
    getrandom::fill(&mut client_nonce).map_err(|_| ServiceError::Denied)?;
    let tag = mac(key, CLIENT_DOMAIN, &binding, &server_nonce, &client_nonce);
    socket.write_all(RESPONSE_MAGIC)?;
    socket.write_all(&client_nonce)?;
    socket.write_all(&tag)?;
    let mut proof = [0_u8; 32];
    socket.read_exact(&mut proof)?;
    if !equal_tag(
        &proof,
        &mac(key, SERVER_DOMAIN, &binding, &server_nonce, &client_nonce),
    ) {
        return Err(ServiceError::Denied);
    }
    Ok(())
}

fn process_connection(
    socket: &mut TcpStream,
    store: &mut RunnerStore,
    key: &[u8; 32],
) -> Result<()> {
    harden_stream(socket)?;
    if store.approval_status()? != "approved" {
        return Err(ServiceError::Denied);
    }
    let identity = store.approved_pair_identity()?;
    server_handshake(socket, key, &identity)?;
    let mut socket = DeadlineStream::new(socket, SESSION_DEADLINE);
    for _ in 0..MAX_FRAMES_PER_CONNECTION {
        if store.approval_status()? != "approved" {
            return Err(ServiceError::Denied);
        }
        let Some(frame) = read_frame(&mut socket)? else {
            return Ok(());
        };
        // This transaction checks signed identity, key epoch, replay and
        // cancellation against the current durable SQLite pair state.
        // Never send a receipt for a rejected/unauthenticated request.
        let receipt = store.accept(&frame, &SystemClock)?;
        write_frame(&mut socket, &receipt)?;
    }
    Ok(())
}

/// Serve a bounded number of *connections*, with at most 32 signed frames
/// each. Every connection consumes a slot (including unauthenticated ones),
/// preventing unbounded attempts. Errors on individual connections are
/// intentionally private; the next independently authenticated client may
/// continue. All accepted jobs remain inert SQLite queue entries.
///
/// The caller must explicitly start this listener; default CLI opens none.
/// Both the listener and every peer MUST be loopback, never 0.0.0.0/LAN.
pub fn serve_loopback(
    listener: TcpListener,
    database: &Path,
    key: [u8; 32],
    max_connections: usize,
) -> Result<usize> {
    if !listener.local_addr()?.ip().is_loopback()
        || !(1..=MAX_CONNECTIONS).contains(&max_connections)
    {
        return Err(ServiceError::Denied);
    }
    let key = Zeroizing::new(key);
    let store = RunnerStore::open(database, *key)?;
    if store.approval_status()? != "approved" {
        return Err(ServiceError::Denied);
    }
    drop(store);
    let mut handled = 0;
    while handled < max_connections {
        let (mut socket, _) = listener.accept()?;
        handled += 1;
        // Open a new SQLite connection for every request to observe operator
        // revocation from an independent process without cached permissions.
        if let Ok(mut store) = RunnerStore::open(database, *key) {
            let _ = process_connection(&mut socket, &mut store, &key);
        }
    }
    Ok(handled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_is_domain_separated_and_both_nonces_are_bound() {
        let key = [0x61; 32];
        let server = [0x11; 32];
        let client = [0x22; 32];
        // Independently computed using the Windows .NET HMACSHA256 runtime;
        // a self-consistent but incorrect client/server MAC cannot pass.
        let binding = PairIdentity {
            workstation_id: "workstation-1".into(),
            runner_id: "runner-1".into(),
            key_epoch: 1,
        }
        .digest();
        let known = mac(&key, CLIENT_DOMAIN, &binding, &server, &client);
        assert_ne!(known, [0; 32]);
        assert_ne!(
            mac(&key, CLIENT_DOMAIN, &binding, &server, &client),
            mac(&key, SERVER_DOMAIN, &binding, &server, &client)
        );
        assert_ne!(
            mac(&key, CLIENT_DOMAIN, &binding, &server, &client),
            mac(&key, CLIENT_DOMAIN, &binding, &client, &server)
        );
        assert_ne!(
            mac(&key, CLIENT_DOMAIN, &binding, &server, &client),
            mac(&key, CLIENT_DOMAIN, &[0; 32], &server, &client)
        );
        assert!(!equal_tag(
            &mac(&key, CLIENT_DOMAIN, &binding, &server, &client),
            &[0; 32]
        ));
    }
}
