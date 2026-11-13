//! Keeps every terminal in step by following the server's `/events` socket.

use futures::StreamExt;
use gloo_net::websocket::{futures::WebSocket, Message};
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::{GetUntracked, Set};
use wasm_bindgen_futures::spawn_local;

use crate::state::{refresh_balances, refresh_held, refresh_menu, App};

const MAX_BACKOFF_MS: u32 = 10_000;

pub fn start(app: App) {
    spawn_local(async move {
        let mut backoff = 1_000;
        loop {
            let session = match app.session.get_untracked() {
                Some(session) => session,
                None => {
                    TimeoutFuture::new(500).await;
                    continue;
                }
            };
            let config = app.config.get_untracked();
            let url = socket_url(&config.base_url, &session.token);
            match WebSocket::open(&url) {
                Ok(socket) => {
                    app.online.set(true);
                    backoff = 1_000;
                    let (_writer, mut reader) = socket.split();
                    while let Some(Ok(message)) = reader.next().await {
                        if let Message::Text(text) = message {
                            handle(&app, &text);
                        }
                    }
                    app.online.set(false);
                }
                Err(_) => app.online.set(false),
            }
            TimeoutFuture::new(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF_MS);
        }
    });
}

fn socket_url(base_url: &str, token: &str) -> String {
    let base = if let Some(rest) = base_url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base_url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base_url.to_owned()
    };
    format!("{base}/events?token={token}")
}

fn handle(app: &App, text: &str) {
    let Ok(event) = serde_json::from_str::<pos_types::ServerEvent>(text) else {
        return;
    };
    match event.event.as_str() {
        "menu.changed" => {
            let app = *app;
            spawn_local(async move {
                refresh_menu(app).await;
            });
        }
        "order.created" | "order.updated" | "order.paid" | "order.voided" => {
            let app = *app;
            spawn_local(async move {
                refresh_held(app).await;
            });
        }
        "stock.changed" => {
            let app = *app;
            spawn_local(async move {
                refresh_balances(app).await;
                refresh_menu(app).await;
            });
        }
        _ => {}
    }
}
