//! Bounded in-memory authorization and device lifecycle.
use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use rand::{random, random_range};
use serde::Serialize;
use subtle::ConstantTimeEq;
use tokio::sync::{oneshot, watch};

use crate::{
    device::{DeviceInput, run_xbox_owner},
    input::InputFrame,
};

const AUTH_TTL: Duration = Duration::from_secs(120);
const RECONNECT_TTL: Duration = Duration::from_secs(15);
const MAX_PENDING: usize = 8;
const MAX_SESSIONS: usize = 4;

pub(crate) fn secret() -> String {
    let mut result = String::with_capacity(64);
    for byte in random::<[u8; 32]>() {
        let _ = write!(result, "{byte:02x}");
    }
    result
}

pub(crate) fn matches(expected: &str, actual: &str) -> bool {
    expected.as_bytes().ct_eq(actual.as_bytes()).unwrap_u8() == 1
}

pub(crate) fn pin(code: &str) -> Option<String> {
    let digits = if let Some((first, last)) = code.split_once('-') {
        if first.len() != 4 || last.len() != 4 {
            return None;
        }
        format!("{first}{last}")
    } else {
        code.to_owned()
    };
    (digits.len() == 8 && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(digits)
}

struct Invite {
    id: u64,
    token: String,
    pin: String,
    expires: Option<Instant>,
}

struct Request {
    token: String,
    approved: bool,
    expires: Instant,
}

struct Device {
    input: watch::Sender<DeviceInput>,
    ready: watch::Receiver<bool>,
}

struct Session {
    token: String,
    expires: Option<Instant>,
    socket: Option<watch::Sender<()>>,
    device: Option<Device>,
    closing: bool,
    connections: u64,
    frames: u64,
}

impl Session {
    fn close(&mut self) {
        self.closing = true;
        self.socket = None;
        self.device = None;
    }
}

#[derive(Clone, Default)]
pub(crate) struct Sessions(Arc<Mutex<Registry>>);

#[derive(Default)]
pub(crate) struct Registry {
    next_id: u64,
    invites: Vec<Invite>,
    requests: HashMap<u64, Request>,
    sessions: HashMap<u64, Session>,
    failures: Option<(Instant, u8)>,
    stopping: bool,
    // Runtime-only administrative admission policy. Defaults retain M2 behavior.
    exposure_paused: bool,
    joins_blocked: bool,
    session_limit: Option<usize>,
    #[cfg(test)]
    pub(crate) owner: Option<TestOwner>,
}

#[cfg(test)]
type TestOwner =
    fn(watch::Receiver<DeviceInput>, oneshot::Sender<Result<(), String>>) -> Result<(), String>;

/// Safe, secret-free administrative status response.
#[derive(Serialize)]
pub(crate) struct AdminSnapshot {
    pub exposure_enabled: bool,
    pub join_requests_enabled: bool,
    pub max_sessions: usize,
    pub sessions: Vec<SessionView>,
    pub invites: Vec<InviteView>,
    pub requests: Vec<RequestView>,
}

#[derive(Serialize)]
pub(crate) struct SessionView {
    pub id: u64,
    pub status: &'static str,
    pub connections: u64,
    pub frames: u64,
    pub expires_in_seconds: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct InviteView {
    pub id: u64,
    pub reusable: bool,
    pub expires_in_seconds: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct RequestView {
    pub id: u64,
    pub approved: bool,
    pub expires_in_seconds: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Unauthorized,
    Pending,
    Busy,
    Limited,
}

impl Sessions {
    pub(crate) fn lock(&self) -> MutexGuard<'_, Registry> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn attach(&self, token: &str) -> Result<SocketLease, Error> {
        let mut registry = self.lock();
        registry.expire();
        if registry.exposure_paused {
            return Err(Error::Busy);
        }
        let (&id, session) = registry
            .sessions
            .iter_mut()
            .find(|(_, session)| !session.closing && matches(&session.token, token))
            .ok_or(Error::Unauthorized)?;
        if session.socket.is_some() {
            return Err(Error::Busy);
        }

        let (cancel, cancelled) = watch::channel(());
        session.socket = Some(cancel);
        Ok(SocketLease {
            sessions: self.clone(),
            id,
            cancelled,
            connected: false,
        })
    }
}

impl Registry {
    pub(crate) fn snapshot(&mut self) -> AdminSnapshot {
        self.expire();
        let now = Instant::now();
        let mut sessions: Vec<_> = self
            .sessions
            .iter()
            .map(|(&id, session)| {
                let status = if session.closing {
                    "closing"
                } else if session.socket.is_some() {
                    "connected"
                } else if session.device.is_some() {
                    "reconnecting"
                } else {
                    "authorized"
                };
                SessionView {
                    id,
                    status,
                    connections: session.connections,
                    frames: session.frames,
                    expires_in_seconds: session
                        .expires
                        .map(|end| end.saturating_duration_since(now).as_secs()),
                }
            })
            .collect();
        sessions.sort_by_key(|entry| entry.id);
        let mut invites: Vec<_> = self
            .invites
            .iter()
            .map(|invite| InviteView {
                id: invite.id,
                reusable: invite.expires.is_none(),
                expires_in_seconds: invite
                    .expires
                    .map(|end| end.saturating_duration_since(now).as_secs()),
            })
            .collect();
        invites.sort_by_key(|entry| entry.id);
        let mut requests: Vec<_> = self
            .requests
            .iter()
            .map(|(&id, request)| RequestView {
                id,
                approved: request.approved,
                expires_in_seconds: request.expires.saturating_duration_since(now).as_secs(),
            })
            .collect();
        requests.sort_by_key(|entry| entry.id);
        AdminSnapshot {
            exposure_enabled: !self.exposure_paused,
            join_requests_enabled: !self.joins_blocked,
            max_sessions: self.session_limit.unwrap_or(MAX_SESSIONS),
            sessions,
            invites,
            requests,
        }
    }

    /// Disabling exposure revokes admission credentials and ends every device.
    pub(crate) fn set_exposure(&mut self, enabled: bool) {
        self.exposure_paused = !enabled;
        if !enabled {
            self.invites.clear();
            self.requests.clear();
            let ids: Vec<_> = self.sessions.keys().copied().collect();
            for id in ids {
                self.end(id);
            }
        }
    }

    /// New policy applies to future admissions; existing excess sessions stay.
    pub(crate) fn set_policy(&mut self, join_requests_enabled: bool, max_sessions: usize) -> bool {
        if !(1..=MAX_SESSIONS).contains(&max_sessions) {
            return false;
        }
        self.joins_blocked = !join_requests_enabled;
        self.session_limit = Some(max_sessions);
        if !join_requests_enabled {
            self.requests.clear();
        }
        true
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub(crate) fn expire(&mut self) {
        let now = Instant::now();
        self.invites
            .retain(|invite| invite.expires.is_none_or(|expiry| expiry > now));
        self.requests.retain(|_, request| request.expires > now);
        self.sessions.retain(|_, session| {
            if session.closing {
                return true;
            }
            if session.expires.is_some_and(|deadline| deadline <= now) {
                let worker_running = session.device.is_some();
                session.close();
                return worker_running;
            }
            true
        });
    }

    pub(crate) fn invite(&mut self, reusable: bool) -> Result<(u64, String, String), Error> {
        self.expire();
        if self.stopping || self.exposure_paused || self.invites.len() >= MAX_PENDING {
            return Err(Error::Busy);
        }
        let digits = loop {
            let digits = format!("{:08}", random_range(0..100_000_000u32));
            if self.invites.iter().all(|invite| invite.pin != digits) {
                break digits;
            }
        };
        let token = secret();
        let display_pin = format!("{}-{}", &digits[..4], &digits[4..]);
        let id = self.id();
        self.invites.push(Invite {
            id,
            token: token.clone(),
            pin: digits,
            expires: (!reusable).then(|| Instant::now() + AUTH_TTL),
        });
        Ok((id, token, display_pin))
    }

    fn capacity(&self) -> Result<(), Error> {
        if self.stopping
            || self.exposure_paused
            || self.sessions.len() >= self.session_limit.unwrap_or(MAX_SESSIONS)
        {
            return Err(Error::Busy);
        }
        Ok(())
    }

    fn authorize(&mut self) -> String {
        let token = secret();
        let id = self.id();
        self.sessions.insert(
            id,
            Session {
                token: token.clone(),
                expires: Some(Instant::now() + AUTH_TTL),
                socket: None,
                device: None,
                closing: false,
                connections: 0,
                frames: 0,
            },
        );
        token
    }

    pub(crate) fn redeem(&mut self, credential: &str) -> Result<String, Error> {
        self.expire();
        if self.stopping || self.exposure_paused {
            return Err(Error::Busy);
        }
        let now = Instant::now();
        let (started, failures) = self.failures.get_or_insert((now, 0));
        if now.duration_since(*started) >= Duration::from_secs(60) {
            (*started, *failures) = (now, 0);
        }
        if *failures >= 5 {
            return Err(Error::Limited);
        }
        let normalized = pin(credential);
        let Some(index) = self.invites.iter().position(|invite| {
            matches(&invite.token, credential)
                || normalized
                    .as_ref()
                    .is_some_and(|pin| matches(&invite.pin, pin))
        }) else {
            *failures += 1;
            return Err(Error::Unauthorized);
        };
        self.capacity()?;
        if self.invites[index].expires.is_some() {
            self.invites.swap_remove(index);
        }
        Ok(self.authorize())
    }

    pub(crate) fn revoke_invite(&mut self, id: u64) -> bool {
        self.expire();
        let length = self.invites.len();
        self.invites.retain(|invite| invite.id != id);
        length != self.invites.len()
    }

    pub(crate) fn request(&mut self, cookie: Option<&str>) -> Result<(u64, String), Error> {
        self.expire();
        if self.exposure_paused || self.joins_blocked {
            return Err(Error::Busy);
        }
        if let Some((&id, request)) = self
            .requests
            .iter()
            .find(|(_, request)| cookie.is_some_and(|cookie| matches(&request.token, cookie)))
        {
            return Ok((id, request.token.clone()));
        }
        if self.stopping || self.requests.len() >= MAX_PENDING {
            return Err(Error::Busy);
        }
        let id = self.id();
        let token = secret();
        self.requests.insert(
            id,
            Request {
                token: token.clone(),
                approved: false,
                expires: Instant::now() + AUTH_TTL,
            },
        );
        Ok((id, token))
    }

    pub(crate) fn resolve(&mut self, id: u64, approve: bool) -> bool {
        self.expire();
        if approve {
            if let Some(request) = self.requests.get_mut(&id) {
                request.approved = true;
                return true;
            }
            false
        } else {
            self.requests.remove(&id).is_some()
        }
    }

    pub(crate) fn claim(&mut self, cookie: &str) -> Result<String, Error> {
        self.expire();
        if self.exposure_paused || self.joins_blocked {
            return Err(Error::Busy);
        }
        let (&id, request) = self
            .requests
            .iter()
            .find(|(_, request)| matches(&request.token, cookie))
            .ok_or(Error::Unauthorized)?;
        if !request.approved {
            return Err(Error::Pending);
        }
        self.capacity()?;
        self.requests.remove(&id);
        Ok(self.authorize())
    }

    pub(crate) fn has_session(&self, token: &str) -> bool {
        self.sessions
            .values()
            .any(|session| !session.closing && matches(&session.token, token))
    }

    pub(crate) fn end_token(&mut self, token: &str) -> bool {
        let id = self.sessions.iter().find_map(|(&id, session)| {
            (!session.closing && matches(&session.token, token)).then_some(id)
        });
        id.is_some_and(|id| self.end(id))
    }

    pub(crate) fn end(&mut self, id: u64) -> bool {
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        if !session.closing {
            if session.device.is_some() {
                session.close();
            } else {
                self.sessions.remove(&id);
            }
        }
        true
    }

    pub(crate) fn shutdown(&mut self) {
        self.stopping = true;
        self.sessions.retain(|_, session| {
            let worker_running = session.closing || session.device.is_some();
            session.close();
            worker_running
        });
    }

    pub(crate) fn drained(&self) -> bool {
        self.sessions.is_empty()
    }

    pub(crate) fn describe(&mut self) -> String {
        self.expire();
        let mut output = String::new();
        for invite in &self.invites {
            let expiry = invite.expires.map_or_else(
                || "until revoked".into(),
                |deadline| {
                    format!(
                        "{}s",
                        deadline.saturating_duration_since(Instant::now()).as_secs()
                    )
                },
            );
            let _ = writeln!(output, "invite {}: expiry {expiry}", invite.id);
        }
        for (id, request) in &self.requests {
            let status = if request.approved {
                "approved"
            } else {
                "pending"
            };
            let remaining = request
                .expires
                .saturating_duration_since(Instant::now())
                .as_secs();
            let _ = writeln!(output, "request {id}: {status}, expires in {remaining}s");
        }
        for (id, session) in &self.sessions {
            let status = match (
                session.closing,
                session.socket.is_some(),
                session.device.is_some(),
            ) {
                (true, _, _) => "closing",
                (_, true, _) => "connected",
                (_, _, true) => "reconnecting",
                _ => "authorized",
            };
            let expiry = session.expires.map_or_else(
                || "-".into(),
                |deadline| {
                    format!(
                        "{}s",
                        deadline.saturating_duration_since(Instant::now()).as_secs()
                    )
                },
            );
            let _ = writeln!(
                output,
                "session {id}: {status}, expiry {expiry}, connections {}, frames {}",
                session.connections, session.frames
            );
        }
        if output.is_empty() {
            output.push_str("No pending requests or sessions.\n");
        }
        output
    }
}

pub(crate) struct SocketLease {
    sessions: Sessions,
    id: u64,
    pub(crate) cancelled: watch::Receiver<()>,
    connected: bool,
}

impl SocketLease {
    pub(crate) fn start(&mut self) -> Option<watch::Receiver<bool>> {
        #[cfg(test)]
        let owner = self.sessions.lock().owner.unwrap_or(run_xbox_owner);
        #[cfg(not(test))]
        let owner = run_xbox_owner;
        self.start_with(owner)
    }

    fn start_with(
        &mut self,
        owner: impl FnOnce(
            watch::Receiver<DeviceInput>,
            oneshot::Sender<Result<(), String>>,
        ) -> Result<(), String>
        + Send
        + 'static,
    ) -> Option<watch::Receiver<bool>> {
        let mut registry = self.sessions.lock();
        registry.expire();
        if registry.exposure_paused {
            return None;
        }
        let session = registry.sessions.get_mut(&self.id)?;
        if session.closing {
            return None;
        }
        self.connected = true;
        session.expires = None;
        session.connections += 1;
        if let Some(device) = &session.device {
            return Some(device.ready.clone());
        }

        let (input, snapshots) = watch::channel(DeviceInput::default());
        let (startup, started) = oneshot::channel();
        let (ready, status) = watch::channel(false);
        let worker = tokio::task::spawn_blocking(move || owner(snapshots, startup));
        let sessions = self.sessions.clone();
        let id = self.id;
        tokio::spawn(async move {
            if matches!(started.await, Ok(Ok(()))) {
                ready.send_replace(true);
            }
            match worker.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => eprintln!("session {id}: device error: {error}"),
                Err(error) => eprintln!("session {id}: worker failure: {error}"),
            }
            sessions.lock().sessions.remove(&id);
        });
        session.device = Some(Device {
            input,
            ready: status.clone(),
        });
        Some(status)
    }

    pub(crate) fn publish(&self, frame: InputFrame) {
        let mut registry = self.sessions.lock();
        let Some(session) = registry.sessions.get_mut(&self.id) else {
            return;
        };
        if let Some(device) = &session.device {
            device.input.send_replace(DeviceInput {
                connection: session.connections,
                frame: Some(frame),
            });
            session.frames += 1;
        }
    }
}

impl Drop for SocketLease {
    fn drop(&mut self) {
        let mut registry = self.sessions.lock();
        if let Some(session) = registry
            .sessions
            .get_mut(&self.id)
            .filter(|session| !session.closing)
        {
            session.socket = None;
            if !self.connected {
                return;
            }
            if let Some(device) = &session.device {
                device.input.send_replace(DeviceInput {
                    connection: session.connections,
                    frame: None,
                });
            }
            session.expires = Some(
                Instant::now()
                    + if session.device.is_some() {
                        RECONNECT_TTL
                    } else {
                        AUTH_TTL
                    },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, MAX_SESSIONS, Registry, Sessions, pin};
    use std::time::{Duration, Instant};

    use proptest::prelude::*;

    #[test]
    fn admin_exposure_revokes_invites_joins_and_sessions() {
        let mut registry = Registry::default();
        let (_, invite, _) = registry.invite(false).unwrap();
        let (request_id, request_token) = registry.request(None).unwrap();
        let token = registry.redeem(&invite).unwrap();
        let id = *registry.sessions.keys().next().unwrap();
        registry.set_exposure(false);
        assert!(!registry.snapshot().exposure_enabled);
        assert!(registry.snapshot().sessions.is_empty());
        assert!(registry.snapshot().invites.is_empty());
        assert!(registry.snapshot().requests.is_empty());
        assert_eq!(registry.redeem(&invite), Err(Error::Busy));
        assert_eq!(registry.request(None), Err(Error::Busy));
        assert_eq!(registry.claim(&request_token), Err(Error::Busy));
        assert!(!registry.resolve(request_id, true));
        assert!(!registry.has_session(&token));
        assert!(!registry.end(id));
        registry.set_exposure(true);
        assert!(registry.snapshot().exposure_enabled);
        assert!(registry.invite(false).is_ok());
    }

    #[test]
    fn admin_policy_limits_new_sessions_and_disables_requests() {
        let mut registry = Registry::default();
        assert!(!registry.set_policy(true, 0));
        assert!(!registry.set_policy(true, MAX_SESSIONS + 1));
        let (id, request) = registry.request(None).unwrap();
        assert!(registry.set_policy(false, 1));
        assert_eq!(registry.request(None), Err(Error::Busy));
        assert_eq!(registry.claim(&request), Err(Error::Busy));
        assert!(!registry.resolve(id, true));
        let (_, credential, _) = registry.invite(true).unwrap();
        assert!(registry.redeem(&credential).is_ok());
        assert_eq!(registry.redeem(&credential), Err(Error::Busy));
        assert_eq!(registry.snapshot().max_sessions, 1);
        assert!(!registry.snapshot().join_requests_enabled);
    }

    #[test]
    fn admin_snapshot_omits_secret_tokens_and_pins() {
        let mut registry = Registry::default();
        let (_, token, pin) = registry.invite(true).unwrap();
        let (_, request_token) = registry.request(None).unwrap();
        let session = registry.authorize();
        let serialized = serde_json::to_string(&registry.snapshot()).unwrap();
        assert!(!serialized.contains(&token));
        assert!(!serialized.contains(&pin));
        assert!(!serialized.contains(&request_token));
        assert!(!serialized.contains(&session));
        assert!(serialized.contains("authorized"));
    }

    proptest! {
        #[test]
        fn pin_parser_accepts_only_eight_ascii_digits(input in ".*") {
            let expected = input.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_digit() || (input.len() == 9 && index == 4 && byte == b'-')
            }) && (input.len() == 8 || (input.len() == 9 && input.as_bytes()[4] == b'-'));
            prop_assert_eq!(pin(&input).is_some(), expected);
        }
    }

    #[test]
    fn invite_is_single_use_across_pin_and_qr() {
        let mut registry = Registry::default();
        let (_, qr, pin) = registry.invite(false).unwrap();
        assert!(registry.redeem(&pin).is_ok());
        assert_eq!(registry.redeem(&qr), Err(Error::Unauthorized));
        let (_, qr, pin) = registry.invite(false).unwrap();
        assert!(registry.redeem(&qr).is_ok());
        assert_eq!(registry.redeem(&pin), Err(Error::Unauthorized));
        let (_, qr, _) = registry.invite(false).unwrap();
        registry.invites.last_mut().unwrap().expires = Some(Instant::now());
        assert_eq!(registry.redeem(&qr), Err(Error::Unauthorized));
    }

    #[test]
    fn reusable_invite_survives_redemption_until_revoked() {
        let mut registry = Registry::default();
        let (id, qr, pin) = registry.invite(true).unwrap();
        assert!(registry.redeem(&qr).is_ok());
        assert!(registry.redeem(&pin).is_ok());
        assert!(registry.revoke_invite(id));
        assert_eq!(registry.redeem(&qr), Err(Error::Unauthorized));
    }

    #[test]
    fn redemption_is_atomic_and_capacity_preserves_unspent_invite() {
        let sessions = Sessions::default();
        let (_, qr, _) = sessions.lock().invite(false).unwrap();
        std::thread::scope(|scope| {
            let a = scope.spawn(|| sessions.lock().redeem(&qr));
            let b = scope.spawn(|| sessions.lock().redeem(&qr));
            assert_ne!(a.join().unwrap().is_ok(), b.join().unwrap().is_ok());
        });
        let mut registry = sessions.lock();
        for _ in 1..MAX_SESSIONS {
            registry.authorize();
        }
        let (_, qr, _) = registry.invite(false).unwrap();
        assert_eq!(registry.redeem(&qr), Err(Error::Busy));
        let id = *registry.sessions.keys().next().unwrap();
        assert!(registry.end(id));
        assert!(registry.redeem(&qr).is_ok());
    }

    #[test]
    fn only_requesting_browser_can_claim_approval() {
        let mut registry = Registry::default();
        let (id, first) = registry.request(None).unwrap();
        let (_, second) = registry.request(None).unwrap();
        assert_eq!(registry.claim(&first), Err(Error::Pending));
        assert!(registry.resolve(id, true));
        assert_eq!(registry.claim(&second), Err(Error::Pending));
        assert!(registry.claim(&first).is_ok());
        assert_eq!(registry.claim(&first), Err(Error::Unauthorized));
    }

    #[test]
    fn failed_upgrade_does_not_extend_authorization() {
        let sessions = Sessions::default();
        let token = sessions.lock().authorize();
        let lease = sessions.attach(&token).unwrap();
        assert!(matches!(sessions.attach(&token), Err(Error::Busy)));
        sessions.lock().sessions.get_mut(&lease.id).unwrap().expires = Some(Instant::now());
        drop(lease);
        assert!(matches!(sessions.attach(&token), Err(Error::Unauthorized)));
    }

    async fn wait_drained(sessions: &Sessions) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !sessions.lock().drained() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn reconnect_reuses_worker_and_end_releases_it() {
        let sessions = Sessions::default();
        let token = sessions.lock().authorize();
        let mut first = sessions.attach(&token).unwrap();
        let (updates, observed) = std::sync::mpsc::channel();
        let mut ready = first
            .start_with(move |mut rx, started| {
                started.send(Ok(())).unwrap();
                while let Ok(changed) = rx.has_changed() {
                    if changed {
                        let input = rx.borrow_and_update();
                        updates
                            .send((input.connection, input.frame.is_some()))
                            .unwrap();
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            })
            .unwrap();
        ready.wait_for(|value| *value).await.unwrap();
        first.publish(crate::input::InputFrame::default());
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(1)).unwrap(),
            (1, true)
        );
        drop(first);
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(1)).unwrap(),
            (1, false)
        );
        let mut second = sessions.attach(&token).unwrap();
        let ready = second
            .start_with(|_, _| panic!("must reuse worker"))
            .unwrap();
        assert!(*ready.borrow());
        second.publish(crate::input::InputFrame::default());
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(1)).unwrap(),
            (2, true)
        );
        assert!(sessions.lock().end_token(&token));
        assert!(second.cancelled.changed().await.is_err());
        wait_drained(&sessions).await;
        assert!(matches!(sessions.attach(&token), Err(Error::Unauthorized)));
    }

    #[tokio::test]
    async fn failed_device_start_releases_capacity() {
        let sessions = Sessions::default();
        let token = sessions.lock().authorize();
        let mut lease = sessions.attach(&token).unwrap();
        let mut ready = lease
            .start_with(|_, started| {
                started.send(Err("fake start error".into())).unwrap();
                Err("fake start error".into())
            })
            .unwrap();
        assert!(ready.wait_for(|ready| *ready).await.is_err());
        wait_drained(&sessions).await;
    }
}
