//! Where the till finds the shop's server. Terminal B uses this to point at A.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api::Api;
use crate::config::{normalize_base_url, Config, DEFAULT_BASE_URL};
use crate::screens::boot;
use crate::state::App;

#[component]
pub fn Connection(app: App) -> impl IntoView {
    let (address, set_address) = signal(app.config.get().base_url.clone());
    let terminal_id = app.config.get().terminal_id.clone();
    let busy = RwSignal::new(false);

    let test = move |_| {
        let url = normalize_base_url(&address.get_untracked());
        busy.set(true);
        spawn_local(async move {
            let api = Api::new(&url, "connection-test", None);
            match api.health().await {
                Ok(health) => {
                    app.online.set(true);
                    app.notify(format!(
                        "Connected to Hiffey POS {} ({}).",
                        health.version, health.status
                    ));
                }
                Err(error) => {
                    app.online.set(false);
                    app.toast(crate::state::ToastKind::Error, error.message);
                }
            }
            busy.set(false);
        });
    };

    let save = move |_| {
        let current = app.config.get();
        let config = Config::with_connection(&address.get_untracked(), &current.terminal_id);
        config.save();
        app.config.set(config);
        spawn_local(async move {
            boot::run(app).await;
        });
    };

    let use_local = move |_| {
        set_address.set(DEFAULT_BASE_URL.to_owned());
    };

    view! {
        <main class="centered-card">
            <header class="setup-head">
                <p class="eyebrow">"Hiffey POS"</p>
                <h1>"Connect this till"</h1>
                <p class="muted">
                    "Terminal A hosts the shop's data. Point this till at that PC's address on the shop network."
                </p>
            </header>

            {move || {
                app.connection_error
                    .get()
                    .map(|error| view! { <div class="banner banner-error">{error}</div> })
            }}

            <label class="field">
                <span>"Server address"</span>
                <input
                    type="text"
                    prop:value=move || address.get()
                    on:input=move |event| set_address.set(event_target_value(&event))
                    placeholder="http://192.168.1.10:47800"
                    autocomplete="off"
                    spellcheck="false"
                />
                <small class="muted">"Use the address shown on Terminal A, including the port."</small>
            </label>

            <label class="field">
                <span>"Terminal ID"</span>
                <input type="text" prop:value=terminal_id readonly />
                <small class="muted">"This machine's fixed identity. It stays the same on every order."</small>
            </label>

            <div class="row gap">
                <button class="button ghost" on:click=use_local>"Use this PC as the server"</button>
                <button class="button ghost" on:click=test disabled=move || busy.get()>
                    {move || if busy.get() { "Testing…" } else { "Test connection" }}
                </button>
                <button class="button primary" on:click=save>"Save and continue"</button>
            </div>
        </main>
    }
}
