//! Application state: endpoint, pairing, chat, gossip, blobs.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use dashmap::DashMap;
use futures_lite::StreamExt;
use iroh::{
    endpoint::{presets, Connection, RemoteInfo},
    protocol::{AcceptError, ProtocolHandler, Router},
    Endpoint, EndpointId, PublicKey, SecretKey, TransportAddr,
};
use iroh_blobs::{store::mem::MemStore, BlobsProtocol, Hash};
use iroh_gossip::{
    api::{Event, GossipSender},
    net::Gossip,
    proto::TopicId,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::codes::{self, CODE_TTL_SECS};
use crate::config::Config;
use crate::drop as dead_drop;
use crate::identity::Identity;
use crate::pairing::{self, IdentityPayload};
use crate::protocol::{self, ChatMsg, GossipMsg, CHAT_ALPN, PAIR_ALPN};
use crate::storage::{self, StoredMessage, StoredPeer, StoredRoom, Storage};

static APP_REF: OnceLock<Arc<App>> = OnceLock::new();

pub fn register_app(app: Arc<App>) {
    let _ = APP_REF.set(app);
}

fn app_ref() -> Result<Arc<App>> {
    APP_REF
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("app not ready"))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub endpoint_id: String,
    pub label: String,
    pub connected: bool,
    pub path: String,
    pub verified: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RoomMember {
    pub endpoint_id: String,
    pub label: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub topic_id: String,
    pub label: String,
    pub members: Vec<RoomMember>,
}

#[derive(Clone, Serialize)]
pub struct UiMessage {
    pub id: String,
    pub conversation_id: String,
    pub sender_id: String,
    pub body: String,
    pub kind: String,
    pub ts: i64,
    pub outgoing: bool,
}

#[derive(Clone, Serialize)]
pub struct StatusSnapshot {
    pub endpoint_id: String,
    pub short_id: String,
    pub fingerprint: String,
    pub history_enabled: bool,
    pub sound: bool,
    pub hotkey: String,
    pub pairing_code: Option<String>,
    pub pairing_expires_in: Option<u64>,
    pub pairing_kind: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct CodeInspect {
    pub kind: String,
    pub summary: String,
    pub members: Option<u32>,
    pub reachable: bool,
}

struct PendingPair {
    code: String,
    kind: codes::CodeKind,
    started: Instant,
    /// Peer codes expire; room codes stay until cancelled.
    expires: bool,
    cancel: tokio::sync::watch::Sender<bool>,
}

struct PeerSession {
    label: String,
    send: mpsc::UnboundedSender<Vec<u8>>,
    path: Arc<Mutex<String>>,
    abort: Mutex<Option<tokio::task::AbortHandle>>,
}

struct RoomSession {
    sender: GossipSender,
    members: Arc<Mutex<HashSet<String>>>,
    labels: Arc<Mutex<HashMap<String, String>>>,
    abort: tokio::task::AbortHandle,
}

pub struct App {
    pub identity: Identity,
    pub storage: Storage,
    pub data_dir: PathBuf,
    pub config: Mutex<Config>,
    endpoint: Endpoint,
    gossip: Gossip,
    blobs: MemStore,
    _router: Router,
    peers: DashMap<String, PeerSession>,
    rooms: DashMap<String, RoomSession>,
    pending: Mutex<Option<PendingPair>>,
    dm_gossip: DashMap<String, GossipSender>,
    app_handle: Mutex<Option<AppHandle>>,
}

impl App {
    pub async fn bootstrap(data_dir: PathBuf) -> Result<Arc<Self>> {
        // Full wipe requested by prior /burn (db may have been locked mid-burn).
        if data_dir.join(".burn").exists() {
            let _ = std::fs::remove_dir_all(&data_dir);
            std::fs::create_dir_all(&data_dir)?;
        }

        let identity = Identity::load_or_create(&data_dir)?;
        let db_key = storage::db_key_from_identity(&identity.secret.to_bytes());
        let storage = Storage::open(&data_dir, &db_key)?;
        let config = Config::load(&data_dir);

        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(identity.secret.clone())
            .alpns(vec![
                CHAT_ALPN.to_vec(),
                PAIR_ALPN.to_vec(),
                iroh_gossip::ALPN.to_vec(),
                iroh_blobs::ALPN.to_vec(),
            ])
            .bind()
            .await
            .context("bind endpoint")?;

        let gossip = Gossip::builder().spawn(endpoint.clone());
        let blobs = MemStore::new();
        let blobs_proto = BlobsProtocol::new(&blobs, None);
        let chat_handler = ChatAccept::new();

        let router = Router::builder(endpoint.clone())
            .accept(CHAT_ALPN, chat_handler.clone())
            .accept(iroh_gossip::ALPN, gossip.clone())
            .accept(iroh_blobs::ALPN, blobs_proto)
            .spawn();

        endpoint.online().await;

        let app = Arc::new(Self {
            identity,
            storage,
            data_dir,
            config: Mutex::new(config),
            endpoint,
            gossip,
            blobs,
            _router: router,
            peers: DashMap::new(),
            rooms: DashMap::new(),
            pending: Mutex::new(None),
            dm_gossip: DashMap::new(),
            app_handle: Mutex::new(None),
        });

        register_app(app.clone());
        chat_handler.attach(app.clone());

        let boot = app.clone();
        tokio::spawn(async move {
            if let Err(e) = boot.reconnect_known().await {
                tracing::warn!("reconnect: {e:#}");
            }
        });

        Ok(app)
    }

    pub fn set_app_handle(&self, handle: AppHandle) {
        *self.app_handle.lock() = Some(handle);
    }

    fn emit<T: Serialize + Clone>(&self, event: &str, payload: T) {
        if let Some(h) = self.app_handle.lock().as_ref() {
            let _ = h.emit(event, payload);
        }
    }

    pub fn status(&self) -> Result<StatusSnapshot> {
        let id = self.identity.endpoint_id_bytes();
        let pending = self.pending.lock();
        let (code, exp, kind) = match pending.as_ref() {
            Some(p) => {
                let left = if p.expires {
                    Some(CODE_TTL_SECS.saturating_sub(p.started.elapsed().as_secs()))
                } else {
                    None
                };
                (Some(p.code.clone()), left, Some(p.kind.as_str().to_string()))
            }
            None => (None, None, None),
        };
        Ok(StatusSnapshot {
            endpoint_id: hex::encode(id),
            short_id: codes::short_id(&id),
            fingerprint: codes::fingerprint(&id),
            history_enabled: self.storage.history_enabled()?,
            sound: self.config.lock().sound,
            hotkey: self.config.lock().hotkey.clone(),
            pairing_code: code,
            pairing_expires_in: exp,
            pairing_kind: kind,
        })
    }

    pub fn list_peers(&self) -> Result<Vec<PeerInfo>> {
        let stored = self.storage.list_peers()?;
        Ok(stored
            .into_iter()
            .map(|p| {
                let sess = self.peers.get(&p.endpoint_id);
                PeerInfo {
                    endpoint_id: p.endpoint_id.clone(),
                    label: p.label,
                    connected: sess.is_some(),
                    path: sess
                        .map(|s| s.path.lock().clone())
                        .unwrap_or_else(|| "OFFLINE".into()),
                    verified: self
                        .storage
                        .is_peer_verified(&p.endpoint_id)
                        .unwrap_or(false),
                }
            })
            .collect())
    }

    pub fn list_rooms(&self) -> Result<Vec<RoomInfo>> {
        let stored = self.storage.list_rooms()?;
        Ok(stored
            .into_iter()
            .map(|r| {
                let (members, labels) = self
                    .rooms
                    .get(&r.topic_id)
                    .map(|s| {
                        (
                            s.members.lock().iter().cloned().collect::<Vec<_>>(),
                            s.labels.lock().clone(),
                        )
                    })
                    .unwrap_or_default();
                let members = members
                    .into_iter()
                    .map(|endpoint_id| {
                        let label = labels.get(&endpoint_id).cloned().unwrap_or_default();
                        RoomMember {
                            endpoint_id,
                            label,
                        }
                    })
                    .collect();
                RoomInfo {
                    topic_id: r.topic_id,
                    label: r.label,
                    members,
                }
            })
            .collect())
    }

    pub async fn start_pairing(&self) -> Result<String> {
        self.begin_invite(codes::CodeKind::Peer, None).await
    }

    async fn begin_invite(
        &self,
        kind: codes::CodeKind,
        topic_id: Option<[u8; 32]>,
    ) -> Result<String> {
        if let Some(old) = self.pending.lock().take() {
            let _ = old.cancel.send(true);
        }

        let code_display = match kind {
            codes::CodeKind::Peer => codes::generate_peer_code(),
            codes::CodeKind::Room => codes::generate_room_code(),
        };
        let (parsed_kind, body) = codes::parse_code(&code_display)?;
        let password = codes::password_material(parsed_kind, &body);
        let expires = matches!(parsed_kind, codes::CodeKind::Peer);

        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        *self.pending.lock() = Some(PendingPair {
            code: code_display.clone(),
            kind: parsed_kind,
            started: Instant::now(),
            expires,
            cancel: cancel_tx,
        });

        let app = app_ref()?;
        let long_term = self.identity.secret.clone();
        let topic_hex = topic_id.map(hex::encode);
        tokio::spawn(async move {
            if let Err(e) = run_pairing_host(
                app,
                parsed_kind,
                body,
                password,
                long_term,
                String::new(),
                topic_id,
                topic_hex,
                expires,
                cancel_rx,
            )
            .await
            {
                tracing::warn!("invite host: {e:#}");
            }
        });

        if expires {
            let app2 = app_ref()?;
            let code_check = code_display.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(CODE_TTL_SECS)).await;
                let mut g = app2.pending.lock();
                if g.as_ref().map(|p| p.code == code_check).unwrap_or(false) {
                    if let Some(p) = g.take() {
                        let _ = p.cancel.send(true);
                    }
                    app2.emit("pairing_expired", ());
                }
            });
        }

        Ok(code_display)
    }

    pub async fn inspect_code(&self, code_input: String) -> Result<CodeInspect> {
        let (kind, body) = codes::parse_code(&code_input)?;
        match kind {
            codes::CodeKind::Peer => Ok(CodeInspect {
                kind: "peer".into(),
                summary: "1:1 peer · single-use".into(),
                members: None,
                reachable: false,
            }),
            codes::CodeKind::Room => {
                match preview_room(kind, &body).await {
                    Ok(n) => Ok(CodeInspect {
                        kind: "room".into(),
                        summary: format!("party · {n} present"),
                        members: Some(n),
                        reachable: true,
                    }),
                    Err(_) => Ok(CodeInspect {
                        kind: "room".into(),
                        summary: "party · host not reached".into(),
                        members: None,
                        reachable: false,
                    }),
                }
            }
        }
    }

    pub async fn join_code(&self, code_input: String) -> Result<serde_json::Value> {
        let (kind, body) = codes::parse_code(&code_input)?;
        let password = codes::password_material(kind, &body);
        match kind {
            codes::CodeKind::Peer => {
                let (peer, topic) = run_pairing_join(
                    app_ref()?,
                    kind,
                    body,
                    password,
                    self.identity.secret.clone(),
                    String::new(),
                    false,
                )
                .await?;
                if topic.is_some() {
                    bail!("peer code returned a room topic - use an R- code for parties");
                }
                Ok(serde_json::json!({ "kind": "peer", "peer": peer }))
            }
            codes::CodeKind::Room => {
                let (peer, topic) = run_pairing_join(
                    app_ref()?,
                    kind,
                    body.clone(),
                    password,
                    self.identity.secret.clone(),
                    String::new(),
                    true, // party: skip DM side-channel
                )
                .await?;
                let topic_bytes = topic.unwrap_or_else(|| codes::room_topic_from_code(&body));
                let topic_hex = hex::encode(topic_bytes);
                let topic_id = TopicId::from_bytes(topic_bytes);
                let bootstrap = vec![parse_endpoint_id(&peer.endpoint_id)?];
                self.storage.upsert_room(&StoredRoom {
                    topic_id: topic_hex.clone(),
                    label: format!("party-{}", codes::short_id(&topic_bytes)),
                    created_at: chrono::Utc::now().timestamp(),
                    bootstrap: peer.endpoint_id.clone(),
                })?;
                self.join_room_inner(topic_id, topic_hex.clone(), bootstrap)
                    .await?;
                let room = self
                    .list_rooms()?
                    .into_iter()
                    .find(|r| r.topic_id == topic_hex)
                    .unwrap_or(RoomInfo {
                        topic_id: topic_hex,
                        label: format!("party-{}", codes::short_id(&topic_bytes)),
                        members: vec![],
                    });
                Ok(serde_json::json!({ "kind": "room", "room": room }))
            }
        }
    }

    pub async fn start_room(&self) -> Result<(String, String)> {
        let code_display = codes::generate_room_code();
        let (_kind, body) = codes::parse_code(&code_display)?;
        let topic_bytes = codes::room_topic_from_code(&body);
        let topic = TopicId::from_bytes(topic_bytes);
        let topic_hex = hex::encode(topic_bytes);

        self.storage.upsert_room(&StoredRoom {
            topic_id: topic_hex.clone(),
            label: format!("party-{}", codes::short_id(&topic_bytes)),
            created_at: chrono::Utc::now().timestamp(),
            bootstrap: String::new(),
        })?;
        self.join_room_inner(topic, topic_hex.clone(), vec![])
            .await?;

        // Install pending with this exact code (not a fresh generate)
        if let Some(old) = self.pending.lock().take() {
            let _ = old.cancel.send(true);
        }
        let password = codes::password_material(codes::CodeKind::Room, &body);
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        *self.pending.lock() = Some(PendingPair {
            code: code_display.clone(),
            kind: codes::CodeKind::Room,
            started: Instant::now(),
            expires: false,
            cancel: cancel_tx,
        });
        let app = app_ref()?;
        let long_term = self.identity.secret.clone();
        let topic_hex_clone = topic_hex.clone();
        tokio::spawn(async move {
            if let Err(e) = run_pairing_host(
                app,
                codes::CodeKind::Room,
                body,
                password,
                long_term,
                String::new(),
                Some(topic_bytes),
                Some(topic_hex_clone),
                false,
                cancel_rx,
            )
            .await
            {
                tracing::warn!("room host: {e:#}");
            }
        });

        Ok((code_display, topic_hex))
    }

    pub async fn join_pairing(&self, code_input: String) -> Result<PeerInfo> {
        let v = self.join_code(code_input).await?;
        if v.get("kind").and_then(|k| k.as_str()) == Some("peer") {
            Ok(serde_json::from_value(v.get("peer").cloned().unwrap())?)
        } else {
            bail!("not a peer code")
        }
    }

    pub async fn join_room_code(&self, code_input: String) -> Result<RoomInfo> {
        let v = self.join_code(code_input).await?;
        if v.get("kind").and_then(|k| k.as_str()) == Some("room") {
            Ok(serde_json::from_value(v.get("room").cloned().unwrap())?)
        } else {
            bail!("not a room code")
        }
    }

    async fn join_room_inner(
        &self,
        topic: TopicId,
        topic_hex: String,
        bootstrap: Vec<EndpointId>,
    ) -> Result<()> {
        if self.rooms.contains_key(&topic_hex) {
            return Ok(());
        }
        let gossip_topic = self.gossip.subscribe(topic, bootstrap).await?;
        let (sender, mut receiver) = gossip_topic.split();
        let members = Arc::new(Mutex::new(HashSet::new()));
        let labels = Arc::new(Mutex::new(HashMap::new()));
        let members2 = members.clone();
        let labels2 = labels.clone();
        let app = app_ref()?;
        let tid = topic_hex.clone();

        let task = tokio::spawn(async move {
            while let Some(ev) = receiver.next().await {
                match ev {
                    Ok(Event::Received(msg)) => {
                        if let Ok(g) = protocol::decode_gossip(&msg.content) {
                            handle_gossip_event(&app, &tid, g, &members2, &labels2);
                        }
                    }
                    Ok(Event::NeighborUp(id)) => {
                        let hex_id = hex::encode(id.as_bytes());
                        members2.lock().insert(hex_id);
                        emit_presence(&app, &tid, &members2);
                        persist_room_bootstrap(&app, &tid, &members2);
                    }
                    Ok(Event::NeighborDown(id)) => {
                        members2.lock().remove(&hex::encode(id.as_bytes()));
                        emit_presence(&app, &tid, &members2);
                        persist_room_bootstrap(&app, &tid, &members2);
                    }
                    _ => {}
                }
            }
        });

        let me = hex::encode(self.identity.endpoint_id_bytes());
        let handle = codes::short_id(&self.identity.endpoint_id_bytes());
        let payload = protocol::encode_gossip(&GossipMsg::Presence {
            sender: me.clone(),
            label: handle.clone(),
        })?;
        let _ = sender.broadcast(payload.into()).await;
        labels.lock().insert(me, handle);

        self.rooms.insert(
            topic_hex,
            RoomSession {
                sender,
                members,
                labels,
                abort: task.abort_handle(),
            },
        );
        Ok(())
    }

    pub async fn connect_peer(&self, endpoint_id_hex: &str) -> Result<()> {
        let me = hex::encode(self.identity.endpoint_id_bytes());
        if endpoint_id_hex == me {
            bail!("connecting to ourself is not supported");
        }
        let id = parse_endpoint_id(endpoint_id_hex)?;
        if self.peers.contains_key(endpoint_id_hex) {
            return Ok(());
        }
        self.emit(
            "handshake",
            serde_json::json!({ "phase": "negotiating", "peer": endpoint_id_hex }),
        );
        let conn = self
            .endpoint
            .connect(id, CHAT_ALPN)
            .await
            .context("connect peer")?;
        self.emit(
            "handshake",
            serde_json::json!({ "phase": "verifying", "peer": endpoint_id_hex }),
        );
        self.spawn_peer_session(conn, true).await?;
        self.emit(
            "handshake",
            serde_json::json!({ "phase": "connected", "peer": endpoint_id_hex }),
        );
        Ok(())
    }

    /// Reconnect a known peer by local nickname or id prefix.
    pub async fn connect_named(&self, name: &str) -> Result<PeerInfo> {
        let q = name.trim().to_lowercase();
        let peers = self.storage.list_peers()?;
        let peer = peers
            .iter()
            .find(|p| !p.label.is_empty() && p.label.to_lowercase() == q)
            .or_else(|| {
                peers
                    .iter()
                    .find(|p| !p.label.is_empty() && p.label.to_lowercase().starts_with(&q))
            })
            .or_else(|| {
                peers.iter().find(|p| {
                    p.endpoint_id.starts_with(&q)
                        || codes::short_id(
                            &hex::decode(&p.endpoint_id)
                                .ok()
                                .and_then(|b| <[u8; 32]>::try_from(b).ok())
                                .unwrap_or([0u8; 32]),
                        ) == q
                })
            })
            .ok_or_else(|| anyhow!("no known peer matching '{name}' — pair once with /connect"))?
            .clone();
        self.connect_peer(&peer.endpoint_id).await?;
        let connected = self.peers.contains_key(&peer.endpoint_id);
        let path = self
            .peers
            .get(&peer.endpoint_id)
            .map(|s| s.path.lock().clone())
            .unwrap_or_else(|| "OFFLINE".into());
        Ok(PeerInfo {
            endpoint_id: peer.endpoint_id.clone(),
            label: peer.label,
            connected,
            path,
            verified: self
                .storage
                .is_peer_verified(&peer.endpoint_id)
                .unwrap_or(false),
        })
    }

    pub fn set_verified(&self, endpoint_id: &str, on: bool) -> Result<()> {
        self.storage.set_peer_verified(endpoint_id, on)
    }

    pub fn peer_verified(&self, endpoint_id: &str) -> bool {
        self.storage.is_peer_verified(endpoint_id).unwrap_or(false)
    }

    async fn reconnect_known(&self) -> Result<()> {
        for p in self.storage.list_peers()? {
            if let Err(e) = self.connect_peer(&p.endpoint_id).await {
                tracing::debug!("could not reach {}: {e:#}", p.endpoint_id);
            }
        }
        for room in self.storage.list_rooms()? {
            if self.rooms.contains_key(&room.topic_id) {
                continue;
            }
            let Ok(raw) = hex::decode(&room.topic_id) else {
                continue;
            };
            if raw.len() != 32 {
                continue;
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&raw);
            let topic = TopicId::from_bytes(arr);
            let mut bootstrap = Vec::new();
            for part in room.bootstrap.split(',').filter(|s| !s.is_empty()) {
                if let Ok(id) = parse_endpoint_id(part) {
                    bootstrap.push(id);
                }
            }
            if let Err(e) = self
                .join_room_inner(topic, room.topic_id.clone(), bootstrap)
                .await
            {
                tracing::debug!("could not rejoin party {}: {e:#}", room.topic_id);
            }
        }
        Ok(())
    }

    pub async fn spawn_peer_session(&self, conn: Connection, initiator: bool) -> Result<()> {
        let remote = conn.remote_id();
        let remote_hex = hex::encode(remote.as_bytes());
        if self.peers.contains_key(&remote_hex) {
            return Ok(());
        }

        let path = Arc::new(Mutex::new(classify_remote_info_opt(
            self.endpoint.remote_info(remote).await.as_ref(),
        )));
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

        let (mut send, mut recv) = if initiator {
            conn.open_bi().await.map_err(|e| anyhow!("open_bi: {e}"))?
        } else {
            conn.accept_bi()
                .await
                .map_err(|e| anyhow!("accept_bi: {e}"))?
        };

        if initiator {
            let hello = protocol::encode_chat(&ChatMsg::Typing { active: false })?;
            protocol::write_lp(&mut send, &hello).await?;
        }

        let label = self
            .storage
            .list_peers()?
            .into_iter()
            .find(|p| p.endpoint_id == remote_hex)
            .map(|p| p.label)
            .unwrap_or_default();

        self.storage.upsert_peer(&StoredPeer {
            endpoint_id: remote_hex.clone(),
            label: label.clone(),
            created_at: chrono::Utc::now().timestamp(),
        })?;

        self.peers.insert(
            remote_hex.clone(),
            PeerSession {
                label: label.clone(),
                send: tx,
                path: path.clone(),
                abort: Mutex::new(None),
            },
        );

        let my_id = self.identity.endpoint_id_bytes();
        let mut their_id = [0u8; 32];
        their_id.copy_from_slice(remote.as_bytes());
        let topic_bytes = codes::dm_topic_id(&my_id, &their_id);
        let topic = TopicId::from_bytes(topic_bytes);
        if let Ok(gt) = self.gossip.subscribe(topic, vec![remote]).await {
            let (gsend, mut grec) = gt.split();
            self.dm_gossip.insert(remote_hex.clone(), gsend);
            let app = app_ref()?;
            let conv = remote_hex.clone();
            tokio::spawn(async move {
                while let Some(ev) = grec.next().await {
                    if let Ok(Event::Received(msg)) = ev {
                        if let Ok(GossipMsg::Typing { active, .. }) =
                            protocol::decode_gossip(&msg.content)
                        {
                            app.emit(
                                "typing",
                                serde_json::json!({ "conversation_id": conv, "active": active }),
                            );
                        }
                    }
                }
            });
        }

        self.emit(
            "peer_update",
            PeerInfo {
                endpoint_id: remote_hex.clone(),
                label,
                connected: true,
                path: path.lock().clone(),
                verified: self
                    .storage
                    .is_peer_verified(&remote_hex)
                    .unwrap_or(false),
            },
        );

        tokio::spawn(async move {
            while let Some(payload) = rx.recv().await {
                if protocol::write_lp(&mut send, &payload).await.is_err() {
                    break;
                }
            }
        });

        let app = app_ref()?;
        let conv = remote_hex.clone();
        let path_watch = path.clone();
        let endpoint = self.endpoint.clone();
        let session_task = tokio::spawn(async move {
            let path_task = {
                let path_watch = path_watch.clone();
                let endpoint = endpoint.clone();
                let app = app.clone();
                let conv = conv.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        if let Some(info) = endpoint.remote_info(remote).await {
                            let p = classify_remote_info(&info);
                            let mut cur = path_watch.lock();
                            if *cur != p {
                                *cur = p.clone();
                                app.emit(
                                    "conn_path",
                                    serde_json::json!({ "conversation_id": conv, "path": p }),
                                );
                            }
                        }
                    }
                })
            };

            loop {
                match protocol::read_lp(&mut recv, 1_000_000).await {
                    Ok(buf) => {
                        if let Ok(msg) = protocol::decode_chat(&buf) {
                            handle_chat_incoming(&app, &conv, msg);
                        }
                    }
                    Err(_) => break,
                }
            }
            path_task.abort();
            app.peers.remove(&conv);
            app.dm_gossip.remove(&conv);
            app.emit(
                "peer_update",
                PeerInfo {
                    endpoint_id: conv.clone(),
                    label: String::new(),
                    connected: false,
                    path: "OFFLINE".into(),
                    verified: app.storage.is_peer_verified(&conv).unwrap_or(false),
                },
            );
        });

        if let Some(sess) = self.peers.get(&remote_hex) {
            *sess.abort.lock() = Some(session_task.abort_handle());
        }

        Ok(())
    }

    pub async fn send_text(&self, conversation_id: &str, body: String) -> Result<UiMessage> {
        let id = Uuid::new_v4().to_string();
        let ts = chrono::Utc::now().timestamp();
        let me = hex::encode(self.identity.endpoint_id_bytes());

        if let Some(room) = self.rooms.get(conversation_id) {
            let g = GossipMsg::Chat {
                id: id.clone(),
                sender: me.clone(),
                body: body.clone(),
                ts,
            };
            room.sender
                .broadcast(protocol::encode_gossip(&g)?.into())
                .await?;
        } else if let Some(peer) = self.peers.get(conversation_id) {
            let msg = ChatMsg::Text {
                id: id.clone(),
                body: body.clone(),
                ts,
            };
            peer.send
                .send(protocol::encode_chat(&msg)?)
                .map_err(|_| anyhow!("peer send channel closed"))?;
        } else {
            self.connect_peer(conversation_id).await?;
            return Box::pin(self.send_text(conversation_id, body)).await;
        }

        let stored = StoredMessage {
            id: id.clone(),
            conversation_id: conversation_id.into(),
            sender_id: me.clone(),
            body: body.clone(),
            kind: "text".into(),
            created_at: ts,
        };
        self.storage.insert_message(&stored)?;

        let ui = UiMessage {
            id,
            conversation_id: conversation_id.into(),
            sender_id: me,
            body,
            kind: "text".into(),
            ts,
            outgoing: true,
        };
        self.emit("message", ui.clone());
        Ok(ui)
    }

    pub async fn send_typing(&self, conversation_id: &str, active: bool) -> Result<()> {
        let me = hex::encode(self.identity.endpoint_id_bytes());
        let g = GossipMsg::Typing {
            sender: me,
            active,
        };
        let payload = protocol::encode_gossip(&g)?;
        if let Some(room) = self.rooms.get(conversation_id) {
            room.sender.broadcast(payload.into()).await?;
        } else if let Some(gs) = self.dm_gossip.get(conversation_id) {
            gs.broadcast(payload.into()).await?;
        }
        Ok(())
    }

    pub async fn send_file(&self, conversation_id: &str, path: PathBuf) -> Result<UiMessage> {
        self.emit(
            "file_progress",
            serde_json::json!({ "conversation_id": conversation_id, "pct": 5, "phase": "read" }),
        );
        let data = tokio::fs::read(&path).await?;
        self.emit(
            "file_progress",
            serde_json::json!({ "conversation_id": conversation_id, "pct": 40, "phase": "hash" }),
        );
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();
        let size = data.len() as u64;
        let tag = self.blobs.add_slice(&data).await?;
        self.emit(
            "file_progress",
            serde_json::json!({ "conversation_id": conversation_id, "pct": 75, "phase": "offer" }),
        );
        let hash_hex = tag.hash.to_string();

        let id = Uuid::new_v4().to_string();
        let ts = chrono::Utc::now().timestamp();
        let me = hex::encode(self.identity.endpoint_id_bytes());
        let body = format!("FILE {name} ({size}b) {hash_hex}");

        if let Some(room) = self.rooms.get(conversation_id) {
            let g = GossipMsg::FileOffer {
                id: id.clone(),
                sender: me.clone(),
                name: name.clone(),
                size,
                hash: hash_hex.clone(),
                ts,
            };
            room.sender
                .broadcast(protocol::encode_gossip(&g)?.into())
                .await?;
        } else if let Some(peer) = self.peers.get(conversation_id) {
            let msg = ChatMsg::FileOffer {
                id: id.clone(),
                name,
                size,
                hash: hash_hex,
                ts,
            };
            peer.send
                .send(protocol::encode_chat(&msg)?)
                .map_err(|_| anyhow!("send closed"))?;
        } else {
            bail!("not connected");
        }

        self.storage.insert_message(&StoredMessage {
            id: id.clone(),
            conversation_id: conversation_id.into(),
            sender_id: me.clone(),
            body: body.clone(),
            kind: "file".into(),
            created_at: ts,
        })?;

        let ui = UiMessage {
            id,
            conversation_id: conversation_id.into(),
            sender_id: me,
            body,
            kind: "file".into(),
            ts,
            outgoing: true,
        };
        self.emit(
            "file_progress",
            serde_json::json!({ "conversation_id": conversation_id, "pct": 100, "phase": "done" }),
        );
        self.emit("message", ui.clone());
        Ok(ui)
    }

    pub async fn download_file(
        &self,
        from_endpoint: &str,
        hash_hex: &str,
        dest: PathBuf,
    ) -> Result<()> {
        let peer = parse_endpoint_id(from_endpoint)?;
        let hash: Hash = hash_hex.parse().context("hash")?;
        let conn = self
            .endpoint
            .connect(peer, iroh_blobs::ALPN)
            .await
            .context("connect for blob")?;
        self.blobs
            .remote()
            .fetch(conn, hash)
            .complete()
            .await
            .context("blob fetch")?;
        let bytes = self.blobs.get_bytes(hash).await.context("get bytes")?;
        tokio::fs::write(dest, &bytes).await?;
        Ok(())
    }

    pub fn history(&self, conversation_id: &str) -> Result<Vec<UiMessage>> {
        let me = hex::encode(self.identity.endpoint_id_bytes());
        Ok(self
            .storage
            .messages_for(conversation_id, 500)?
            .into_iter()
            .map(|m| UiMessage {
                outgoing: m.sender_id == me,
                id: m.id,
                conversation_id: m.conversation_id,
                sender_id: m.sender_id,
                body: m.body,
                kind: m.kind,
                ts: m.created_at,
            })
            .collect())
    }

    pub fn wipe(&self, conversation_id: &str) -> Result<usize> {
        self.storage.wipe_conversation(conversation_id)
    }

    /// Destroy identity + all local data. App should exit; next launch is a new person.
    pub fn burn_all(&self) -> Result<()> {
        let dir = &self.data_dir;
        let _ = self.storage.purge_all();
        // Marker so next boot wipes the dir even if sqlite files stay locked now.
        let _ = std::fs::write(dir.join(".burn"), b"1");
        for name in [
            "identity.json",
            "config.json",
            "history.db",
            "history.db-wal",
            "history.db-shm",
        ] {
            let _ = std::fs::remove_file(dir.join(name));
        }
        Ok(())
    }

    pub fn set_sound(&self, on: bool) -> Result<()> {
        let mut cfg = self.config.lock();
        cfg.sound = on;
        cfg.save(&self.data_dir)?;
        Ok(())
    }

    pub fn set_hotkey(&self, hotkey: &str) -> Result<String> {
        let mut cfg = self.config.lock();
        cfg.hotkey = hotkey.to_string();
        cfg.save(&self.data_dir)?;
        Ok(cfg.hotkey.clone())
    }

    pub fn fingerprint_self(&self) -> String {
        codes::fingerprint(&self.identity.endpoint_id_bytes())
    }

    pub fn fingerprint_peer(&self, name_or_id: &str) -> Result<String> {
        let q = name_or_id.trim().to_lowercase();
        let peers = self.storage.list_peers()?;
        let peer = peers
            .iter()
            .find(|p| !p.label.is_empty() && p.label.to_lowercase() == q)
            .or_else(|| {
                peers
                    .iter()
                    .find(|p| !p.label.is_empty() && p.label.to_lowercase().starts_with(&q))
            })
            .or_else(|| peers.iter().find(|p| p.endpoint_id.starts_with(&q)))
            .ok_or_else(|| anyhow!("peer not found"))?;
        let mut id = [0u8; 32];
        let raw = hex::decode(&peer.endpoint_id).context("peer id")?;
        if raw.len() != 32 {
            bail!("bad peer id");
        }
        id.copy_from_slice(&raw);
        Ok(codes::fingerprint(&id))
    }

    /// Seal a text note or small file for a known peer. Returns path written.
    pub fn seal_drop(
        &self,
        peer_query: &str,
        kind: &str,
        name: &str,
        body: &[u8],
        dest: PathBuf,
    ) -> Result<String> {
        let q = peer_query.trim().to_lowercase();
        let peers = self.storage.list_peers()?;
        let peer = peers
            .iter()
            .find(|p| !p.label.is_empty() && p.label.to_lowercase() == q)
            .or_else(|| {
                peers
                    .iter()
                    .find(|p| !p.label.is_empty() && p.label.to_lowercase().starts_with(&q))
            })
            .or_else(|| peers.iter().find(|p| p.endpoint_id.starts_with(&q)))
            .ok_or_else(|| anyhow!("unknown peer '{peer_query}' — need a known contact"))?;
        let mut recip = [0u8; 32];
        let raw = hex::decode(&peer.endpoint_id)?;
        if raw.len() != 32 {
            bail!("bad peer id");
        }
        recip.copy_from_slice(&raw);
        let secret = self.identity.secret.to_bytes();
        let sealed = dead_drop::seal(&secret, &recip, kind, name, body)?;
        dead_drop::write_drop_file(&dest, &sealed)?;
        Ok(dest.to_string_lossy().into_owned())
    }

    pub fn open_drop(&self, path: PathBuf) -> Result<dead_drop::OpenedDrop> {
        let data = dead_drop::read_drop_file(&path)?;
        let secret = self.identity.secret.to_bytes();
        let id = self.identity.endpoint_id_bytes();
        dead_drop::open(&secret, &id, &data)
    }

    pub fn set_history(&self, on: bool) -> Result<()> {
        self.storage.set_history_enabled(on)
    }

    pub fn set_label(&self, endpoint_id: &str, label: &str) -> Result<()> {
        if !self.storage.set_peer_label(endpoint_id, label)? {
            anyhow::bail!("unknown peer");
        }
        Ok(())
    }

    pub fn set_conv_label(&self, conversation_id: &str, label: &str) -> Result<()> {
        if self.storage.set_peer_label(conversation_id, label)? {
            return Ok(());
        }
        if self.storage.set_room_label(conversation_id, label)? {
            return Ok(());
        }
        anyhow::bail!("unknown conversation")
    }

    pub fn leave(&self, conversation_id: &str) -> Result<()> {
        if let Some((_, room)) = self.rooms.remove(conversation_id) {
            room.abort.abort();
            let _ = self.storage.remove_room(conversation_id);
            return Ok(());
        }
        if let Some((_, peer)) = self.peers.remove(conversation_id) {
            if let Some(h) = peer.abort.lock().take() {
                h.abort();
            }
        }
        self.dm_gossip.remove(conversation_id);
        let _ = self.storage.remove_peer(conversation_id);
        Ok(())
    }

    /// Cancel pending P-/R- invite code.
    pub fn cancel_invite(&self) -> Result<()> {
        if let Some(old) = self.pending.lock().take() {
            let _ = old.cancel.send(true);
        }
        Ok(())
    }

    pub fn save_drop_bytes(&self, dest: PathBuf, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, bytes)?;
        Ok(())
    }
}

