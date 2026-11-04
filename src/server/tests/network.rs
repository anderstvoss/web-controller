//! Actual HTTP/WebSocket transport tests with an observable fake device owner.
//! No browser, TLS provisioning, kernel device or physical phone is needed.
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::http::HeaderValue;
use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot, watch},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message as ClientMessage, client::IntoClientRequest},
};

use super::{ServiceState, router};
use crate::{device::DeviceInput, input::InputFrame, sessions::Sessions};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

static OBSERVERS: Mutex<Option<mpsc::UnboundedSender<watch::Receiver<DeviceInput>>>> =
    Mutex::new(None);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static FAIL_NEXT: AtomicBool = AtomicBool::new(false);

fn fake_owner(
    snapshots: watch::Receiver<DeviceInput>,
    startup: oneshot::Sender<Result<(), String>>,
) -> Result<(), String> {
    if FAIL_NEXT.swap(false, Ordering::SeqCst) {
        let _ = startup.send(Err("synthetic startup failure".into()));
        return Err("synthetic startup failure".into());
    }
    LIVE.fetch_add(1, Ordering::SeqCst);
    OBSERVERS
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .send(snapshots.clone())
        .unwrap();
    let _ = startup.send(Ok(()));
    while snapshots.has_changed().is_ok() {
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(snapshots);
    LIVE.fetch_sub(1, Ordering::SeqCst);
    Ok(())
}

struct Host {
    address: String,
    sessions: Sessions,
}

impl Drop for Host {
    fn drop(&mut self) {
        self.sessions.lock().shutdown();
    }
}

impl Host {
    async fn post(&self, path: &str, cookie: &str, body: &str) -> (u16, String, String) {
        let mut socket = TcpStream::connect(&self.address).await.unwrap();
        socket
            .write_all(
                format!(
                    "POST {path} HTTP/1.1\r\nHost: example.test\r\nOrigin: https://example.test\r\nCookie: {cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), socket.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let response = String::from_utf8(bytes).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        let cookie = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(": ")?;
                name.eq_ignore_ascii_case("set-cookie")
                    .then(|| value.split(';').next().unwrap().to_owned())
            })
            .unwrap_or_default();
        (status, cookie, body.into())
    }

    async fn pair(&self, credential: &str) -> String {
        let body = serde_json::json!({ "code": credential }).to_string();
        let (status, cookie, _) = self.post("/pair", "", &body).await;
        assert_eq!(status, 204);
        cookie
    }

    async fn socket(&self, cookie: &str) -> Socket {
        let mut request = format!("ws://{}/input", self.address)
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", HeaderValue::from_static("https://example.test"));
        request
            .headers_mut()
            .insert("cookie", HeaderValue::from_str(cookie).unwrap());
        let (mut socket, _) = connect_async(request).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            ClientMessage::Text("ready".into())
        );
        socket
    }
}

async fn send(socket: &mut Socket, seq: u64, button: usize) {
    let mut frame = InputFrame {
        seq,
        ..InputFrame::default()
    };
    frame.buttons[button] = true;
    socket
        .send(ClientMessage::Text(
            serde_json::to_string(&frame).unwrap().into(),
        ))
        .await
        .unwrap();
}

async fn observed(
    snapshots: &mut watch::Receiver<DeviceInput>,
    predicate: impl Fn(&DeviceInput) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(7), snapshots.wait_for(predicate))
        .await
        .unwrap()
        .unwrap();
}

