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
    ws_pool: Arc<tokio::sync::Mutex<HashMap<String, WebSocketEntry>>>,
}

impl ServiceState {
    pub fn new() -> Self {
        Self {
            ws_pool: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub async fn update_title(&self, title: String) -> anyhow::Result<()> {
        let mut ws_pool = self.ws_pool.lock().await;
        let mut error_ids = vec![];
        for (id, ws) in ws_pool.iter_mut() {
            let msg = MessageEvent::UpdateTitle {
                title: title.clone(),
            };
            if let Err(e) = ws.tx.send(WsEvent::Message(msg)).await {
                log::error!("send message to {} live2d error: {:?}", id, e);
                error_ids.push(id.clone());
            }
        }
        for id in error_ids {
            ws_pool.remove(&id);
        }

        Ok(())
    }

    pub async fn random_say(
        &self,
        vtb_name: String,
        text: Option<String>,
        motion: Option<String>,
        wav_voice: Option<Bytes>,
    ) -> anyhow::Result<()> {
        let mut ws_pool = self.ws_pool.lock().await;
        let ws = ws_pool
            .values_mut()
            .next()
            .ok_or_else(|| anyhow::anyhow!("ws_pool is empty"))?;

        if text.is_some() || motion.is_some() {
            let msg = MessageEvent::Speech(SpeechEvent {
                vtb_name,
                motion: motion.unwrap_or_default(),
                message: text.unwrap_or_default(),
                voice: wav_voice.is_some(),
            });
            ws.tx
                .send(WsEvent::Message(msg))
                .await
                .map_err(|e| anyhow::anyhow!("send message to live2d error: {:?}", e))?;

            if let Some(data) = wav_voice {
                ws.tx
                    .send(WsEvent::Message(MessageEvent::Voice(data)))
                    .await
                    .map_err(|e| anyhow::anyhow!("send wav to live2d error: {:?}", e))?;
            }
        }

        Ok(())
    }

    pub async fn say(
        &self,
        id: &str,
        vtb_name: String,
        text: Option<String>,
        motion: Option<String>,
        wav_voice: Option<Bytes>,
    ) -> anyhow::Result<()> {
        let mut ws_pool = self.ws_pool.lock().await;
        let ws = ws_pool
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("ID {id} Not found"))?;

        if text.is_some() || motion.is_some() {
            let msg = MessageEvent::Speech(SpeechEvent {
                vtb_name,
                motion: motion.unwrap_or_default(),
                message: text.unwrap_or_default(),
                voice: wav_voice.is_some(),
            });
            ws.tx
                .send(WsEvent::Message(msg))
                .await
                .map_err(|e| anyhow::anyhow!("send message to live2d error: {:?}", e))?;

            if let Some(data) = wav_voice {
                ws.tx
                    .send(WsEvent::Message(MessageEvent::Voice(data)))
                    .await
                    .map_err(|e| anyhow::anyhow!("send wav to live2d error: {:?}", e))?;
            }
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
        .route("/update_title", post(update_title))
        .route("/register_callback/{id}", post(register_callback))
}

async fn test_page() -> axum::response::Html<&'static str> {
    axum::response::Html(
        r#"
        <!doctype html>
        <html>
            <head></head>
            <body>
                <form action="/api/say_form" method="post" enctype="multipart/form-data">
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
        vtb_name: "".to_string(),
        text: None,
        motion: None,
        voice: None,
    };

    while let Some(field) = multipart.next_field().await? {
        let field_name = field.name().unwrap_or_default();
        match field_name {
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
        vtb_name,
        text,
        motion,
        voice,
        ..
    } = msg.unwrap();

    match state.random_say(vtb_name, text, motion, voice).await {
        Ok(_) => Ok(format!("ok")),
        Err(e) => {
            log::error!("random_say error: {:?}", e);
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

    match state.say(&id, vtb_name, text, motion, None).await {
        Ok(_) => Ok(format!("ok")),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Debug, serde::Deserialize)]
struct UpdateTitleRequest {
    title: String,
}

async fn update_title(
    State(state): State<ServiceState>,
    Json(title): Json<UpdateTitleRequest>,
) -> Result<String, StatusCode> {
    match state.update_title(title.title).await {
        Ok(_) => Ok(format!("ok")),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Debug, serde::Deserialize)]
struct RegisterCallbackRequest {
    callback_url: String,
}

async fn register_callback(
    Path(id): Path<String>,
    State(state): State<ServiceState>,
    Json(callback_url): Json<RegisterCallbackRequest>,
) -> Result<String, StatusCode> {
    let ws_pool = state.ws_pool.lock().await;
    if let Some(ws) = ws_pool.get(&id) {
        ws.tx
            .send(WsEvent::AddCallback(vec![callback_url.callback_url]))
            .await
            .map_err(|e| {
                log::error!("send message to live2d error: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }

    Ok(format!("ok"))
}

async fn websocket_handler(
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
    State(state): State<ServiceState>,
) -> impl IntoResponse {
    log::info!("ws connect id: {}", id);
    ws.on_upgrade(|socket| websocket(id, socket, state))
}

#[derive(Debug, Clone)]
pub enum WsEvent {
    Message(MessageEvent),
    AddCallback(Vec<String>),
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type")]
pub enum MessageEvent {
    UpdateTitle {
        title: String,
    },
    Speech(SpeechEvent),
    #[serde(skip)]
    Voice(Bytes),
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SpeechEvent {
    pub vtb_name: String,
    pub motion: String,
    pub message: String,
    pub voice: bool,
}

impl Into<axum::extract::ws::Message> for MessageEvent {
    fn into(self) -> axum::extract::ws::Message {
        axum::extract::ws::Message::Text(Utf8Bytes::from(serde_json::to_string(&self).unwrap()))
    }
}

async fn websocket(id: String, stream: WebSocket, state: ServiceState) {
    let mut pool = state.ws_pool.lock().await;

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

async fn callback(callback_urls: Vec<String>) {
    for url in callback_urls {
        tokio::spawn(async move {
            let client = reqwest::get(&url).await;
            if let Err(e) = client {
                log::warn!("callback {url} error: {:?}", e);
            }
        });
    }
}

async fn ws_loop(
    id: String,
    mut ws: WebSocket,
    mut rx: tokio::sync::mpsc::Receiver<WsEvent>,
) -> anyhow::Result<()> {
    log::info!("ws_loop {} start", id);
    let mut callback_urls: Option<Vec<String>> = None;
    loop {
        let r = select_ws(&mut ws, &mut rx).await?;
        match r {
            SelectResult::Ws(Message::Text(_)) | SelectResult::Ws(Message::Binary(_)) => {
                log::info!("{id} done");
                if let Some(callback_urls) = callback_urls.take() {
                    tokio::spawn(callback(callback_urls));
                }
            }

            SelectResult::Ws(Message::Close(_)) => {
                Err(anyhow::anyhow!("ws closed"))?;
            }
            SelectResult::Ws(Message::Ping(_)) | SelectResult::Ws(Message::Pong(_)) => {}
            SelectResult::Event(WsEvent::Message(MessageEvent::Voice(data))) => {
                ws.send(Message::Binary(data)).await?;
            }
            SelectResult::Event(WsEvent::Message(ws_msg)) => {
                ws.send(ws_msg.into()).await?;
            }
            SelectResult::Event(WsEvent::AddCallback(urls)) => match &mut callback_urls {
                Some(callback_urls) => {
                    callback_urls.extend(urls);
                }
                None => {
                    callback_urls = Some(urls);
                }
            },
        }
    }
}