fn emit_presence(app: &App, tid: &str, members: &Arc<Mutex<HashSet<String>>>) {
    app.emit(
        "presence",
        serde_json::json!({
            "topic": tid,
            "members": members.lock().iter().cloned().collect::<Vec<_>>()
        }),
    );
}

fn persist_room_bootstrap(app: &App, tid: &str, members: &Arc<Mutex<HashSet<String>>>) {
    let boot = members
        .lock()
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join(",");
    let _ = app.storage.set_room_bootstrap(tid, &boot);
}

fn parse_endpoint_id(hex_str: &str) -> Result<EndpointId> {
    let bytes = hex::decode(hex_str).context("hex endpoint id")?;
    if bytes.len() != 32 {
        bail!("endpoint id must be 32 bytes");
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    PublicKey::from_bytes(&arr).map_err(|e| anyhow!("{e}"))
}

fn classify_remote_info_opt(info: Option<&RemoteInfo>) -> String {
    match info {
        Some(i) => classify_remote_info(i),
        None => "...".into(),
    }
}

fn classify_remote_info(info: &RemoteInfo) -> String {
    let mut direct = false;
    let mut relay = false;
    for addr in info.addrs() {
        match addr.addr() {
            TransportAddr::Ip(_) => direct = true,
            TransportAddr::Relay(_) => relay = true,
            _ => {}
        }
    }
    if direct {
        "DIRECT".into()
    } else if relay {
        "RELAYED".into()
    } else {
        "UNKNOWN".into()
    }
}

fn notify_if_background(app: &App, title: &str, body: &str) {
    let Some(handle) = app.app_handle.lock().clone() else {
        return;
    };
    if crate::desktop::window_focused(&handle) {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    let preview: String = body.chars().take(80).collect();
    let _ = handle
        .notification()
        .builder()
        .title(title)
        .body(preview)
        .show();
}

fn handle_chat_incoming(app: &App, conv: &str, msg: ChatMsg) {
    match msg {
        ChatMsg::Text { id, body, ts } => {
            let sender = conv.to_string();
            let _ = app.storage.insert_message(&StoredMessage {
                id: id.clone(),
                conversation_id: conv.into(),
                sender_id: sender.clone(),
                body: body.clone(),
                kind: "text".into(),
                created_at: ts,
            });
            notify_if_background(app, "ratline", &body);
            app.emit(
                "message",
                UiMessage {
                    id,
                    conversation_id: conv.into(),
                    sender_id: sender,
                    body,
                    kind: "text".into(),
                    ts,
                    outgoing: false,
                },
            );
        }
        ChatMsg::FileOffer {
            id,
            name,
            size,
            hash,
            ts,
        } => {
            let body = format!("FILE {name} ({size}b) {hash}");
            let _ = app.storage.insert_message(&StoredMessage {
                id: id.clone(),
                conversation_id: conv.into(),
                sender_id: conv.into(),
                body: body.clone(),
                kind: "file".into(),
                created_at: ts,
            });
            notify_if_background(app, "ratline", &format!("file · {name}"));
            app.emit(
                "message",
                UiMessage {
                    id,
                    conversation_id: conv.into(),
                    sender_id: conv.into(),
                    body,
                    kind: "file".into(),
                    ts,
                    outgoing: false,
                },
            );
        }
        ChatMsg::Typing { active } => {
            app.emit(
                "typing",
                serde_json::json!({ "conversation_id": conv, "active": active }),
            );
        }
    }
}

fn handle_gossip_event(
    app: &App,
    topic: &str,
    g: GossipMsg,
    members: &Arc<Mutex<HashSet<String>>>,
    labels: &Arc<Mutex<HashMap<String, String>>>,
) {
    match g {
        GossipMsg::Chat {
            id,
            sender,
            body,
            ts,
        } => {
            let me = hex::encode(app.identity.endpoint_id_bytes());
            if sender == me {
                return;
            }
            let _ = app.storage.insert_message(&StoredMessage {
                id: id.clone(),
                conversation_id: topic.into(),
                sender_id: sender.clone(),
                body: body.clone(),
                kind: "text".into(),
                created_at: ts,
            });
            notify_if_background(app, "ratline", &body);
            app.emit(
                "message",
                UiMessage {
                    id,
                    conversation_id: topic.into(),
                    sender_id: sender,
                    body,
                    kind: "text".into(),
                    ts,
                    outgoing: false,
                },
            );
        }
        GossipMsg::Typing { sender, active } => {
            app.emit(
                "typing",
                serde_json::json!({ "conversation_id": topic, "sender": sender, "active": active }),
            );
        }
        GossipMsg::FileOffer {
            id,
            sender,
            name,
            size,
            hash,
            ts,
        } => {
            let body = format!("FILE {name} ({size}b) {hash}");
            let _ = app.storage.insert_message(&StoredMessage {
                id: id.clone(),
                conversation_id: topic.into(),
                sender_id: sender.clone(),
                body: body.clone(),
                kind: "file".into(),
                created_at: ts,
            });
            app.emit(
                "message",
                UiMessage {
                    id,
                    conversation_id: topic.into(),
                    sender_id: sender,
                    body,
                    kind: "file".into(),
                    ts,
                    outgoing: false,
                },
            );
        }
        GossipMsg::Presence { sender, label } => {
            members.lock().insert(sender.clone());
            if !label.is_empty() {
                labels.lock().insert(sender, label);
            }
            emit_presence(app, topic, members);
            persist_room_bootstrap(app, topic, members);
        }
    }
}

#[derive(Clone, Default)]
struct ChatAccept {
    app: Arc<Mutex<Option<Arc<App>>>>,
}

impl std::fmt::Debug for ChatAccept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatAccept").finish_non_exhaustive()
    }
}

