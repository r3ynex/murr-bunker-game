use crate::model::Room;
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const APPLICATION_ID: &str = "1572945936311656559";
const LARGE_IMAGE_KEY: &str = "logo";
const LARGE_IMAGE_TEXT: &str = "Murr Bunker";
const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;
const MAX_FRAME_BYTES: usize = 256 * 1024;
const RECONNECT_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presence {
    pub details: String,
    pub state: Option<String>,
}

impl Presence {
    pub fn from_room(room: Option<&Room>) -> Self {
        let Some(room) = room else {
            return Self {
                details: "В главном меню".into(),
                state: None,
            };
        };
        let is_host = room.you == room.host_id;
        match room.stage.as_str() {
            "lobby" => Self {
                details: if is_host {
                    "В лобби · Ведущий".into()
                } else {
                    "В лобби".into()
                },
                state: Some(format!(
                    "Игроков: {} · Мест: {}",
                    room.players.len(),
                    room.seats
                )),
            },
            "playing" => {
                let round = room.round.max(1);
                let alive = room.players.iter().filter(|p| p.active).count();
                let details = if room.vote_open {
                    format!("Голосование · Раунд {round}")
                } else {
                    format!("В бункере · Раунд {round}")
                };
                let state = if is_host {
                    format!("Ведущий · Живых: {alive} · Мест: {}", room.seats)
                } else {
                    format!("Живых: {alive} · Мест: {}", room.seats)
                };
                Self {
                    details,
                    state: Some(state),
                }
            }
            _ => {
                let alive = room.players.iter().filter(|p| p.active).count();
                Self {
                    details: "Партия завершена".into(),
                    state: Some(format!("Выживших: {alive} / Мест: {}", room.seats)),
                }
            }
        }
    }
}

enum Command {
    Update(Presence),
    SetEnabled(bool),
    Shutdown,
}

pub struct DiscordRpc {
    tx: Sender<Command>,
    enabled: bool,
    last: Option<Presence>,
}

impl DiscordRpc {
    pub fn new(enabled: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        std::thread::spawn(move || worker_loop(rx, enabled, started_at));
        let mut rpc = Self {
            tx,
            enabled,
            last: None,
        };
        rpc.sync(None);
        rpc
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            let _ = self.tx.send(Command::SetEnabled(enabled));
        }
    }

    pub fn sync(&mut self, room: Option<&Room>) {
        let presence = Presence::from_room(room);
        if self.last.as_ref() != Some(&presence) {
            self.last = Some(presence.clone());
            let _ = self.tx.send(Command::Update(presence));
        }
    }
}

impl Drop for DiscordRpc {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
    }
}

fn worker_loop(rx: Receiver<Command>, mut enabled: bool, started_at: u64) {
    let mut socket: Option<IpcSocket> = None;
    let mut current: Option<Presence> = None;
    let mut dirty = false;
    let mut nonce: u64 = 1;
    let mut last_attempt: Option<Instant> = None;

    loop {
        let msg = if enabled && socket.is_none() {
            match rx.recv_timeout(RECONNECT_INTERVAL) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => break,
            }
        };

        let mut shutdown = false;
        let mut handle = |cmd: Command| match cmd {
            Command::Update(presence) => {
                current = Some(presence);
                dirty = true;
            }
            Command::SetEnabled(next) => {
                if enabled != next {
                    enabled = next;
                    dirty = true;
                    if enabled {
                        last_attempt = None;
                    }
                }
            }
            Command::Shutdown => shutdown = true,
        };

        if let Some(cmd) = msg {
            handle(cmd);
        }
        while let Ok(cmd) = rx.try_recv() {
            handle(cmd);
        }

        if shutdown {
            if let Some(mut sock) = socket.take() {
                let _ = send_activity(&mut sock, None, started_at, &mut nonce);
                let _ = write_frame(&mut sock, OP_CLOSE, &json!({}));
            }
            break;
        }

        if !enabled {
            if let Some(mut sock) = socket.take() {
                let _ = send_activity(&mut sock, None, started_at, &mut nonce);
                let _ = write_frame(&mut sock, OP_CLOSE, &json!({}));
            }
            dirty = false;
            continue;
        }

        if socket.is_none() {
            let can_try = last_attempt.is_none_or(|t| t.elapsed() >= Duration::from_secs(5));
            if can_try {
                last_attempt = Some(Instant::now());
                if let Some(sock) = connect_and_handshake() {
                    socket = Some(sock);
                    dirty = true;
                }
            }
        }

        if dirty {
            if let Some(sock) = socket.as_mut() {
                if send_activity(sock, current.as_ref(), started_at, &mut nonce).is_ok() {
                    dirty = false;
                } else {
                    socket = None;
                }
            }
        }
    }
}

