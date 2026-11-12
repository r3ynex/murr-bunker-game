use crate::model::Session;
use serde_json::{json, Value};
use std::io::{self, Read};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const WORKER_HOST: &str = "murr-bunker-api.r3ynex-error.workers.dev";

fn prefer_worker_ipv4(netloc: &str, addresses: &mut [SocketAddr]) {
    // Some Windows networks advertise IPv6 but time out when using it for this Worker.
    if netloc
        .rsplit_once(':')
        .is_some_and(|(host, _)| host.eq_ignore_ascii_case(WORKER_HOST))
    {
        addresses.sort_by_key(SocketAddr::is_ipv6);
    }
}

fn resolve_room_host(netloc: &str) -> io::Result<Vec<SocketAddr>> {
    let mut addresses: Vec<_> = netloc.to_socket_addrs()?.collect();
    prefer_worker_ipv4(netloc, &mut addresses);
    Ok(addresses)
}

fn room_agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .resolver(resolve_room_host)
        .timeout_connect(Duration::from_secs(8))
        .timeout(timeout)
        .build()
}

pub fn request_room(
    base_url: String,
    nickname: String,
    avatar: u8,
    code: Option<String>,
) -> Receiver<Result<Session, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let base = base_url.trim().trim_end_matches('/');
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            let _ = tx.send(Err(
                "URL сервера должен начинаться с http:// или https://".into()
            ));
            return;
        }
        let url = match code {
            Some(code) => format!("{base}/api/rooms/{code}/join"),
            None => format!("{base}/api/rooms"),
        };
        let agent = room_agent(Duration::from_secs(20));
        let response = agent
            .post(&url)
            .send_json(json!({ "name": nickname, "avatar": avatar }));
        let result: Result<Session, String> = match response {
            Ok(resp) => resp.into_json::<Session>().map_err(|err| err.to_string()),
            Err(ureq::Error::Status(code, resp)) => {
                let error = resp
                    .into_json::<Value>()
                    .ok()
                    .and_then(|v| v.get("error")?.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("Сервер ответил кодом {code}"));
                Err(error)
            }
            Err(err) => Err(format!("Не удалось связаться с сервером: {err}")),
        };
        let _ = tx.send(result);
    });
    rx
}

pub enum WsEvent {
    Opened,
    Message(String),
    Closed,
    Error(String),
}

pub type WsSender = mpsc::Sender<String>;

pub fn connect(
    base_url: &str,
    session: &Session,
    wake_up: impl Fn() + Send + 'static,
) -> Result<(WsSender, Receiver<WsEvent>), String> {
    let base = base_url.trim().trim_end_matches('/');
    let url = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        return Err("Некорректный адрес сервера".into());
    };
    // The token is opaque. Do not include this URL in any diagnostic.
    let url = format!(
        "{url}/api/rooms/{}/ws?token={}",
        session.code, session.token
    );
    let (events, receiver) = mpsc::channel();
    let (sender, outgoing) = mpsc::channel();
    thread::Builder::new()
        .name("room-websocket".into())
        .spawn(move || {
            let notify = |event| {
                let delivered = events.send(event).is_ok();
                if delivered {
                    wake_up();
                }
                delivered
            };
            let event = match run_websocket(&url, &outgoing, &notify) {
                Ok(()) => WsEvent::Closed,
                Err(error) => WsEvent::Error(error),
            };
            notify(event);
        })
        .map_err(|error| format!("Не удалось запустить WebSocket: {error}"))?;
    Ok((sender, receiver))
}

fn run_websocket(
    url: &str,
    outgoing: &Receiver<String>,
    notify: &impl Fn(WsEvent) -> bool,
) -> Result<(), String> {
    use tungstenite::{http::Uri, stream::MaybeTlsStream, Error, Message};

    let uri: Uri = url.parse().map_err(|_| "Некорректный адрес WebSocket")?;
    let host = uri.host().ok_or("В адресе WebSocket нет имени сервера")?;
    let port = uri
        .port_u16()
        .unwrap_or(if uri.scheme_str() == Some("wss") {
            443
        } else {
            80
        });
    let addresses = resolve_room_host(&format!("{host}:{port}"))
        .map_err(|error| format!("Не удалось найти адрес WebSocket: {error}"))?;
    let stream = addresses
        .into_iter()
        .find_map(|address| TcpStream::connect_timeout(&address, Duration::from_secs(3)).ok())
        .ok_or("Сервер не принял TCP-подключение WebSocket")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(6)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(6)))
        .map_err(|error| error.to_string())?;
    stream
        .set_nodelay(true)
        .map_err(|error| error.to_string())?;

    // Keep the original URL for the Host header and certificate verification.
    let (mut socket, _) = tungstenite::client_tls(url, stream).map_err(|error| match error {
        tungstenite::HandshakeError::Failure(Error::Http(response)) => {
            format!("Сервер отклонил WebSocket: HTTP {}", response.status())
        }
        tungstenite::HandshakeError::Failure(Error::Io(error)) => {
            format!("Сервер не завершил подключение WebSocket: {error}")
        }
        _ => "Не удалось завершить рукопожатие WebSocket".into(),
    })?;
    let tcp = match socket.get_mut() {
        MaybeTlsStream::Plain(tcp) => tcp,
        MaybeTlsStream::Rustls(stream) => &mut stream.sock,
        _ => return Err("Неподдерживаемый тип соединения WebSocket".into()),
    };
    tcp.set_read_timeout(Some(Duration::from_millis(40)))
        .map_err(|error| error.to_string())?;
    if !notify(WsEvent::Opened) {
        return Ok(());
    }

    loop {
        for _ in 0..32 {
            match outgoing.try_recv() {
                Ok(text) => socket
                    .send(Message::Text(text))
                    .map_err(|error| error.to_string())?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    return Ok(());
                }
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                if !notify(WsEvent::Message(text)) {
                    return Ok(());
                }
            }
            Ok(Message::Close(_)) => {
                let _ = socket.flush();
                return Ok(());
            }
            Ok(_) => {}
            Err(Error::ConnectionClosed | Error::AlreadyClosed) => return Ok(()),
            Err(Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(format!("Ошибка WebSocket: {error}")),
        }
    }
}

