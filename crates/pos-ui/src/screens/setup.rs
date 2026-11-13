//! First run on Terminal A: name the shop and create the owner account.

use leptos::prelude::*;
use pos_types::SetupRequest;
use wasm_bindgen_futures::spawn_local;

use crate::components::Spinner;
use crate::state::{refresh_held, refresh_menu, refresh_settings, App, Screen};

#[component]
pub fn Setup(app: App) -> impl IntoView {
    let business_name = RwSignal::new(String::new());
    let owner_name = RwSignal::new(String::new());
    let address = RwSignal::new(String::new());
    let phone = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let created = RwSignal::new(None::<pos_types::SetupResponse>);

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let request = SetupRequest {
            business_name: business_name.get_untracked().trim().to_owned(),
            owner_name: owner_name.get_untracked().trim().to_owned(),
            business_address: address.get_untracked().trim().to_owned(),
            business_phone: phone.get_untracked().trim().to_owned(),
        };
        if request.business_name.is_empty() || request.owner_name.is_empty() {
            app.toast(
                crate::state::ToastKind::Error,
                "Enter the shop name and the owner's name.",
            );
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match app.api_anonymous().complete_setup(&request).await {
                Ok(response) => {
                    app.online.set(true);
                    created.set(Some(response));
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    let take_over = move |_| {
        if let Some(response) = created.get_untracked() {
            app.start_session(response.session);
            spawn_local(async move {
                refresh_menu(app).await;
                refresh_held(app).await;
                refresh_settings(app).await;
            });
        }
    };

    view! {
        <main class="centered-card">
            <header class="setup-head">
                <p class="eyebrow">"First time here"</p>
                <h1>"Set up the shop"</h1>
                <p class="muted">
                    "This creates the owner account on this PC. You will see the owner PIN once — write it down."
                </p>
            </header>

            <div class="banner banner-info">
                <span>"Server: " <strong>{move || app.config.get().base_url}</strong></span>
                <button
                    class="link"
                    on:click=move |_| app.screen.set(Screen::Connection)
                >
                    "Change"
                </button>
            </div>

            <label class="field">
                <span>"Shop name"</span>
                <input
                    type="text"
                    prop:value=move || business_name.get()
                    on:input=move |event| business_name.set(event_target_value(&event))
                    placeholder="Hiffey Restaurant"
                />
            </label>
            <label class="field">
                <span>"Owner's name"</span>
                <input
                    type="text"
                    prop:value=move || owner_name.get()
                    on:input=move |event| owner_name.set(event_target_value(&event))
                    placeholder="Full name"
                />
            </label>
            <div class="row gap">
                <label class="field grow">
                    <span>"Shop address (receipt)"</span>
                    <input
                        type="text"
                        prop:value=move || address.get()
                        on:input=move |event| address.set(event_target_value(&event))
                    />
                </label>
                <label class="field grow">
                    <span>"Phone (receipt)"</span>
                    <input
                        type="text"
                        prop:value=move || phone.get()
                        on:input=move |event| phone.set(event_target_value(&event))
                    />
                </label>
            </div>

            <button class="button primary wide" on:click=submit disabled=move || busy.get()>
                {move || {
                    if busy.get() {
                        view! { <Spinner /> }.into_any()
                    } else {
                        view! { "Create shop and owner" }.into_any()
                    }
                }}
            </button>

            {move || {
                created
                    .get()
                    .map(|response| {
                        let pin = response.pin.clone();
                        view! {
                            <div class="overlay">
                                <div class="modal">
                                    <header class="modal-head">
                                        <h2>"Owner PIN"</h2>
                                    </header>
                                    <div class="modal-body">
                                        <p>"Write this PIN down and keep it safe. It is shown only once."</p>
                                        <p class="pin-display">{pin}</p>
                                        <p class="muted">
                                            "Use it to sign in and to approve paid voids, resets and settings."
                                        </p>
                                        <button class="button primary wide" on:click=take_over>
                                            "I have written it down — open the till"
                                        </button>
                                    </div>
                                </div>
                            </div>
                        }
                    })
            }}
        </main>
    }
}