fn connect_and_handshake() -> Option<IpcSocket> {
    let mut sock = IpcSocket::connect()?;
    let handshake = json!({
        "v": 1,
        "client_id": APPLICATION_ID,
    });
    write_frame(&mut sock, OP_HANDSHAKE, &handshake).ok()?;
    let (op, _) = read_frame(&mut sock).ok()?;
    if op == OP_CLOSE {
        return None;
    }
    Some(sock)
}

fn build_activity_payload(presence: Option<&Presence>, started_at: u64, nonce: u64) -> Value {
    let activity = presence.map(|p| {
        let mut obj = serde_json::Map::new();
        obj.insert("details".into(), Value::String(p.details.clone()));
        if let Some(state) = &p.state {
            obj.insert("state".into(), Value::String(state.clone()));
        }
        if started_at > 0 {
            obj.insert("timestamps".into(), json!({ "start": started_at }));
        }
        obj.insert(
            "assets".into(),
            json!({
                "large_image": LARGE_IMAGE_KEY,
                "large_text": LARGE_IMAGE_TEXT,
            }),
        );
        Value::Object(obj)
    });
    json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": activity,
        },
        "nonce": nonce.to_string(),
    })
}

fn send_activity(
    sock: &mut IpcSocket,
    presence: Option<&Presence>,
    started_at: u64,
    nonce: &mut u64,
) -> io::Result<()> {
    let current_nonce = *nonce;
    *nonce = nonce.wrapping_add(1);
    let payload = build_activity_payload(presence, started_at, current_nonce);
    write_frame(sock, OP_FRAME, &payload)?;
    loop {
        let (op, body) = read_frame(sock)?;
        if op == OP_PING {
            write_raw_frame(sock, OP_PONG, &body)?;
            continue;
        }
        if op == OP_CLOSE {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "Discord IPC closed",
            ));
        }
        return Ok(());
    }
}

fn write_frame<W: Write>(writer: &mut W, opcode: u32, payload: &Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_raw_frame(writer, opcode, &bytes)
}

fn write_raw_frame<W: Write>(writer: &mut W, opcode: u32, payload: &[u8]) -> io::Result<()> {
    let mut header = [0u8; 8];
    header[..4].copy_from_slice(&opcode.to_le_bytes());
    header[4..].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    writer.write_all(&header)?;
    writer.write_all(payload)?;
    writer.flush()
}

fn read_frame<R: Read>(reader: &mut R) -> io::Result<(u32, Vec<u8>)> {
    let mut header = [0u8; 8];
    reader.read_exact(&mut header)?;
    let opcode = u32::from_le_bytes(header[..4].try_into().unwrap());
    let len = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Discord IPC frame too large",
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok((opcode, body))
}

#[cfg(windows)]
struct IpcSocket(std::fs::File);

#[cfg(windows)]
impl IpcSocket {
    fn connect() -> Option<Self> {
        use std::fs::OpenOptions;
        for index in 0..10 {
            let path = format!(r"\\?\pipe\discord-ipc-{index}");
            if let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) {
                return Some(Self(file));
            }
        }
        None
    }
}

#[cfg(windows)]
impl Read for IpcSocket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

