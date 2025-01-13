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

#[derive(Clone, Debug)]
pub struct ServiceState {
    ws_pool: Arc<tokio::sync::Mutex<HashMap<String, WebSocket>>>,
}

impl ServiceState {
    pub fn new() -> Self {
        Self {
            ws_pool: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub async fn random_say(
        &self,
        vtb_name: String,
        text: Option<String>,
        motion: Option<String>,
        wav_voice: Option<Bytes>,
        hold_sec: u32,
    ) -> anyhow::Result<()> {
        let mut ws_pool = self.ws_pool.lock().await;
        let ws = ws_pool
            .values_mut()
            .next()
            .ok_or_else(|| anyhow::anyhow!("ws_pool is empty"))?;

        if text.is_some() || motion.is_some() {
            let msg = MessageEvent {
                vtb_name,
                motion: motion.unwrap_or_default(),
                say: text.unwrap_or_default(),
                hold_sec,
            };
            ws.send(msg.into())
                .await
                .map_err(|e| anyhow::anyhow!("send message to live2d error: {:?}", e))?;
        }
        if let Some(data) = wav_voice {
            ws.send(Message::Binary(data))
                .await
                .map_err(|e| anyhow::anyhow!("send wav to live2d error: {:?}", e))?;
        }
        Ok(())
    }

    pub async fn say(
        &self,
        id: &str,
        vtb_name: String,
        text: Option<String>,
        motion: Option<String>,
        wav_voice: Option<Vec<u8>>,
        hold_sec: u32,
    ) -> anyhow::Result<()> {
        let mut ws_pool = self.ws_pool.lock().await;
        let ws = ws_pool
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("ID {id} Not found"))?;

        if text.is_some() || motion.is_some() {
            let msg = MessageEvent {
                vtb_name,
                motion: motion.unwrap_or_default(),
                say: text.unwrap_or_default(),
                hold_sec,
            };
            ws.send(msg.into())
                .await
                .map_err(|e| anyhow::anyhow!("send message to live2d {id} error: {:?}", e))?;
        }
        if let Some(data) = wav_voice {
            ws.send(Message::Binary(Bytes::from_owner(data)))
                .await
                .map_err(|e| anyhow::anyhow!("send wav to live2d {id} error: {:?}", e))?;
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
}

#[cfg(debug_assertions)]
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
                req.voice = Some(field.bytes().await?);
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

    match state.random_say(vtb_name, text, motion, voice, 5).await {
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

    match state.say(&id, vtb_name, text, motion, None, 5).await {
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

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MessageEvent {
    pub vtb_name: String,
    pub motion: String,
    pub say: String,
    pub hold_sec: u32,
}

impl Into<axum::extract::ws::Message> for MessageEvent {
    fn into(self) -> axum::extract::ws::Message {
        axum::extract::ws::Message::Text(Utf8Bytes::from(serde_json::to_string(&self).unwrap()))
    }
}

async fn websocket(id: String, stream: WebSocket, state: ServiceState) {
    let mut pool = state.ws_pool.lock().await;
    pool.insert(id, stream);
    log::info!("ws pool {:?}", pool.keys());
}
