//! Small shared pieces used across the screens.

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::state::{App, Toast, ToastKind};

#[component]
pub fn Spinner() -> impl IntoView {
    view! { <span class="spinner" aria-label="Working"></span> }
}

#[component]
pub fn Badge(#[prop(into)] text: String, #[prop(into)] tone: String) -> impl IntoView {
    view! { <span class=format!("badge badge-{tone}")>{text}</span> }
}

#[component]
pub fn Modal(
    #[prop(into)] title: String,
    #[prop(optional)] wide: bool,
    on_close: Callback<()>,
    children: Children,
) -> impl IntoView {
    let class = if wide { "modal modal-wide" } else { "modal" };
    let heading = title.clone();
    let dialog = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        if let Some(element) = dialog.get() {
            let _ = element.focus();
        }
    });
    view! {
        <div
            class="overlay"
            role="dialog"
            aria-modal="true"
            aria-label=title
            tabindex="-1"
            node_ref=dialog
            on:keydown=move |event| {
                if event.key() == "Escape" {
                    on_close.run(());
                }
            }
            on:click=move |_| on_close.run(())
        >
            <div class=class on:click=|event| event.stop_propagation()>
                <header class="modal-head">
                    <h2>{heading}</h2>
                    <button class="icon-button" on:click=move |_| on_close.run(()) aria-label="Close">
                        "\u{00d7}"
                    </button>
                </header>
                <div class="modal-body">{children()}</div>
            </div>
        </div>
    }
}

#[component]
pub fn ToastHost(app: App) -> impl IntoView {
    view! {
        <div class="toasts" aria-live="polite">
            {move || {
                app.toasts
                    .get()
                    .into_iter()
                    .map(|toast| view! { <ToastCard app toast /> })
                    .collect_view()
            }}
        </div>
    }
}

#[component]
fn ToastCard(app: App, toast: Toast) -> impl IntoView {
    let id = toast.id;
    let (class, role) = match toast.kind {
        ToastKind::Success => ("toast toast-success", "status"),
        ToastKind::Error => ("toast toast-error", "alert"),
    };
    spawn_local(async move {
        TimeoutFuture::new(4_500).await;
        app.toasts.update(|list| list.retain(|item| item.id != id));
    });
    view! {
        <div class=class role=role on:click=move |_| {
            app.toasts.update(|list| list.retain(|item| item.id != id));
        }>
            {toast.message}
        </div>
    }
}

#[component]
pub fn Empty(#[prop(into)] text: String) -> impl IntoView {
    view! { <p class="empty">{text}</p> }
}
