//! The signed-in frame: navigation down the side, work area on the right.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::format::clock;
use crate::screens::admin::menu::MenuAdmin;
use crate::screens::admin::reports::ReportsAdmin;
use crate::screens::admin::settings::SettingsAdmin;
use crate::screens::admin::stock::StockAdmin;
use crate::screens::admin::workers::WorkersAdmin;
use crate::screens::sale::{Held, Sale};
use crate::state::{App, Page};

#[component]
pub fn Shell(app: App) -> impl IntoView {
    let now = RwSignal::new(chrono::Utc::now());
    Effect::new(move |_| {
        use gloo_timers::future::TimeoutFuture;
        spawn_local(async move {
            loop {
                TimeoutFuture::new(30_000).await;
                now.set(chrono::Utc::now());
            }
        });
    });

    let sign_out = move |_| end_session(app);
    let sign_out_top = move |_| end_session(app);

    let can_manage = app.role().is_some_and(pos_types::Role::can_manage);

    view! {
        <div class="shell">
            <aside class="sidebar">
                <div class="brand">
                    <img class="brand-logo" src="/assets/logo.png" alt="" />
                    <div>
                        <strong>"Hiffey"</strong>
                        <small class="muted">"Restaurant till"</small>
                    </div>
                </div>

                <nav class="nav">
                    <NavItem app page=Page::Sale label="Sale" />
                    <NavItem app page=Page::Held label="Held orders" />
                    {can_manage
                        .then(|| {
                            view! {
                                <hr class="nav-rule" />
                                <p class="nav-caption">"Manage"</p>
                                <NavItem app page=Page::Stock label="Stock" />
                                <NavItem app page=Page::Menu label="Menu" />
                                <NavItem app page=Page::Workers label="Workers" />
                                <NavItem app page=Page::Settings label="Settings" />
                                <hr class="nav-rule" />
                                <p class="nav-caption">"Owner"</p>
                                <NavItem app page=Page::Reports label="Reports" />
                                <NavItem app page=Page::Sync label="Sync" />
                            }
                        })}
                </nav>

                <div class="sidebar-foot">
                    <div class="user-chip">
                        <span class="avatar">
                            {move || initials(&app.user().map(|user| user.name).unwrap_or_default())}
                            <span class=move || {
                                if app.online.get() { "presence online" } else { "presence" }
                            }></span>
                        </span>
                        <div class="who">
                            <strong>{move || app.user().map(|user| user.name).unwrap_or_default()}</strong>
                            <small class="muted">{move || {
                                app.user().map(|user| user.role.to_string()).unwrap_or_default()
                            }}</small>
                        </div>
                    </div>
                    <button class="signout" on:click=sign_out aria-label="Log out">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M14 4h4a1.5 1.5 0 0 1 1.5 1.5v13A1.5 1.5 0 0 1 18 20h-4" />
                            <path d="M10 8l-4 4 4 4M6 12h10" />
                        </svg>
                        <span>"Log out"</span>
                    </button>
                </div>
            </aside>

            <main class="workspace">
                <header class="topbar">
                    <div>
                        <p class="eyebrow">{move || page_label(app.page.get())}</p>
                        <h1>{move || page_title(app.page.get())}</h1>
                    </div>
                    <div class="topbar-side">
                        <span class="muted small">{move || clock(&now.get())}</span>
                        <span class="status-pill">
                            <span class=move || {
                                if app.online.get() { "dot dot-online" } else { "dot dot-offline" }
                            }></span>
                            {move || if app.online.get() { "Online" } else { "Offline" }}
                        </span>
                        <button class="topbar-logout" on:click=sign_out_top>
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                                <path d="M14 4h4a1.5 1.5 0 0 1 1.5 1.5v13A1.5 1.5 0 0 1 18 20h-4" />
                                <path d="M10 8l-4 4 4 4M6 12h10" />
                            </svg>
                            <span>"Log out"</span>
                        </button>
                    </div>
                </header>
                <section class="page">
                    <PageBody app />
                </section>
            </main>
        </div>
    }
}

fn end_session(app: App) {
    spawn_local(async move {
        let _ = app.api().logout().await;
        app.end_session();
    });
}

#[component]
fn NavItem(app: App, page: Page, #[prop(into)] label: String) -> impl IntoView {
    let label = label.clone();
    view! {
        <button
            class=move || {
                if app.page.get() == page { "nav-item active" } else { "nav-item" }
            }
            on:click=move |_| app.page.set(page)
        >
            <span class="nav-icon" aria-hidden="true">{nav_icon(page)}</span>
            {label}
        </button>
    }
}

