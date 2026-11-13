//! Cashiers and managers sign in with their 6-digit PIN.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::state::{refresh_held, refresh_menu, refresh_settings, App, Screen, ToastKind};

#[component]
pub fn Login(app: App) -> impl IntoView {
    let pin = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let push = move |digit: char| {
        if busy.get_untracked() {
            return;
        }
        pin.update(|value| {
            if value.len() < 6 {
                value.push(digit);
            }
        });
        if pin.get_untracked().len() == 6 {
            sign_in(app, pin, busy, error);
        }
    };

    let backspace = move |_| {
        pin.update(|value| {
            value.pop();
        });
        error.set(None);
    };

    let clear = move |_| {
        pin.set(String::new());
        error.set(None);
    };

    let submit = move |_| {
        if pin.get_untracked().len() == 6 {
            sign_in(app, pin, busy, error);
        }
    };

    view! {
        <main class="login">
            <section class="login-side">
                <div class="login-brand">
                    <img class="login-logo" src="/assets/logo.png" alt="" />
                    <div>
                        <strong>"Hiffey"</strong>
                        <span>"Restaurant"</span>
                    </div>
                </div>

                <div class="login-portrait">
                    <img
                        class="login-hero"
                        src="/assets/food-menu.jpg"
                        alt="Hiffey restaurant food menu"
                    />
                </div>

                <div class="login-hero-copy">
                    // <p class="eyebrow">"Hiffey POS · Counter till"</p>
                    <h1>"Start the service day."</h1>
                    <p class="muted">
                        "Orders, stock and receipts stay on this machine. Sign in with your PIN to open the till."
                    </p>
                </div>

                <div class="login-meta">
                    <span class=move || {
                        if app.online.get() { "dot dot-online" } else { "dot dot-offline" }
                    }></span>
                    <span class="muted small">{move || {
                        if app.online.get() { "Till online" } else { "Till offline" }
                    }}</span>
                    <span class="muted small">{move || app.config.get().base_url}</span>
                    <button
                        class="link"
                        on:click=move |_| app.screen.set(Screen::Connection)
                    >
                        "Connection"
                    </button>
                </div>
            </section>

            <section class="login-pad">
                <header class="login-pad-head">
                    <p class="eyebrow">"Sign in"</p>
                    <h2>"Who is on the till?"</h2>
                    <p class="muted small">"Six digits. The till signs in as soon as the last one lands."</p>
                </header>

                <div class="pin-dots" aria-label="PIN">
                    {(0..6)
                        .map(|index| {
                            view! {
                                <span class=move || {
                                    if pin.get().len() > index { "dot filled" } else { "dot" }
                                }></span>
                            }
                        })
                        .collect_view()}
                </div>

                {move || {
                    error
                        .get()
                        .map(|message| view! { <p class="error-text">{message}</p> })
                }}

                <div class="keypad">
                    {(1..=9)
                        .map(|digit| {
                            view! {
                                <button
                                    class="key"
                                    on:click=move |_| push(char::from_digit(digit, 10).unwrap_or('0'))
                                >
                                    {digit.to_string()}
                                </button>
                            }
                        })
                        .collect_view()}
                    <button class="key key-soft" on:click=clear>"Clear"</button>
                    <button class="key" on:click=move |_| push('0')>"0"</button>
                    <button class="key key-soft" on:click=backspace>"\u{232b}"</button>
                </div>

                <button
                    class="button primary wide"
                    on:click=submit
                    disabled=move || busy.get() || pin.get().len() < 6
                >
                    {move || if busy.get() { "Signing in…" } else { "Sign in" }}
                </button>
            </section>
        </main>
    }
}

fn sign_in(app: App, pin: RwSignal<String>, busy: RwSignal<bool>, error: RwSignal<Option<String>>) {
    if busy.get_untracked() {
        return;
    }
    busy.set(true);
    error.set(None);
    let entered = pin.get_untracked();
    spawn_local(async move {
        match app.api_anonymous().login(&entered).await {
            Ok(session) => {
                pin.set(String::new());
                app.start_session(session);
                refresh_menu(app).await;
                refresh_settings(app).await;
                refresh_held(app).await;
            }
            Err(failure) => {
                pin.set(String::new());
                error.set(Some(failure.message.clone()));
                if failure.is_offline() {
                    app.online.set(false);
                }
                app.toast(ToastKind::Error, failure.message);
            }
        }
        busy.set(false);
    });
}
