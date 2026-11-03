//! HTTPS-reverse-proxy-facing, origin-restricted single-session web controller.
use std::{
    env,
    fmt::Write as _,
    sync::{
        Arc, Mutex,
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
use tokio::{net::TcpListener, sync::{oneshot, watch}};

use crate::{device::run_xbox_owner, input::parse_frame};

#[derive(Clone)]
struct ServiceState {
    origin: Arc<str>,
    pair_code: Arc<str>,
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

fn cookie_session<'a>(headers: &'a HeaderMap) -> Option<&'a str> {
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
    if request.code.len() > 256 {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let mut access = state.access.lock().unwrap_or_else(|e| e.into_inner());
    if access.window_started.elapsed() >= Duration::from_secs(60) {
        access.window_started = Instant::now();
        access.failures = 0;
    }
    if access.failures >= 5 {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    if !token_matches(&request.code, &state.pair_code) {
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
        let access = state.access.lock().unwrap_or_else(|e| e.into_inner());
        access
            .session
            .as_deref()
            .zip(cookie_session(&headers))
            .is_some_and(|(expected, provided)| token_matches(expected, provided))
    };
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if state.active.swap(true, Ordering::AcqRel) {
        return StatusCode::CONFLICT.into_response();
    }
    ws.max_frame_size(512)
        .max_message_size(512)
        .on_upgrade(move |socket| serve_socket(socket, state))
        .into_response()
}

async fn serve_socket(mut socket: WebSocket, state: ServiceState) {
    let (tx, rx) = watch::channel(None);
    let (startup_tx, startup_rx) = oneshot::channel();
    let live = Arc::new(AtomicBool::new(true));
    let worker_live = Arc::clone(&live);
    let worker = tokio::task::spawn_blocking(move || run_xbox_owner(rx, worker_live, startup_tx));

    if !matches!(startup_rx.await, Ok(Ok(()))) {
        let _ = socket
            .send(Message::Text("virtual controller creation failed".into()))
            .await;
    } else {
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
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(Message::Binary(_)) => break,
                Ok(_) => {}
            }
        }
    }

    live.store(false, Ordering::Release);
    drop(tx);
    match worker.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("virtual controller closed with error: {error}"),
        Err(error) => eprintln!("virtual controller task failed: {error}"),
    }
    state.active.store(false, Ordering::Release);
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
    if pair_code.len() < 16 {
        return Err("WEB_CONTROLLER_PAIR_CODE must be at least 16 characters".into());
    }

    let state = ServiceState {
        origin: Arc::from(origin),
        pair_code: Arc::from(pair_code),
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
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::{cookie_session, origin_matches, random_session, token_matches};

    #[test]
    fn token_matching_and_cookie_parsing() {
        assert!(token_matches("a", "a"));
        assert!(!token_matches("a", "b"));
        assert!(!token_matches("a", "aa"));
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("other=1; wc_session=abc"));
        assert_eq!(cookie_session(&headers), Some("abc"));
    }

    #[test]
    fn origin_is_exact_not_prefix_based() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, HeaderValue::from_static("https://example.test"));
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