#[cfg(windows)]
impl Write for IpcSocket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(unix)]
struct IpcSocket(std::os::unix::net::UnixStream);

#[cfg(unix)]
impl IpcSocket {
    fn connect() -> Option<Self> {
        use std::os::unix::net::UnixStream;
        use std::path::PathBuf;

        let mut dirs = Vec::new();
        for key in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
            if let Some(val) = std::env::var_os(key) {
                let p = PathBuf::from(&val);
                dirs.push(p.clone());
                dirs.push(p.join("app/com.discordapp.Discord"));
                dirs.push(p.join("snap.discord"));
            }
        }
        dirs.push(PathBuf::from("/tmp"));

        for dir in dirs {
            for index in 0..10 {
                let path = dir.join(format!("discord-ipc-{index}"));
                if let Ok(stream) = UnixStream::connect(&path) {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                    return Some(Self(stream));
                }
            }
        }
        None
    }
}

#[cfg(unix)]
impl Read for IpcSocket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

#[cfg(unix)]
impl Write for IpcSocket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn sample_room(stage: &str, host: bool, vote_open: bool) -> Room {
        serde_json::from_value(json!({
            "code": "SECRET42",
            "you": if host { "p1" } else { "p2" },
            "hostId": "p1",
            "stage": stage,
            "round": 2,
            "seats": 3,
            "catastrophe": "",
            "bunker": "",
            "packName": "",
            "voteOpen": vote_open,
            "ballotsCount": 0,
            "voteResults": [],
            "log": [],
            "connected": 2,
            "players": [
                { "id": "p1", "name": "Host", "active": true, "ready": true, "avatar": 1, "cards": [] },
                { "id": "p2", "name": "Guest", "active": true, "ready": false, "avatar": 2, "cards": [] },
                { "id": "p3", "name": "Out", "active": false, "ready": false, "avatar": 3, "cards": [] }
            ]
        }))
        .expect("sample room json")
    }

    #[test]
    fn encodes_and_decodes_ipc_frames() {
        let mut buf = Vec::new();
        let payload = json!({"v": 1, "client_id": APPLICATION_ID});
        write_frame(&mut buf, OP_HANDSHAKE, &payload).unwrap();
        let (op, body) = read_frame(&mut Cursor::new(buf)).unwrap();
        assert_eq!(op, OP_HANDSHAKE);
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), payload);
    }

    #[test]
    fn builds_presence_for_all_stages_without_leaking_room_code() {
        let menu = Presence::from_room(None);
        assert_eq!(menu.details, "В главном меню");
        assert!(menu.state.is_none());

        let lobby = Presence::from_room(Some(&sample_room("lobby", true, false)));
        assert!(lobby.details.contains("В лобби"));
        assert!(lobby.details.contains("Ведущий"));
        assert!(!lobby.details.contains("SECRET42"));
        assert!(!lobby.state.as_deref().unwrap_or("").contains("SECRET42"));

        let playing = Presence::from_room(Some(&sample_room("playing", false, false)));
        assert_eq!(playing.details, "В бункере · Раунд 2");
        assert_eq!(playing.state.as_deref(), Some("Живых: 2 · Мест: 3"));

        let voting = Presence::from_room(Some(&sample_room("playing", true, true)));
        assert_eq!(voting.details, "Голосование · Раунд 2");
        assert_eq!(voting.state.as_deref(), Some("Ведущий · Живых: 2 · Мест: 3"));

        let finished = Presence::from_room(Some(&sample_room("finished", false, false)));
        assert_eq!(finished.details, "Партия завершена");
        assert_eq!(finished.state.as_deref(), Some("Выживших: 2 / Мест: 3"));

        let payload = build_activity_payload(Some(&playing), 1700000000, 7);
        assert_eq!(payload["cmd"], "SET_ACTIVITY");
        assert_eq!(payload["args"]["activity"]["assets"]["large_image"], "logo");
        assert_eq!(payload["args"]["activity"]["timestamps"]["start"], 1700000000);
    }
}