impl ChatAccept {
    fn new() -> Self {
        Self {
            app: Arc::new(Mutex::new(None)),
        }
    }
    fn attach(&self, app: Arc<App>) {
        *self.app.lock() = Some(app);
    }
}

impl ProtocolHandler for ChatAccept {
    fn accept(
        &self,
        connection: Connection,
    ) -> impl std::future::Future<Output = Result<(), AcceptError>> + Send {
        let app = self.app.lock().clone();
        async move {
            if let Some(app) = app {
                if let Err(e) = app.spawn_peer_session(connection, false).await {
                    tracing::warn!("chat accept: {e:#}");
                }
            }
            Ok(())
        }
    }
}

async fn pair_as_host(
    conn: Connection,
    password: &str,
    mine: &IdentityPayload,
) -> Result<IdentityPayload> {
    let role = pairing::start_a(password);
    let my_spake_msg = role.outbound.clone();
    let (mut send, mut recv) = conn.accept_bi().await?;

    let (kind, their_spake) = pairing::read_frame(&mut recv).await?;
    if kind != pairing::FRAME_SPAKE {
        bail!("expected SPAKE frame");
    }
    let session = pairing::finish_a(role, &their_spake)?;

    send.write_all(&pairing::encode_frame(pairing::FRAME_SPAKE, &my_spake_msg))
        .await?;

    let sealed = pairing::seal_payload(&session, mine)?;
    send.write_all(&pairing::encode_frame(pairing::FRAME_IDENTITY, &sealed))
        .await?;

    let (kind, their_ct) = pairing::read_frame(&mut recv).await?;
    if kind != pairing::FRAME_IDENTITY {
        bail!("expected identity");
    }
    pairing::open_payload(&session, &their_ct)
}

