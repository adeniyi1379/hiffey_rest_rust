#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

mod cart;
mod format;

#[cfg(target_arch = "wasm32")]
mod api;
#[cfg(target_arch = "wasm32")]
mod components;
#[cfg(target_arch = "wasm32")]
mod config;
#[cfg(target_arch = "wasm32")]
mod events;
#[cfg(target_arch = "wasm32")]
mod screens;
#[cfg(target_arch = "wasm32")]
mod shell;
#[cfg(target_arch = "wasm32")]
mod state;

#[cfg(target_arch = "wasm32")]
fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(Root);
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("pos-ui renders in the browser. Build it with trunk, or run the Tauri app.");
}

#[cfg(target_arch = "wasm32")]
use leptos::prelude::*;

#[cfg(target_arch = "wasm32")]
use components::ToastHost;
#[cfg(target_arch = "wasm32")]
use screens::{Boot, Connection, Login, Setup};
#[cfg(target_arch = "wasm32")]
use state::{App, Screen};

#[cfg(target_arch = "wasm32")]
#[component]
fn root() -> impl IntoView {
    let app = App::new();
    app.config.set(crate::config::Config::load());
    events::start(app);
    leptos::task::spawn_local(async move {
        crate::screens::boot::run(app).await;
    });

    view! {
        {move || match app.screen.get() {
            Screen::Boot => view! { <Boot app /> }.into_any(),
            Screen::Connection => view! { <Connection app /> }.into_any(),
            Screen::Setup => view! { <Setup app /> }.into_any(),
            Screen::Login => view! { <Login app /> }.into_any(),
            Screen::Shell => view! { <shell::Shell app /> }.into_any(),
        }}
        <ToastHost app />
    }
}
