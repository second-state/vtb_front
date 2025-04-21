use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Bytes,
    extract::{
        ws::{Message, Utf8Bytes, WebSocket},
        Multipart, Path, State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};

#[allow(unused)]
#[derive(Debug)]
pub struct WebSocketEntry {
    id: String,
    tx: tokio::sync::mpsc::Sender<WsEvent>,
}

#[derive(Clone, Debug)]
pub struct ServiceState {
    ws_pool: Arc<tokio::sync::RwLock<HashMap<String, WebSocketEntry>>>,
}

impl ServiceState {
    pub fn new() -> Self {
        Self {
            ws_pool: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    pub async fn update_title(&self, id: &str, title: String) -> anyhow::Result<()> {
        let ws_pool = self.ws_pool.read().await;
        let ws = ws_pool
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("ID {id} Not found"))?;
        ws.tx
            .send(WsEvent::UpdateTitle(title))
            .await
            .map_err(|e| anyhow::anyhow!("send message to live2d error: {:?}", e))?;

        Ok(())
    }

    pub async fn change_scene(&self, id: &str, index: usize) -> anyhow::Result<()> {
        let ws_pool = self.ws_pool.read().await;
        let ws = ws_pool
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("ID {id} Not found"))?;
        ws.tx
            .send(WsEvent::ChangeScene(index))
            .await
            .map_err(|e| anyhow::anyhow!("send message to live2d error: {:?}", e))?;

        Ok(())
    }

    pub async fn say(
        &self,
        id: &str,
        vtb_name: String,
        text: Option<String>,
        motion: Option<String>,
        wav_voice: Option<Bytes>,
        sync: bool,
    ) -> anyhow::Result<()> {
        let ws_pool = self.ws_pool.read().await;
        let ws = if id.is_empty() {
            ws_pool
                .values()
                .next()
                .ok_or_else(|| anyhow::anyhow!("ws_pool is empty"))?
        } else {
            ws_pool
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("ID {id} Not found"))?
        };

        if text.is_some() || motion.is_some() {
            if sync {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let ws_event = WsEvent::SyncMessage {
                    vtb_name,
                    motion: motion.unwrap_or_default(),
                    message: text.unwrap_or_default(),
                    voice: wav_voice,
                    waker: tx,
                };
                ws.tx
                    .send(ws_event)
                    .await
                    .map_err(|e| anyhow::anyhow!("send message to live2d error: {:?}", e))?;
                rx.await
                    .map_err(|_| anyhow::anyhow!("live2d connect closed"))?;
            } else {
                let ws_event = WsEvent::Message {
                    vtb_name,
                    motion: motion.unwrap_or_default(),
                    message: text.unwrap_or_default(),
                    voice: wav_voice,
                };
                ws.tx
                    .send(ws_event)
                    .await
                    .map_err(|e| anyhow::anyhow!("send message to live2d error: {:?}", e))?;
            };
        }
        Ok(())
    }
}

pub fn router(state: ServiceState, dist: &str) -> Router {
    let serve_dir = tower_http::services::ServeDir::new(dist).not_found_service(
        tower_http::services::ServeFile::new(format!("{}/index.html", dist)),
    );

    if cfg!(debug_assertions) {
        Router::new()
            // .nest_service("/", serve_dir.clone())
            .nest("/api", api_router())
            .route("/ws/{id}", get(websocket_handler))
            .route("/test/say", get(test_page))
            .fallback_service(serve_dir)
            .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
            .with_state(state)
    } else {
        Router::new()
            // .nest_service("/", serve_dir)
            .nest("/api", api_router())
            .route("/ws/{id}", get(websocket_handler))
            .fallback_service(serve_dir)
            .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
            .with_state(state)
    }
}

fn api_router() -> Router<ServiceState> {
    Router::new()
        .route("/say/{id}", post(send_msg))
        .route("/say_form", post(send_msg_form))
        .route("/sync/say/{id}", post(send_msg_sync))
        .route("/sync/say_form", post(send_msg_form_sync))
        .route("/update_title/{id}", post(update_title))
        .route("/change_scene/{id}", post(change_scene))
}

async fn test_page() -> axum::response::Html<&'static str> {
    axum::response::Html(
        r#"
        <!doctype html>
        <html>
            <head></head>
            <body>
                <label>Async:</label>
                <form action="/api/say_form" method="post" enctype="multipart/form-data">
                    <label>
                        id:
                        <input type="text" name="id">
                    </label>
                    <label>
                        vtb_name:
                        <input type="text" name="vtb_name">
                    </label>
                    <label>
                        text:
                        <input type="text" name="text">
                    </label>
                    <label>
                        motion:
                        <input type="text" name="motion">
                    </label>
                    <label>
                        voice:
                        <input type="file" name="voice">
                    </label>

                    <input type="submit" value="Submit">
                </form>
                <label>Sync:</label>
                <form action="/api/sync/say_form" method="post" enctype="multipart/form-data">
                    <label>
                        id:
                        <input type="text" name="id">
                    </label>
                    <label>
                        vtb_name:
                        <input type="text" name="vtb_name">
                    </label>
                    <label>
                        text:
                        <input type="text" name="text">
                    </label>
                    <label>
                        motion:
                        <input type="text" name="motion">
                    </label>
                    <label>
                        voice:
                        <input type="file" name="voice">
                    </label>

                    <input type="submit" value="Submit">
                </form>
            </body>
        </html>
        "#,
    )
}