async fn pair_as_joiner(
    conn: Connection,
    password: &str,
    mine: &IdentityPayload,
) -> Result<IdentityPayload> {
    let role = pairing::start_b(password);
    let my_spake_msg = role.outbound.clone();
    let (mut send, mut recv) = conn.open_bi().await?;

    send.write_all(&pairing::encode_frame(pairing::FRAME_SPAKE, &my_spake_msg))
        .await?;

    let (kind, their_spake) = pairing::read_frame(&mut recv).await?;
    if kind != pairing::FRAME_SPAKE {
        bail!("expected SPAKE frame");
    }
    let session = pairing::finish_b(role, &their_spake)?;

    let sealed = pairing::seal_payload(&session, mine)?;
    send.write_all(&pairing::encode_frame(pairing::FRAME_IDENTITY, &sealed))
        .await?;

    let (kind, their_ct) = pairing::read_frame(&mut recv).await?;
    if kind != pairing::FRAME_IDENTITY {
        bail!("expected identity");
    }
    pairing::open_payload(&session, &their_ct)
}

async fn run_pairing_host(
    app: Arc<App>,
    kind: codes::CodeKind,
    _body: String,
    password: String,
    long_term: SecretKey,
    label: String,
    topic_id: Option<[u8; 32]>,
    topic_hex: Option<String>,
    single_use: bool,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let eph = codes::ephemeral_secret(kind, &_body);
    let ep = Endpoint::builder(presets::N0)
        .secret_key(eph)
        .alpns(vec![PAIR_ALPN.to_vec()])
        .bind()
        .await?;
    ep.online().await;

    loop {
        let member_count = topic_hex.as_ref().and_then(|th| {
            app.rooms
                .get(th)
                .map(|r| r.members.lock().len() as u32 + 1) // include self
        });

        let my_payload = IdentityPayload {
            endpoint_id: {
                let mut a = [0u8; 32];
                a.copy_from_slice(long_term.public().as_bytes());
                a
            },
            label: label.clone(),
            topic_id,
            member_count,
        };

        tokio::select! {
            _ = cancel.changed() => {
                if *cancel.borrow() {
                    ep.close().await;
                    return Ok(());
                }
            }
            incoming = ep.accept() => {
                let Some(incoming) = incoming else { break; };
                let conn = match incoming.await {
                    Ok(c) => c,
                    Err(e) => { tracing::warn!("incoming: {e}"); continue; }
                };
                match pair_as_host(conn, &password, &my_payload).await {
                    Ok(their) => {
                        if their.endpoint_id.iter().all(|&b| b == 0) {
                            // inspect probe - ignore
                            continue;
                        }
                        let their_hex = hex::encode(their.endpoint_id);
                        let me = hex::encode(long_term.public().as_bytes());
                        if their_hex == me {
                            tracing::warn!("peer joined with our own identity (self-join)");
                            continue;
                        }
                        let is_party = topic_id.is_some();
                        if is_party {
                            // stay on the party line — no 1:1 side channel
                            if let Some(ref th) = topic_hex {
                                if let Some(session) = app.rooms.get(th) {
                                    let label = if their.label.is_empty() {
                                        codes::short_id(&their.endpoint_id)
                                    } else {
                                        their.label.clone()
                                    };
                                    session.members.lock().insert(their_hex.clone());
                                    session.labels.lock().insert(their_hex.clone(), label.clone());
                                    emit_presence(&app, th, &session.members);
                                    persist_room_bootstrap(&app, th, &session.members);
                                }
                                app.emit(
                                    "party_join",
                                    serde_json::json!({
                                        "topic": th,
                                        "endpoint_id": their_hex,
                                        "label": their.label,
                                    }),
                                );
                            }
                        } else {
                            app.storage.upsert_peer(&StoredPeer {
                                endpoint_id: their_hex.clone(),
                                label: their.label.clone(),
                                created_at: chrono::Utc::now().timestamp(),
                            })?;
                            if single_use {
                                *app.pending.lock() = None;
                            }
                            app.emit("paired", PeerInfo {
                                endpoint_id: their_hex.clone(),
                                label: their.label,
                                connected: false,
                                path: "...".into(),
                                verified: app.storage.is_peer_verified(&their_hex).unwrap_or(false),
                            });
                            if let Err(e) = app.connect_peer(&their_hex).await {
                                tracing::debug!("post-pair connect: {e:#}");
                            }
                        }
                        if single_use {
                            ep.close().await;
                            return Ok(());
                        }
                        // party: keep listening for more joiners
                    }
                    Err(e) => tracing::warn!("pair attempt failed: {e:#}"),
                }
            }
        }
    }
    ep.close().await;
    Ok(())
}