// ZIPs are automatically shared by the host through the same Cloudflare room.
// Authentication uses a bearer header; room tokens are never included in logs.
pub fn upload_bundle(
    base_url: String,
    session: Session,
    bytes: Vec<u8>,
) -> Receiver<Result<String, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let url = format!(
            "{}/api/rooms/{}/bundle",
            base_url.trim().trim_end_matches('/'),
            session.code
        );
        let response = room_agent(Duration::from_secs(25))
            .post(&url)
            .set("authorization", &format!("Bearer {}", session.token))
            .set("content-type", "application/zip")
            .send_bytes(&bytes);
        let result = match response {
            Ok(r) => r
                .into_json::<Value>()
                .map(|v| {
                    v.get("packName")
                        .and_then(Value::as_str)
                        .unwrap_or("ZIP")
                        .to_owned()
                })
                .map_err(|e| e.to_string()),
            Err(ureq::Error::Status(code, r)) => {
                let reason = r
                    .into_json::<Value>()
                    .ok()
                    .and_then(|v| v.get("error")?.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("HTTP {code}"));
                Err(reason)
            }
            Err(e) => Err(e.to_string()),
        };
        let _ = tx.send(result);
    });
    rx
}

pub fn download_background(
    base_url: String,
    session: Session,
) -> Receiver<Result<Vec<u8>, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let url = format!(
            "{}/api/rooms/{}/background",
            base_url.trim().trim_end_matches('/'),
            session.code
        );
        let response = room_agent(Duration::from_secs(25))
            .get(&url)
            .set("authorization", &format!("Bearer {}", session.token))
            .call();
        let result = match response {
            Ok(r) => {
                let mut bytes = Vec::new();
                r.into_reader()
                    .take(6_000_001)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())
                    .and_then(|_| {
                        if bytes.len() > 6_000_000 {
                            Err("Изображение больше 6 МБ".to_owned())
                        } else {
                            Ok(bytes)
                        }
                    })
            }
            Err(ureq::Error::Status(code, _)) => Err(format!("Фон недоступен: HTTP {code}")),
            Err(e) => Err(e.to_string()),
        };
        let _ = tx.send(result);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    #[test]
    fn worker_prefers_ipv4_but_keeps_ipv6_and_custom_host_order() {
        let ipv6: SocketAddr = "[2001:db8::1]:443".parse().unwrap();
        let ipv4: SocketAddr = "192.0.2.1:443".parse().unwrap();
        let mut addresses = [ipv6, ipv4];
        prefer_worker_ipv4("custom.example:443", &mut addresses);
        assert_eq!(addresses, [ipv6, ipv4]);
        prefer_worker_ipv4(&format!("{WORKER_HOST}:443"), &mut addresses);
        assert_eq!(addresses, [ipv4, ipv6]);
        let mut ipv6_only = [ipv6];
        prefer_worker_ipv4(&format!("{WORKER_HOST}:443"), &mut ipv6_only);
        assert_eq!(ipv6_only, [ipv6]);
    }

    #[test]
    fn websocket_delivers_initial_state_and_sends_commands() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            tcp.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut socket = tungstenite::accept(tcp).unwrap();
            socket
                .send(tungstenite::Message::Text("initial state".into()))
                .unwrap();
            assert_eq!(socket.read().unwrap().into_text().unwrap(), "command");
            socket.close(None).unwrap();
        });
        let session = Session {
            code: "TESTROOM".into(),
            player_id: "test".into(),
            token: "test-token".into(),
        };
        let (sender, receiver) = connect(&format!("http://{address}"), &session, || {}).unwrap();
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
            WsEvent::Opened
        ));
        assert!(
            matches!(receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
            WsEvent::Message(message) if message == "initial state")
        );
        sender.send("command".into()).unwrap();
        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
            WsEvent::Closed
        ));
        server.join().unwrap();
    }

    #[test]
    fn https_room_connection_starts_a_tls_handshake() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let session = Session {
            code: "TESTROOM".into(),
            player_id: "test-player".into(),
            token: "test-token".into(),
        };
        let (_sender, receiver) = connect(
            &format!("https://{}", listener.local_addr().unwrap()),
            &session,
            || {},
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut header = [0; 5];
                    stream.read_exact(&mut header).expect("TLS record header");
                    assert_eq!(
                        header[0], 0x16,
                        "the client must start with a TLS handshake"
                    );
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Ok(WsEvent::Error(error)) = receiver.try_recv() {
                        panic!("WebSocket failed before the handshake: {error}");
                    }
                    assert!(Instant::now() < deadline, "WebSocket never connected");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("could not accept local TLS connection: {error}"),
            }
        }
    }
}
