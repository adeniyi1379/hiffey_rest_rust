use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
    Json,
};
use pos_types::{ServerEvent, SyncStatus, UserSummary};
use tokio::sync::broadcast;

use crate::{auth, error::ApiError, AppState};

#[derive(Debug, serde::Deserialize)]
pub struct EventsQuery {
    pub token: String,
}

/// Live push channel. The UI connects with its session token as a query parameter,
/// because browsers cannot set headers on a WebSocket handshake.
pub async fn events(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
) -> Result<Response, ApiError> {
    let session = auth::load_session(&state.pool, &query.token, "ws").await?;
    let receiver = state.events.subscribe();
    Ok(ws.on_upgrade(move |socket| push_events(socket, receiver, session.user)))
}

async fn push_events(
    mut socket: WebSocket,
    mut receiver: broadcast::Receiver<ServerEvent>,
    user: UserSummary,
) {
    let hello = ServerEvent {
        event: "connected".to_owned(),
        data: serde_json::json!({ "user": user }),
    };
    if send_event(&mut socket, &hello).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => {}
                }
            }
            event = receiver.recv() => {
                match event {
                    Ok(event) => {
                        if send_event(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn send_event(socket: &mut WebSocket, event: &ServerEvent) -> Result<(), ()> {
    let text = serde_json::to_string(event).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

/// Cloud sync is not enabled yet, but the outbox is already being filled.
pub async fn sync_status(
    State(state): State<AppState>,
    actor: auth::AuthUser,
) -> Result<Json<SyncStatus>, ApiError> {
    let _ = actor;
    let pending_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sync_outbox WHERE synced_at IS NULL")
            .fetch_one(&state.pool)
            .await?;
    let last_success_at: Option<String> =
        sqlx::query_scalar("SELECT last_success_at FROM sync_state WHERE id = 1")
            .fetch_one(&state.pool)
            .await?;

    Ok(Json(SyncStatus {
        enabled: false,
        pending_count,
        last_success_at,
    }))
}
