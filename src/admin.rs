//! Opt-in loopback administration plane; separate listener and credentials.
//! Public phone cookies have no authority here.
use std::{env, fs, net::SocketAddr, path::Path, sync::Arc};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as RoutePath, State},
    http::{HeaderMap, StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, task::JoinHandle};

use crate::sessions::{Error as SessionError, Sessions, matches};

#[derive(Clone)]
struct AdminState {
    sessions: Sessions,
    secret: Arc<str>,
    origin: Arc<str>,
    player_origin: Arc<str>,
    assets: Arc<AdminAssets>,
}

struct AdminAssets {
    index: String,
    app: String,
    styles: String,
}

impl AdminAssets {
    fn load(root: &Path) -> std::io::Result<Self> {
        let read = |name| {
            fs::read_to_string(root.join(name)).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("admin asset {name} missing; run npm ci and npm run build in web/: {error}"),
                )
            })
        };
        Ok(Self {
            index: read("admin.html")?,
            app: read("admin.mjs")?,
            styles: read("admin.css")?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    exposure_enabled: bool,
    join_requests_enabled: bool,
    max_sessions: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueInvite {
    reusable: bool,
}

#[derive(Serialize)]
struct IssuedInvite {
    id: u64,
    pin: String,
    url: String,
    reusable: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolution {
    approved: bool,
}

fn origin_matches(headers: &HeaderMap, expected: &str) -> bool {
    let mut origins = headers.get_all(header::ORIGIN).iter();
    origins.next().and_then(|value| value.to_str().ok()) == Some(expected)
        && origins.next().is_none()
}

fn authenticated(headers: &HeaderMap, state: &AdminState) -> bool {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let provided = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    values.next().is_none() && provided.is_some_and(|provided| matches(&state.secret, provided))
}

fn permit(headers: &HeaderMap, state: &AdminState, write: bool) -> Result<(), StatusCode> {
    if !authenticated(headers, state) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if write && !origin_matches(headers, &state.origin) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}

fn no_store<T: IntoResponse>(value: T) -> Response {
    let mut response = value.into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        header::HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    response
}

fn static_response<T: IntoResponse>(value: T) -> Response {
    let mut response = no_store(value);
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        header::HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
        ),
    );
    response
}

fn session_error(error: SessionError) -> StatusCode {
    match error {
        SessionError::Busy => StatusCode::CONFLICT,
        SessionError::Limited => StatusCode::TOO_MANY_REQUESTS,
        SessionError::Unauthorized => StatusCode::UNAUTHORIZED,
        SessionError::Pending => StatusCode::ACCEPTED,
    }
}

async fn page(State(state): State<AdminState>) -> Response {
    static_response(Html(state.assets.index.clone()))
}

async fn script(State(state): State<AdminState>) -> Response {
    static_response((
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        state.assets.app.clone(),
    ))
}

async fn css(State(state): State<AdminState>) -> Response {
    static_response((
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        state.assets.styles.clone(),
    ))
}

async fn snapshot(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Err(code) = permit(&headers, &state, false) {
        return no_store(code);
    }
    no_store(Json(state.sessions.lock().snapshot()))
}

async fn settings(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(request): Json<Settings>,
) -> Response {
    if let Err(code) = permit(&headers, &state, true) {
        return no_store(code);
    }
    let mut registry = state.sessions.lock();
    if !registry.set_policy(request.join_requests_enabled, request.max_sessions) {
        return no_store(StatusCode::BAD_REQUEST);
    }
    registry.set_exposure(request.exposure_enabled);
    no_store(Json(registry.snapshot()))
}

async fn issue(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(request): Json<IssueInvite>,
) -> Response {
    if let Err(code) = permit(&headers, &state, true) {
        return no_store(code);
    }
    let issued = state.sessions.lock().invite(request.reusable);
    match issued {
        Ok((id, token, pin)) => no_store((
            StatusCode::CREATED,
            Json(IssuedInvite {
                id,
                url: format!("{}/#invite={token}", state.player_origin),
                pin,
                reusable: request.reusable,
            }),
        )),
        Err(error) => no_store(session_error(error)),
    }
}

async fn revoke(
    State(state): State<AdminState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<u64>,
) -> Response {
    if let Err(code) = permit(&headers, &state, true) {
        return no_store(code);
    }
    let code = if state.sessions.lock().revoke_invite(id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    };
    no_store(code)
}

async fn kick(
    State(state): State<AdminState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<u64>,
) -> Response {
    if let Err(code) = permit(&headers, &state, true) {
        return no_store(code);
    }
    let code = if state.sessions.lock().end(id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    };
    no_store(code)
}

async fn resolve(
    State(state): State<AdminState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<u64>,
    Json(request): Json<Resolution>,
) -> Response {
    if let Err(code) = permit(&headers, &state, true) {
        return no_store(code);
    }
    let code = if state.sessions.lock().resolve(id, request.approved) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    };
    no_store(code)
}

