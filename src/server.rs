//! HTTPS-reverse-proxy-facing, origin-restricted LAN controller service.
use std::{
    env,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use tokio::net::TcpListener;

use crate::{
    input::parse_frame,
    operator,
    sessions::{Error, Sessions, SocketLease},
};

const SESSION: &str = "wc_session";
const REQUEST: &str = "wc_request";
const FRAME_RATE: f64 = 240.0;
const BURST: f64 = 48.0;

#[derive(Clone)]
struct ServiceState {
    origin: Arc<str>,
    sessions: Sessions,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairRequest {
    code: String,
}

fn origin_matches(headers: &HeaderMap, expected: &str) -> bool {
    let mut origins = headers.get_all(header::ORIGIN).iter();
    origins.next().and_then(|value| value.to_str().ok()) == Some(expected)
        && origins.next().is_none()
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            (key == name).then_some(value)
        })
}

fn set_cookie(name: &str, value: &str, expiry: &str) -> String {
    format!("{name}={value}; HttpOnly; Secure; SameSite=Strict; Path=/{expiry}")
}

fn failure(error: Error) -> Response {
    match error {
        Error::Unauthorized => StatusCode::UNAUTHORIZED,
        Error::Pending => StatusCode::ACCEPTED,
        Error::Busy => StatusCode::CONFLICT,
        Error::Limited => StatusCode::TOO_MANY_REQUESTS,
    }
    .into_response()
}

fn authorized(token: &str) -> Response {
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, set_cookie(SESSION, token, ""))],
    )
        .into_response()
}