fn nav_icon(page: Page) -> impl IntoView {
    match page {
        Page::Sale => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M5 8h14l-1 11H6L5 8Z" />
                <path d="M9 8V6a3 3 0 0 1 6 0v2" />
            </svg>
        }
        .into_any(),
        Page::Held => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="12" cy="12" r="8" />
                <path d="M12 8v4l2.5 2" />
            </svg>
        }
        .into_any(),
        Page::Stock => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M4 8.5 12 4l8 4.5v7L12 20l-8-4.5v-7Z" />
                <path d="M4 8.5 12 13l8-4.5M12 13v7" />
            </svg>
        }
        .into_any(),
        Page::Menu => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M6 4h12v16H6z" />
                <path d="M9 8h6M9 12h6M9 16h4" />
            </svg>
        }
        .into_any(),
        Page::Workers => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="9" cy="8" r="3" />
                <path d="M3.5 19a5.5 5.5 0 0 1 11 0" />
                <path d="M16 5.5a3 3 0 0 1 0 5.8M17 14a5 5 0 0 1 3.5 5" />
            </svg>
        }
        .into_any(),
        Page::Settings => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M5 7h14M5 17h14" />
                <circle cx="9" cy="7" r="2.2" fill="currentColor" stroke="none" />
                <circle cx="15" cy="17" r="2.2" fill="currentColor" stroke="none" />
            </svg>
        }
        .into_any(),
        Page::Reports => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M4 19V5M4 19h16" />
                <path d="M8 15v-4M12 15V7M16 15v-6" />
            </svg>
        }
        .into_any(),
        Page::Sync => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M20 11a8 8 0 0 0-14-4M4 5v4h4M4 13a8 8 0 0 0 14 4M20 19v-4h-4" />
            </svg>
        }
        .into_any(),
    }
}

#[component]
fn PageBody(app: App) -> impl IntoView {
    view! {
        {move || match app.page.get() {
            Page::Sale => view! { <Sale app /> }.into_any(),
            Page::Held => view! { <Held app /> }.into_any(),
            Page::Stock => view! { <StockAdmin app /> }.into_any(),
            Page::Menu => view! { <MenuAdmin app /> }.into_any(),
            Page::Workers => view! { <WorkersAdmin app /> }.into_any(),
            Page::Settings => view! { <SettingsAdmin app /> }.into_any(),
            Page::Reports => view! { <ReportsAdmin app /> }.into_any(),
            Page::Sync => view! { <SyncPanel app /> }.into_any(),
        }}
    }
}

fn initials(name: &str) -> String {
    let mut letters = name
        .split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(2)
        .collect::<String>();
    if letters.is_empty() {
        letters.push('H');
    }
    letters.to_uppercase()
}

fn page_label(page: Page) -> &'static str {
    match page {
        Page::Sale => "Selling",
        Page::Held => "Parked tickets",
        Page::Stock => "Stock room",
        Page::Menu => "Menu and packs",
        Page::Workers => "People",
        Page::Settings => "Shop settings",
        Page::Reports => "Owner tools",
        Page::Sync => "Owner tools",
    }
}

fn page_title(page: Page) -> &'static str {
    match page {
        Page::Sale => "New sale",
        Page::Held => "Held orders",
        Page::Stock => "Stock",
        Page::Menu => "Menu",
        Page::Workers => "Workers",
        Page::Settings => "Settings",
        Page::Reports => "Reports",
        Page::Sync => "Cloud sync",
    }
}

#[component]
fn SyncPanel(app: App) -> impl IntoView {
    Effect::new(move |_| {
        if app.sync.get().is_some() {
            return;
        }
        app.busy.set(true);
        spawn_local(async move {
            match app.api().sync_status().await {
                Ok(status) => app.sync.set(Some(status)),
                Err(error) => app.fail(&error),
            }
            app.busy.set(false);
        });
    });

    view! {
        <div class="panel">
            <div class="panel-head">
                <h2>"Cloud sync"</h2>
            </div>
            {move || {
                app.sync
                    .get()
                    .map(|status| {
                        view! {
                            <div class="sync-grid">
                                <div>
                                    <p class="eyebrow">"State"</p>
                                    <p>{if status.enabled { "On" } else { "Off for the pilot" }}</p>
                                </div>
                                <div>
                                    <p class="eyebrow">"Waiting to send"</p>
                                    <p>{status.pending_count}</p>
                                </div>
                                <div>
                                    <p class="eyebrow">"Last success"</p>
                                    <p>{status.last_success_at.unwrap_or_else(|| "Never".to_owned())}</p>
                                </div>
                            </div>
                            <p class="muted small">
                                "Every order, payment and stock movement is recorded locally first. Sync will copy them once the shop owner switches it on."
                            </p>
                        }
                    })
            }}
            <button
                class="button ghost small"
                on:click=move |_| app.sync.set(None)
                disabled=move || app.busy.get()
            >
                "Refresh"
            </button>
        </div>
    }
}