async fn live_count(count: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while LIVE.load(Ordering::SeqCst) != count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn test_state(sessions: Sessions) -> ServiceState {
    ServiceState {
        origin: "https://example.test".into(),
        sessions,
        assets: std::sync::Arc::new(super::Assets {
            index: String::new(),
            app: String::new(),
            model: String::new(),
            styles: String::new(),
        }),
    }
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one transport scenario shares the same fake owner and connected sessions"
)]
async fn local_clients_reconnect_expire_and_shutdown_independently() {
    let (observers, mut devices) = mpsc::unbounded_channel();
    *OBSERVERS.lock().unwrap() = Some(observers);
    let sessions = Sessions::default();
    sessions.lock().owner = Some(fake_owner);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = Host {
        address: listener.local_addr().unwrap().to_string(),
        sessions: sessions.clone(),
    };
    let (stop, stopped) = oneshot::channel();
    let stop_sessions = sessions.clone();
    let app = router(test_state(sessions));
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
                stop_sessions.lock().shutdown();
            })
            .await
            .unwrap();
    });

    let (_, qr, pin) = host.sessions.lock().invite(true).unwrap();
    let a_cookie = host.pair(&qr).await;
    let b_cookie = host.pair(&pin).await;
    let mut a = host.socket(&a_cookie).await;
    let mut first = devices.recv().await.unwrap();
    let mut b = host.socket(&b_cookie).await;
    let mut second = devices.recv().await.unwrap();
    live_count(2).await;

    send(&mut a, 1, 0).await;
    send(&mut b, 1, 1).await;
    observed(&mut first, |value| {
        value.frame.as_ref().is_some_and(|frame| frame.buttons[0])
    })
    .await;
    observed(&mut second, |value| {
        value.frame.as_ref().is_some_and(|frame| frame.buttons[1])
    })
    .await;
    assert!(!first.borrow().frame.as_ref().unwrap().buttons[1]);
    assert!(!second.borrow().frame.as_ref().unwrap().buttons[0]);

    // Abrupt TCP loss, without a WebSocket close frame.
    drop(a);
    observed(&mut first, |value| value.frame.is_none()).await;
    let mut a = host.socket(&a_cookie).await;
    send(&mut a, 1, 2).await;
    observed(&mut first, |value| {
        value.connection == 2 && value.frame.as_ref().is_some_and(|frame| frame.buttons[2])
    })
    .await;
    assert!(devices.try_recv().is_err());

    // A connected but silent transport must also release input. Keep B active.
    let heartbeat = tokio::spawn(async move {
        let mut seq = 1;
        loop {
            seq += 1;
            send(&mut b, seq, 1).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    observed(&mut first, |value| value.frame.is_none()).await;
    assert!(second.borrow().frame.as_ref().unwrap().buttons[1]);
    drop(a);

    // Wait out the advertised grace using real service deadlines.
    tokio::time::sleep(Duration::from_millis(15_100)).await;
    assert_eq!(host.post("/session", &a_cookie, "").await.0, 401);
    live_count(1).await;
    assert_eq!(host.post("/session", &b_cookie, "").await.0, 204);
    heartbeat.abort();
    let _ = heartbeat.await;
    observed(&mut second, |value| value.frame.is_none()).await;

    // The reusable invite authorizes a fresh device after grace expiry.
    assert_eq!(host.post("/session/end", &b_cookie, "").await.0, 204);
    live_count(0).await;
    let c_cookie = host.pair(&pin).await;
    let mut c = host.socket(&c_cookie).await;
    let mut third = devices.recv().await.unwrap();

    // A replayed sequence closes only its own session transport.
    send(&mut c, 1, 0).await;
    observed(&mut third, |value| value.frame.is_some()).await;
    send(&mut c, 1, 0).await;
    observed(&mut third, |value| value.frame.is_none()).await;
    drop(c);

    // Flooding one transport closes it without ending the retained device.
    let mut flood = host.socket(&c_cookie).await;
    for seq in 1..200 {
        let frame = InputFrame {
            seq,
            ..InputFrame::default()
        };
        if flood
            .send(ClientMessage::Text(
                serde_json::to_string(&frame).unwrap().into(),
            ))
            .await
            .is_err()
        {
            break;
        }
    }
    let closed = tokio::time::timeout(Duration::from_secs(3), flood.next())
        .await
        .unwrap();
    assert!(!matches!(closed, Some(Ok(ClientMessage::Text(_)))));
    observed(&mut third, |value| value.frame.is_none()).await;
    drop(flood);

    // Startup failure must cancel the socket and recover its capacity.
    FAIL_NEXT.store(true, Ordering::SeqCst);
    let d_cookie = host.pair(&pin).await;
    let mut request = format!("ws://{}/input", host.address)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_static("https://example.test"));
    request
        .headers_mut()
        .insert("cookie", HeaderValue::from_str(&d_cookie).unwrap());
    let (mut failed, _) = connect_async(request).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), failed.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        ClientMessage::Text("device unavailable".into())
    );
    live_count(1).await;
    assert_eq!(host.post("/session", &d_cookie, "").await.0, 401);

    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap();
    live_count(0).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !host.sessions.lock().drained() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    *OBSERVERS.lock().unwrap() = None;
}