async fn pair(
    State(state): State<ServiceState>,
    headers: HeaderMap,
    Json(request): Json<PairRequest>,
) -> Response {
    if !origin_matches(&headers, &state.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut registry = state.sessions.lock();
    registry.expire();
    if cookie(&headers, SESSION).is_some_and(|token| registry.has_session(token)) {
        return StatusCode::CONFLICT.into_response();
    }
    registry
        .redeem(&request.code)
        .map_or_else(failure, |token| authorized(&token))
}

async fn request_join(State(state): State<ServiceState>, headers: HeaderMap) -> Response {
    if !origin_matches(&headers, &state.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut registry = state.sessions.lock();
    registry.expire();
    if cookie(&headers, SESSION).is_some_and(|token| registry.has_session(token)) {
        return StatusCode::CONFLICT.into_response();
    }
    match registry.request(cookie(&headers, REQUEST)) {
        Ok((id, token)) => (
            StatusCode::ACCEPTED,
            [
                (
                    header::SET_COOKIE,
                    set_cookie(REQUEST, &token, "; Max-Age=120"),
                ),
                (header::CACHE_CONTROL, "no-store".into()),
            ],
            id.to_string(),
        )
            .into_response(),
        Err(error) => failure(error),
    }
}

async fn claim_join(State(state): State<ServiceState>, headers: HeaderMap) -> Response {
    if !origin_matches(&headers, &state.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(token) = cookie(&headers, REQUEST) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let mut registry = state.sessions.lock();
    registry.expire();
    if cookie(&headers, SESSION).is_some_and(|token| registry.has_session(token)) {
        return StatusCode::CONFLICT.into_response();
    }
    registry
        .claim(token)
        .map_or_else(failure, |token| authorized(&token))
}

async fn session_status(State(state): State<ServiceState>, headers: HeaderMap) -> StatusCode {
    if !origin_matches(&headers, &state.origin) {
        return StatusCode::FORBIDDEN;
    }
    let mut registry = state.sessions.lock();
    registry.expire();
    if cookie(&headers, SESSION).is_some_and(|token| registry.has_session(token)) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::UNAUTHORIZED
    }
}

async fn end_session(State(state): State<ServiceState>, headers: HeaderMap) -> Response {
    if !origin_matches(&headers, &state.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(token) = cookie(&headers, SESSION) {
        state.sessions.lock().end_token(token);
    }
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, set_cookie(SESSION, "", "; Max-Age=0"))],
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
    let Some(token) = cookie(&headers, SESSION) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match state.sessions.attach(token) {
        Ok(lease) => ws
            .max_frame_size(512)
            .max_message_size(512)
            .on_upgrade(move |socket| serve_socket(socket, lease))
            .into_response(),
        Err(error) => failure(error),
    }
}

async fn serve_socket(mut socket: WebSocket, mut lease: SocketLease) {
    let mut cancelled = lease.cancelled.clone();
    let Some(mut ready) = lease.start() else {
        return;
    };
    let started = tokio::select! {
        result = ready.wait_for(|ready| *ready) => result.is_ok(),
        _ = cancelled.changed() => false,
    };
    if !started {
        let _ = tokio::time::timeout(
            Duration::from_secs(1),
            socket.send(Message::Text("device unavailable".into())),
        )
        .await;
        return;
    }

    if !matches!(
        tokio::time::timeout(
            Duration::from_secs(1),
            socket.send(Message::Text("ready".into()))
        )
        .await,
        Ok(Ok(()))
    ) {
        return;
    }
    let mut sequence = None;
    let mut budget = BURST;
    let mut last = Instant::now();
    loop {
        let message = tokio::select! {
            biased;
            _ = cancelled.changed() => break,
            result = tokio::time::timeout(Duration::from_secs(5), socket.recv()) => result.ok().flatten(),
        };
        let Some(Ok(message)) = message else {
            break;
        };
        let now = Instant::now();
        budget = (budget + now.duration_since(last).as_secs_f64() * FRAME_RATE).min(BURST);
        last = now;
        if budget < 1.0 {
            break;
        }
        budget -= 1.0;
        match message {
            Message::Text(raw) => {
                let Ok(frame) = parse_frame(raw.as_bytes()) else {
                    break;
                };
                if sequence.is_some_and(|old| frame.seq <= old) {
                    break;
                }
                sequence = Some(frame.seq);
                lease.publish(frame);
            }
            Message::Close(_) | Message::Binary(_) => break,
            _ => {}
        }
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

async fn shutdown(terminal: tokio::sync::oneshot::Receiver<()>) {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM listener");
        tokio::select! {
            _ = terminal => {}
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::select! {
        _ = terminal => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

/// Start the service with an explicitly configured bind address and HTTPS origin.
///
/// # Errors
/// Missing or invalid configuration and bind/server failures are fatal.
pub async fn serve() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bind = env::var("WEB_CONTROLLER_BIND")?;
    let origin = env::var("WEB_CONTROLLER_ORIGIN")?;
    let uri: Uri = origin.parse()?;
    if uri.scheme_str() != Some("https")
        || uri.authority().is_none()
        || uri
            .path_and_query()
            .is_some_and(|part| part.as_str() != "/")
        || origin.ends_with('/')
        || origin.contains(['#', '?'])
        || uri
            .authority()
            .is_some_and(|part| part.as_str().contains('@'))
    {
        return Err(
            "WEB_CONTROLLER_ORIGIN must be an HTTPS origin without a trailing slash".into(),
        );
    }
    let listener = TcpListener::bind(bind).await?;
    let sessions = Sessions::default();
    let state = ServiceState {
        origin: Arc::from(origin.as_str()),
        sessions: sessions.clone(),
    };
    let app = Router::new()
        .route("/", get(index))
        .route("/app.mjs", get(app_js))
        .route("/input-model.mjs", get(model_js))
        .route("/style.css", get(styles))
        .route("/pair", post(pair))
        .route("/join/request", post(request_join))
        .route("/join/claim", post(claim_join))
        .route("/session", post(session_status))
        .route("/session/end", post(end_session))
        .route("/input", get(upgrade))
        .layer(DefaultBodyLimit::max(512))
        .with_state(state);
    let terminal = operator::start(sessions.clone(), origin);
    let sweep_sessions = sessions.clone();
    let sweep = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            tick.tick().await;
            sweep_sessions.lock().expire();
        }
    });
    let stop_sessions = sessions.clone();
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown(terminal).await;
            stop_sessions.lock().shutdown();
        })
        .await;
    sweep.abort();
    sessions.lock().shutdown();
    while !sessions.lock().drained() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{cookie, origin_matches};
    use axum::http::{HeaderMap, HeaderValue, header};

    #[test]
    fn exact_origin_and_cookie_name() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://example.test"),
        );
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("other=1; wc_session=abc"),
        );
        assert!(origin_matches(&headers, "https://example.test"));
        assert!(!origin_matches(&headers, "https://example.test.evil"));
        assert_eq!(cookie(&headers, "wc_session"), Some("abc"));
        assert_eq!(cookie(&headers, "session"), None);
    }
}