async fn preview_room(kind: codes::CodeKind, body: &str) -> Result<u32> {
    let password = codes::password_material(kind, body);
    let eph = codes::ephemeral_secret(kind, body);
    let dialer = Endpoint::builder(presets::N0)
        .secret_key(SecretKey::generate())
        .bind()
        .await?;
    dialer.online().await;
    let conn = tokio::time::timeout(
        Duration::from_secs(8),
        dialer.connect(eph.public(), PAIR_ALPN),
    )
    .await
    .context("timeout")?
    .context("dial")?;

    // Lightweight: SPAKE then read host identity for member_count, then drop
    let mine = IdentityPayload {
        endpoint_id: [0u8; 32],
        label: String::new(),
        topic_id: None,
        member_count: None,
    };
    let their = pair_as_joiner(conn, &password, &mine).await?;
    dialer.close().await;
    Ok(their.member_count.unwrap_or(1))
}

async fn run_pairing_join(
    app: Arc<App>,
    kind: codes::CodeKind,
    body: String,
    password: String,
    long_term: SecretKey,
    label: String,
    party: bool,
) -> Result<(PeerInfo, Option<[u8; 32]>)> {
    let eph = codes::ephemeral_secret(kind, &body);
    let eph_id = eph.public();

    let dialer = Endpoint::builder(presets::N0)
        .secret_key(SecretKey::generate())
        .bind()
        .await?;
    dialer.online().await;

    let conn = dialer
        .connect(eph_id, PAIR_ALPN)
        .await
        .context("dial pairing host (is the code valid / still active?)")?;

    let mine = IdentityPayload {
        endpoint_id: {
            let mut a = [0u8; 32];
            a.copy_from_slice(long_term.public().as_bytes());
            a
        },
        label,
        topic_id: None,
        member_count: None,
    };

    let their = pair_as_joiner(conn, &password, &mine).await?;
    dialer.close().await;

    let their_hex = hex::encode(their.endpoint_id);
    let me = hex::encode(long_term.public().as_bytes());
    if their_hex == me {
        bail!("that's your own code - give it to someone else");
    }

    let topic = their.topic_id;
    if !party {
        app.storage.upsert_peer(&StoredPeer {
            endpoint_id: their_hex.clone(),
            label: their.label.clone(),
            created_at: chrono::Utc::now().timestamp(),
        })?;
        if let Err(e) = app.connect_peer(&their_hex).await {
            tracing::debug!("post-join connect: {e:#}");
        }
    }

    Ok((
        PeerInfo {
            endpoint_id: their_hex.clone(),
            label: their.label,
            connected: !party,
            path: "...".into(),
            verified: app.storage.is_peer_verified(&their_hex).unwrap_or(false),
        },
        topic,
    ))
}