#[derive(Debug, serde::Deserialize)]
struct SendMsgRequest {
    id: String,
    vtb_name: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    motion: Option<String>,
    #[serde(skip)]
    voice: Option<Bytes>,
}

async fn parse_from_multipart(mut multipart: Multipart) -> anyhow::Result<SendMsgRequest> {
    let mut req = SendMsgRequest {
        id: String::new(),
        vtb_name: String::new(),
        text: None,
        motion: None,
        voice: None,
    };

    while let Some(field) = multipart.next_field().await? {
        let field_name = field.name().unwrap_or_default();
        match field_name {
            "id" => {
                req.id = field.text().await?;
            }
            "vtb_name" => {
                req.vtb_name = field.text().await?;
            }
            "text" => {
                req.text = Some(field.text().await?);
            }
            "motion" => {
                req.motion = Some(field.text().await?);
            }
            "voice" => {
                let data = field.bytes().await?;
                if !data.is_empty() {
                    req.voice = Some(data);
                }
            }
            _ => {}
        }
    }

    Ok(req)
}

async fn send_msg_form(
    State(state): State<ServiceState>,
    multipart: Multipart,
) -> Result<String, StatusCode> {
    let msg = parse_from_multipart(multipart).await;
    if let Err(e) = msg {
        log::error!("parse_from_multipart error: {:?}", e);
        return Err(StatusCode::BAD_REQUEST);
    }

    let SendMsgRequest {
        id,
        vtb_name,
        text,
        motion,
        voice,
        ..
    } = msg.unwrap();

    match state
        .say(id.as_str(), vtb_name, text, motion, voice, false)
        .await
    {
        Ok(_) => Ok(format!("ok")),
        Err(e) => {
            log::error!("say error: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn send_msg(
    Path(id): Path<String>,
    State(state): State<ServiceState>,
    Json(msg): Json<SendMsgRequest>,
) -> Result<String, StatusCode> {
    log::info!("send_msg id: {}, msg: {:?}", id, msg);

    let SendMsgRequest {
        vtb_name,
        text,
        motion,
        ..
    } = msg;

    match state.say(&id, vtb_name, text, motion, None, false).await {
        Ok(_) => Ok(format!("ok")),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn send_msg_form_sync(
    State(state): State<ServiceState>,
    multipart: Multipart,
) -> Result<String, StatusCode> {
    let msg = parse_from_multipart(multipart).await;
    if let Err(e) = msg {
        log::error!("parse_from_multipart error: {:?}", e);
        return Err(StatusCode::BAD_REQUEST);
    }

    let SendMsgRequest {
        id,
        vtb_name,
        text,
        motion,
        voice,
        ..
    } = msg.unwrap();

    match state
        .say(id.as_str(), vtb_name, text, motion, voice, true)
        .await
    {
        Ok(_) => Ok(format!("ok")),
        Err(e) => {
            log::error!("say error: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn send_msg_sync(
    Path(id): Path<String>,
    State(state): State<ServiceState>,
    Json(msg): Json<SendMsgRequest>,
) -> Result<String, StatusCode> {
    log::info!("send_msg id: {}, msg: {:?}", id, msg);

    let SendMsgRequest {
        vtb_name,
        text,
        motion,
        ..
    } = msg;

    match state.say(&id, vtb_name, text, motion, None, true).await {
        Ok(_) => Ok(format!("ok")),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Debug, serde::Deserialize)]
struct UpdateTitleRequest {
    title: String,
}

async fn update_title(
    Path(id): Path<String>,
    State(state): State<ServiceState>,
    Json(title): Json<UpdateTitleRequest>,
) -> Result<String, StatusCode> {
    match state.update_title(&id, title.title).await {
        Ok(_) => Ok(format!("ok")),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Debug, serde::Deserialize)]
struct ChangeSceneRequest {
    index: usize,
}

async fn change_scene(
    Path(id): Path<String>,
    State(state): State<ServiceState>,
    Json(req): Json<ChangeSceneRequest>,
) -> Result<String, StatusCode> {
    match state.change_scene(&id, req.index).await {
        Ok(_) => Ok(format!("ok")),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn websocket_handler(
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
    State(state): State<ServiceState>,
) -> impl IntoResponse {
    log::info!("ws connect id: {}", id);
    ws.on_upgrade(|socket| websocket(id, socket, state))
}

#[derive(Debug)]
pub enum WsEvent {
    Message {
        vtb_name: String,
        motion: String,
        message: String,
        voice: Option<Bytes>,
    },
    SyncMessage {
        vtb_name: String,
        motion: String,
        message: String,
        voice: Option<Bytes>,
        waker: tokio::sync::oneshot::Sender<()>,
    },
    UpdateTitle(String),
    ChangeScene(usize),
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type")]
pub enum MessageEvent {
    UpdateTitle { title: String },
    ChangeScene { index: usize },
    Speech(SpeechEvent),
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SpeechEvent {
    pub vtb_name: String,
    pub motion: String,
    pub message: String,
    pub voice: bool,
    pub waker: Option<usize>,
}

impl Into<axum::extract::ws::Message> for MessageEvent {
    fn into(self) -> axum::extract::ws::Message {
        axum::extract::ws::Message::Text(Utf8Bytes::from(serde_json::to_string(&self).unwrap()))
    }
}

async fn websocket(id: String, stream: WebSocket, state: ServiceState) {
    let mut pool = state.ws_pool.write().await;

    let (tx, rx) = tokio::sync::mpsc::channel(10);

    let ws_entry = WebSocketEntry { id: id.clone(), tx };
    pool.insert(id.clone(), ws_entry);

    // pool.insert(id, stream);
    log::info!("ws pool {:?}", pool.keys());
    tokio::spawn(ws_loop(id, stream, rx));
}

enum SelectResult {
    Event(WsEvent),
    Ws(Message),
}

async fn select_ws(
    ws: &mut WebSocket,
    rx: &mut tokio::sync::mpsc::Receiver<WsEvent>,
) -> anyhow::Result<SelectResult> {
    tokio::select! {
        msg = ws.recv() => {
            match msg {
                Some(Ok(msg)) => Ok(SelectResult::Ws(msg)),
                Some(Err(e)) => Err(anyhow::anyhow!("ws recv error: {:?}", e)),
                None => Err(anyhow::anyhow!("ws closed")),
            }
        }
        msg = rx.recv() => {
            match msg {
                Some(msg) => Ok(SelectResult::Event(msg)),
                None => Err(anyhow::anyhow!("rx recv close")),
            }
        }
    }
}

async fn ws_loop(
    id: String,
    mut ws: WebSocket,
    mut rx: tokio::sync::mpsc::Receiver<WsEvent>,
) -> anyhow::Result<()> {
    log::info!("ws_loop {} start", id);
    let mut wakers: slab::Slab<tokio::sync::oneshot::Sender<()>> = slab::Slab::new();

    loop {
        let r = select_ws(&mut ws, &mut rx).await?;
        match r {
            SelectResult::Ws(Message::Text(wake_id)) => {
                if let Ok(wake_id) = wake_id.parse::<usize>() {
                    log::info!("{id}:{wake_id} done");
                    if let Some(tx) = wakers.try_remove(wake_id) {
                        let _ = tx.send(());
                    }
                }
            }
            SelectResult::Ws(Message::Binary(_)) => {
                log::warn!("binary message not support");
            }

            SelectResult::Ws(Message::Close(_)) => {
                Err(anyhow::anyhow!("ws closed"))?;
            }
            SelectResult::Ws(Message::Ping(_)) | SelectResult::Ws(Message::Pong(_)) => {}

            SelectResult::Event(WsEvent::UpdateTitle(title)) => {
                let event = MessageEvent::UpdateTitle { title };
                ws.send(event.into()).await?;
            }
            SelectResult::Event(WsEvent::ChangeScene(index)) => {
                let event = MessageEvent::ChangeScene { index };
                ws.send(event.into()).await?;
            }

            SelectResult::Event(WsEvent::Message {
                vtb_name,
                motion,
                message,
                voice,
            }) => {
                let event = MessageEvent::Speech(SpeechEvent {
                    vtb_name,
                    motion,
                    message,
                    voice: voice.is_some(),
                    waker: None,
                });
                ws.send(event.into()).await?;
                if let Some(data) = voice {
                    ws.send(Message::Binary(data)).await?;
                }
            }
            SelectResult::Event(WsEvent::SyncMessage {
                vtb_name,
                motion,
                message,
                voice,
                waker,
            }) => {
                // let wake_id = wakers.insert(waker);
                let entry = wakers.vacant_entry();
                let key = entry.key();

                let event = MessageEvent::Speech(SpeechEvent {
                    vtb_name,
                    motion,
                    message,
                    voice: voice.is_some(),
                    waker: Some(key),
                });
                ws.send(event.into()).await?;
                if let Some(data) = voice {
                    ws.send(Message::Binary(data)).await?;
                    entry.insert(waker);
                } else {
                    let _ = waker.send(());
                }
            }
        }
    }
}