fn router(state: AdminState) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/admin.mjs", get(script))
        .route("/admin.css", get(css))
        .route("/api/state", get(snapshot))
        .route("/api/settings", post(settings))
        .route("/api/invites", post(issue))
        .route("/api/invites/{id}/revoke", post(revoke))
        .route("/api/sessions/{id}/kick", post(kick))
        .route("/api/requests/{id}/resolve", post(resolve))
        .layer(DefaultBodyLimit::max(1024))
        .with_state(state)
}

fn validate_origin(origin: &str) -> Result<(), &'static str> {
    let uri: Uri = origin.parse().map_err(|_| "invalid administrator origin")?;
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri.authority().is_none()
        || uri.path_and_query().is_some_and(|part| part.as_str() != "/")
        || origin.ends_with('/')
        || origin.contains(['#', '?'])
        || uri.authority().is_some_and(|authority| authority.as_str().contains('@'))
    {
        return Err("admin origin must be a bare HTTP(S) origin");
    }
    Ok(())
}

/// Start an optional second listener. Requires a loopback bind and independent
/// high-entropy admin credential; never shares a player router or session cookie.
///
/// # Errors
/// Invalid operator configuration, permissions or listener binding fail startup.
pub(crate) async fn start(
    sessions: Sessions,
    player_origin: &str,
    player_port: u16,
    assets_root: &Path,
) -> Result<Option<JoinHandle<std::io::Result<()>>>, Box<dyn std::error::Error + Send + Sync>> {
    let Ok(bind) = env::var("WEB_CONTROLLER_ADMIN_BIND") else {
        return Ok(None);
    };
    let address: SocketAddr = bind.parse()?;
    if !address.ip().is_loopback() || address.port() == 0 || address.port() == player_port {
        return Err("admin listener must use a separate, nonzero loopback-only port".into());
    }
    let token = env::var("WEB_CONTROLLER_ADMIN_TOKEN")?;
    if !(32..=512).contains(&token.len()) || !token.is_ascii() || token.contains(char::is_whitespace)
    {
        return Err("WEB_CONTROLLER_ADMIN_TOKEN must be 32-512 non-whitespace ASCII characters".into());
    }
    let origin = env::var("WEB_CONTROLLER_ADMIN_ORIGIN")
        .unwrap_or_else(|_| format!("http://{address}"));
    validate_origin(&origin)?;
    let assets = Arc::new(AdminAssets::load(assets_root)?);
    let listener = TcpListener::bind(address).await?;
    let state = AdminState {
        sessions,
        secret: Arc::from(token),
        origin: Arc::from(origin),
        player_origin: Arc::from(player_origin),
        assets,
    };
    Ok(Some(tokio::spawn(async move {
        axum::serve(listener, router(state)).await
    })))
}

#[cfg(test)]
mod tests {
    use super::{AdminAssets, AdminState, authenticated, origin_matches, validate_origin};
    use crate::sessions::Sessions;
    use axum::http::{HeaderMap, HeaderValue, header};
    use std::sync::Arc;

    fn state() -> AdminState {
        AdminState {
            sessions: Sessions::default(),
            secret: Arc::from("long-test-credential-that-is-not-production-secret"),
            origin: Arc::from("https://example.test"),
            player_origin: Arc::from("https://player.example.test"),
            assets: Arc::new(AdminAssets {
                index: String::new(),
                app: String::new(),
                styles: String::new(),
            }),
        }
    }

    #[test]
    fn administrator_requires_exact_bearer_and_one_origin() {
        let state = state();
        let mut headers = HeaderMap::new();
        assert!(!authenticated(&headers, &state));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer long-test-credential-that-is-not-production-secret"),
        );
        assert!(authenticated(&headers, &state));
        headers.append(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer additional"),
        );
        assert!(!authenticated(&headers, &state));
        headers.remove(header::AUTHORIZATION);
        headers.insert(header::ORIGIN, HeaderValue::from_static("https://example.test"));
        assert!(origin_matches(&headers, &state.origin));
        headers.append(header::ORIGIN, HeaderValue::from_static("https://example.test"));
        assert!(!origin_matches(&headers, &state.origin));
    }

    #[test]
    fn invalid_admin_origins_fail_closed() {
        assert!(validate_origin("https://example.test").is_ok());
        assert!(validate_origin("https://example.test/path").is_err());
        assert!(validate_origin("https://name@example.test").is_err());
        assert!(validate_origin("https://example.test/").is_err());
    }
}
