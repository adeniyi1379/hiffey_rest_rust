//! Decides what the shop sees first: connection, setup, PIN pad or the till.

use leptos::prelude::*;

use crate::api::Api;
use crate::config;
use crate::state::{refresh_held, refresh_menu, refresh_settings, App, Screen};

pub async fn run(app: App) {
    let api = app.api_anonymous();
    app.connection_error.set(None);
    app.booted.set(false);
    app.screen.set(Screen::Boot);

    if let Err(error) = api.health().await {
        app.online.set(false);
        app.connection_error.set(Some(error.message));
        app.screen.set(Screen::Connection);
        return;
    }
    app.online.set(true);

    match api.setup_status().await {
        Ok(status) if !status.configured => {
            app.screen.set(Screen::Setup);
            return;
        }
        Ok(_) => {}
        Err(error) => {
            app.connection_error.set(Some(error.message));
            app.screen.set(Screen::Connection);
            return;
        }
    }

    if let Some(token) = config::load_token() {
        let config = app.config.get();
        let api = Api::new(&config.base_url, &config.terminal_id, Some(&token));
        if let Ok(user) = api.me().await {
            app.start_session(pos_types::LoginResponse {
                token,
                user,
                expires_at: String::new(),
            });
            refresh_menu(app).await;
            refresh_held(app).await;
            refresh_settings(app).await;
            app.booted.set(true);
            return;
        }
        config::clear_token();
    }

    app.screen.set(Screen::Login);
    app.booted.set(true);
}

#[component]
pub fn Boot(app: App) -> impl IntoView {
    view! {
        <div class="boot">
            <span class="brand-mark">"H"</span>
            <h1>"Hiffey POS"</h1>
            <p class="muted">"Starting the till…"</p>
            {move || {
                app.connection_error
                    .get()
                    .map(|error| view! { <p class="error-text">{error}</p> })
            }}
        </div>
    }
}
