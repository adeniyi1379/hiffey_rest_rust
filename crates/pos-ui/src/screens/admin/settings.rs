//! Shop-wide settings written into receipts and stock warnings.

use leptos::prelude::*;
use pos_types::UpdateSettingsRequest;
use wasm_bindgen_futures::spawn_local;

use crate::components::Spinner;
use crate::format::{money, parse_naira};
use crate::state::{refresh_settings, App, ToastKind};

#[component]
pub fn SettingsAdmin(app: App) -> impl IntoView {
    let business_name = RwSignal::new(String::new());
    let address = RwSignal::new(String::new());
    let phone = RwSignal::new(String::new());
    let footer = RwSignal::new(String::new());
    let packaging = RwSignal::new(String::new());
    let low_stock = RwSignal::new(String::new());
    let loaded = RwSignal::new(false);
    let busy = RwSignal::new(false);

    Effect::new(move |_| {
        if loaded.get() {
            return;
        }
        let app = app;
        spawn_local(async move {
            if app.settings.get_untracked().is_none() {
                refresh_settings(app).await;
            }
            if let Some(settings) = app.settings.get_untracked() {
                business_name.set(settings.business_name);
                address.set(settings.business_address);
                phone.set(settings.business_phone);
                footer.set(settings.receipt_footer);
                packaging.set(format!(
                    "{}.{:02}",
                    settings.packaging_fee_kobo / 100,
                    settings.packaging_fee_kobo % 100
                ));
                low_stock.set(settings.low_stock_default.to_string());
                loaded.set(true);
            }
        });
    });

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let Some(fee) = parse_naira(&packaging.get_untracked()) else {
            app.toast(ToastKind::Error, "Enter a valid packaging fee.");
            return;
        };
        let Ok(low) = low_stock.get_untracked().trim().parse::<i64>() else {
            app.toast(
                ToastKind::Error,
                "Low stock warning must be a whole number.",
            );
            return;
        };
        let request = UpdateSettingsRequest {
            business_name: business_name.get_untracked().trim().to_owned(),
            business_address: address.get_untracked().trim().to_owned(),
            business_phone: phone.get_untracked().trim().to_owned(),
            receipt_footer: footer.get_untracked().trim().to_owned(),
            packaging_fee_kobo: fee,
            low_stock_default: low,
        };
        if request.business_name.is_empty() {
            app.toast(ToastKind::Error, "The shop needs a name.");
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match app.api().update_settings(&request).await {
                Ok(settings) => {
                    app.settings.set(Some(settings));
                    app.notify("Settings saved.");
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="admin">
            <div class="panel form-panel">
                <div class="panel-head">
                    <h2>"Shop details"</h2>
                    <span class="muted small">"These print on every receipt."</span>
                </div>
                <div class="form-grid">
                    <label class="field">
                        <span>"Shop name"</span>
                        <input
                            type="text"
                            prop:value=move || business_name.get()
                            on:input=move |event| business_name.set(event_target_value(&event))
                        />
                    </label>
                    <label class="field">
                        <span>"Phone"</span>
                        <input
                            type="text"
                            prop:value=move || phone.get()
                            on:input=move |event| phone.set(event_target_value(&event))
                        />
                    </label>
                    <label class="field span-two">
                        <span>"Address"</span>
                        <input
                            type="text"
                            prop:value=move || address.get()
                            on:input=move |event| address.set(event_target_value(&event))
                        />
                    </label>
                    <label class="field span-two">
                        <span>"Receipt footer"</span>
                        <textarea
                            rows="2"
                            prop:value=move || footer.get()
                            on:input=move |event| footer.set(event_target_value(&event))
                            placeholder="Thank you. Come again."
                        ></textarea>
                    </label>
                </div>

                <div class="panel-head sub">
                    <h2>"Selling rules"</h2>
                </div>
                <div class="form-grid">
                    <label class="field">
                        <span>"Takeaway packaging fee"</span>
                        <input
                            type="text"
                            inputmode="decimal"
                            prop:value=move || packaging.get()
                            on:input=move |event| packaging.set(event_target_value(&event))
                        />
                        <small class="muted">"Added to takeaway orders only. " {move || money(parse_naira(&packaging.get()).unwrap_or(0))}</small>
                    </label>
                    <label class="field">
                        <span>"Default low stock warning"</span>
                        <input
                            type="number"
                            prop:value=move || low_stock.get()
                            on:input=move |event| low_stock.set(event_target_value(&event))
                        />
                        <small class="muted">"Used for new items unless set differently."</small>
                    </label>
                </div>

                <button class="button primary" on:click=submit disabled=move || busy.get()>
                    {move || if busy.get() { view! { <Spinner /> }.into_any() } else { view! { "Save settings" }.into_any() }}
                </button>
            </div>
        </div>
    }
}
