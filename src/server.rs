//! HTTPS-reverse-proxy-facing, origin-restricted single-session web controller.
use std::{
    env,
    fmt::Write as _,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use rand::random;
use serde::Deserialize;
use subtle::ConstantTimeEq;
use tokio::{
    net::TcpListener,
    sync::{oneshot, watch},
};

use crate::{device::run_xbox_owner, input::parse_frame};

#[derive(Clone)]
struct ServiceState {
    origin: Arc<str>,
    pair_code: [u8; 8],
    access: Arc<Mutex<Access>>,
    active: Arc<AtomicBool>,
}

struct Access {
    session: Option<String>,
    failures: u8,
    window_started: Instant,
}

impl Default for Access {
    fn default() -> Self {
        Self {
            session: None,
            failures: 0,
            window_started: Instant::now(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairRequest {
    code: String,
}

fn origin_matches(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin == expected)
}

fn cookie_session(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| part.trim().strip_prefix("wc_session="))
}

fn token_matches(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).unwrap_u8() == 1
}

fn normalize_pair_code(code: &str) -> Option<[u8; 8]> {
    let bytes = code.as_bytes();
    let digits: [u8; 8] = if matches!(bytes, [_, _, _, _, b'-', _, _, _, _]) {
        let mut digits = [0; 8];
        digits[..4].copy_from_slice(&bytes[..4]);
        digits[4..].copy_from_slice(&bytes[5..]);
        digits
    } else {
        bytes.try_into().ok()?
    };
    digits.iter().all(u8::is_ascii_digit).then_some(digits)
}

fn random_session() -> String {
    let bytes: [u8; 32] = random();
    let mut output = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

async fn pair(
    State(state): State<ServiceState>,
    headers: HeaderMap,
    Json(request): Json<PairRequest>,
) -> Response {
    if !origin_matches(&headers, &state.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let mut access = state.access.lock().unwrap_or_else(PoisonError::into_inner);
    if access.window_started.elapsed() >= Duration::from_secs(60) {
        access.window_started = Instant::now();
        access.failures = 0;
    }
    if access.failures >= 5 {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    if normalize_pair_code(&request.code)
        .is_none_or(|digits| digits.ct_eq(&state.pair_code).unwrap_u8() != 1)
    {
        access.failures += 1;
        return StatusCode::UNAUTHORIZED.into_response();
    }
    access.failures = 0;
    let token = random_session();
    access.session = Some(token.clone());
    (
        StatusCode::NO_CONTENT,
        [(
            header::SET_COOKIE,
            format!("wc_session={token}; HttpOnly; Secure; SameSite=Strict; Path=/"),
        )],
    )
        .into_response()
}

async fn upgrade(
    ws: WebSocketUpgrade,
    State(state): State<ServiceState>,
    headers: HeaderMap,
) -> Response {
    if !origin_matches(&headers, &state.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let authorized = {
        let access = state.access.lock().unwrap_or_else(PoisonError::into_inner);
        access
            .session
            .as_deref()
            .zip(cookie_session(&headers))
            .is_some_and(|(expected, provided)| token_matches(expected, provided))
    };
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(slot) = SocketSlot::acquire(state.active) else {
        return StatusCode::CONFLICT.into_response();
    };
    ws.max_frame_size(512)
        .max_message_size(512)
        .on_upgrade(move |socket| serve_socket(socket, slot))
        .into_response()
}

struct SocketSlot(Arc<AtomicBool>);

impl SocketSlot {
    fn acquire(active: Arc<AtomicBool>) -> Option<Self> {
        if active.swap(true, Ordering::AcqRel) {
            return None;
        }
        Some(Self(active))
    }
}

impl Drop for SocketSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

async fn serve_socket(mut socket: WebSocket, slot: SocketSlot) {
    let (tx, rx) = watch::channel(None);
    let (startup_tx, startup_rx) = oneshot::channel();
    let worker = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        run_xbox_owner(rx, startup_tx)
    });

    if matches!(startup_rx.await, Ok(Ok(()))) {
        let mut latest_sequence = None;
        while let Some(message) = socket.recv().await {
            match message {
                Ok(Message::Text(raw)) => {
                    let Ok(frame) = parse_frame(raw.as_bytes()) else {
                        break;
                    };
                    if latest_sequence.is_some_and(|old| frame.seq <= old) {
                        break;
                    }
                    latest_sequence = Some(frame.seq);
                    tx.send_replace(Some(frame));
                }
                Ok(Message::Close(_) | Message::Binary(_)) | Err(_) => break,
                Ok(_) => {}
            }
        }
    } else {
        let _ = socket
            .send(Message::Text("virtual controller creation failed".into()))
            .await;
    }

    drop(tx);
    match worker.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("virtual controller closed with error: {error}"),
        Err(error) => eprintln!("virtual controller task failed: {error}"),
    }
}

fn security_headers(content_type: &'static str) -> [(header::HeaderName, &'static str); 4] {
    [
        (header::CONTENT_TYPE, content_type),
        (
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::CACHE_CONTROL, "no-store"),
    ]
}

async fn index() -> impl IntoResponse {
    (
        security_headers("text/html; charset=utf-8"),
        Html(include_str!("../assets/index.html")),
    )
}
async fn app_js() -> impl IntoResponse {
    (
        security_headers("text/javascript; charset=utf-8"),
        include_str!("../assets/app.mjs"),
    )
}
async fn model_js() -> impl IntoResponse {
    (
        security_headers("text/javascript; charset=utf-8"),
        include_str!("../assets/input-model.mjs"),
    )
}
async fn styles() -> impl IntoResponse {
    (
        security_headers("text/css; charset=utf-8"),
        include_str!("../assets/style.css"),
    )
}

/// Start the M1 service with explicitly configured address, HTTPS origin and
/// operator pairing code. TLS is terminated by a trusted external proxy.
///
/// # Errors
/// Missing or invalid configuration and bind/server failures are fatal.
pub async fn serve() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bind = env::var("WEB_CONTROLLER_BIND")?;
    let origin = env::var("WEB_CONTROLLER_ORIGIN")?;
    let pair_code = env::var("WEB_CONTROLLER_PAIR_CODE")?;
    if !origin.starts_with("https://") || origin.len() < 12 {
        return Err("WEB_CONTROLLER_ORIGIN must be an HTTPS origin".into());
    }
    let pair_code = normalize_pair_code(&pair_code).ok_or(
        "WEB_CONTROLLER_PAIR_CODE must contain eight ASCII digits, optionally grouped 4-4",
    )?;

    let state = ServiceState {
        origin: Arc::from(origin),
        pair_code,
        access: Arc::new(Mutex::new(Access::default())),
        active: Arc::new(AtomicBool::new(false)),
    };
    let app = Router::new()
        .route("/", get(index))
        .route("/app.mjs", get(app_js))
        .route("/input-model.mjs", get(model_js))
        .route("/style.css", get(styles))
        .route("/pair", post(pair))
        .route("/input", get(upgrade))
        .layer(DefaultBodyLimit::max(512))
        .with_state(state);
    let listener = TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use axum::http::{HeaderMap, HeaderValue, header};

    use proptest::prelude::*;

    use super::{
        SocketSlot, cookie_session, normalize_pair_code, origin_matches, random_session,
        token_matches,
    };

    proptest! {
        #[test]
        fn pair_code_accepts_grouped_and_plain_digits(first in 0..10_000u16, last in 0..10_000u16) {
            let plain = format!("{first:04}{last:04}");
            let grouped = format!("{first:04}-{last:04}");
            let normalized = normalize_pair_code(&plain).unwrap();
            prop_assert_eq!(normalized.as_slice(), plain.as_bytes());
            prop_assert_eq!(normalize_pair_code(&grouped), normalize_pair_code(&plain));
            let padded = format!(" {grouped}");
            prop_assert!(normalize_pair_code(&padded).is_none());
        }
    }

    #[test]
    fn rejects_non_ascii_and_misplaced_separators() {
        for invalid in [
            "１２３４５６７８",
            "123-45678",
            "1234 5678",
            "1234567",
            "1234567a",
        ] {
            assert!(normalize_pair_code(invalid).is_none());
        }
    }

    #[test]
    fn dropped_upgrade_releases_socket_slot() {
        let active = Arc::new(AtomicBool::new(false));
        let slot = SocketSlot::acquire(Arc::clone(&active)).unwrap();
        assert!(SocketSlot::acquire(Arc::clone(&active)).is_none());
        drop(slot);
        assert!(SocketSlot::acquire(active).is_some());
    }

    #[test]
    fn token_matching_and_cookie_parsing() {
        assert!(token_matches("a", "a"));
        assert!(!token_matches("a", "b"));
        assert!(!token_matches("a", "aa"));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("other=1; wc_session=abc"),
        );
        assert_eq!(cookie_session(&headers), Some("abc"));
    }

    #[test]
    fn origin_is_exact_not_prefix_based() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://example.test"),
        );
        assert!(origin_matches(&headers, "https://example.test"));
        assert!(!origin_matches(&headers, "https://example.test.evil"));
    }

    #[test]
    fn randomized_sessions_are_not_identical() {
        let first = random_session();
        assert_eq!(first.len(), 64);
        assert_ne!(first, random_session());
    }
}
