//! The owner's day book: what sold, how it was paid and the CSV exports.

use leptos::prelude::*;
use pos_types::{DailyReport, PaymentMethod};
use wasm_bindgen_futures::spawn_local;

use crate::components::{Empty, Spinner};
use crate::format::money;
use crate::state::{App, ToastKind};

#[derive(Debug, Clone, Copy)]
enum CsvKind {
    Sales,
    Stock,
}

fn today() -> String {
    (chrono::Utc::now() + chrono::Duration::hours(1))
        .format("%Y-%m-%d")
        .to_string()
}

fn method_label(method: PaymentMethod) -> &'static str {
    match method {
        PaymentMethod::Cash => "Cash",
        PaymentMethod::Transfer => "Transfer",
        PaymentMethod::Card => "Card",
    }
}

fn save_csv(filename: &str, csv: &str) -> Result<(), String> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or_else(|| "This window cannot save files.".to_owned())?;
    let document = window
        .document()
        .ok_or_else(|| "This window cannot save files.".to_owned())?;
    let parts = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(csv));
    let blob = web_sys::Blob::new_with_str_sequence(&parts)
        .map_err(|_| "The browser could not prepare the CSV.".to_owned())?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "The browser could not prepare the download.".to_owned())?;
    let anchor = document
        .create_element("a")
        .map_err(|_| "The browser could not prepare the download.".to_owned())?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "The browser could not prepare the download.".to_owned())?;
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}

#[component]
pub fn ReportsAdmin(app: App) -> impl IntoView {
    let date = RwSignal::new(today());
    let refresh = RwSignal::new(0_u32);
    let report = RwSignal::new(None::<DailyReport>);
    let loading = RwSignal::new(false);
    let exporting = RwSignal::new(false);

    Effect::new(move |_| {
        let selected = date.get();
        refresh.get();
        loading.set(true);
        spawn_local(async move {
            match app.api().daily_report(&selected).await {
                Ok(value) => report.set(Some(value)),
                Err(error) => app.fail(&error),
            }
            loading.set(false);
        });
    });

    let export = move |kind: CsvKind| {
        if exporting.get_untracked() {
            return;
        }
        exporting.set(true);
        let selected = date.get_untracked();
        spawn_local(async move {
            let (result, filename) = match kind {
                CsvKind::Sales => (
                    app.api().sales_csv(&selected).await,
                    format!("hiffey-sales-{selected}.csv"),
                ),
                CsvKind::Stock => (
                    app.api().stock_csv(&selected).await,
                    format!("hiffey-stock-{selected}.csv"),
                ),
            };
            match result {
                Ok(csv) => match save_csv(&filename, &csv) {
                    Ok(()) => app.toast(ToastKind::Success, format!("{filename} is ready.")),
                    Err(message) => app.toast(ToastKind::Error, message),
                },
                Err(error) => app.fail(&error),
            }
            exporting.set(false);
        });
    };

    view! {
        <div class="admin reports">
            <div class="panel">
                <div class="panel-head">
                    <div>
                        <p class="eyebrow">"Trading day"</p>
                        <h2>"Sales report"</h2>
                    </div>
                    <div class="report-actions">
                        <input
                            type="date"
                            class="report-date"
                            value=move || date.get()
                            on:change=move |event| date.set(event_target_value(&event))
                        />
                        <button
                            class="button ghost small"
                            on:click=move |_| refresh.update(|value| *value += 1)
                            disabled=move || loading.get()
                        >
                            "Refresh"
                        </button>
                    </div>
                </div>
                <div class="report-exports">
                    <button
                        class="button primary small"
                        on:click=move |_| export(CsvKind::Sales)
                        disabled=move || exporting.get()
                    >
                        {move || if exporting.get() { "Preparing…" } else { "Sales CSV" }}
                    </button>
                    <button
                        class="button ghost small"
                        on:click=move |_| export(CsvKind::Stock)
                        disabled=move || exporting.get()
                    >
                        "Stock CSV"
                    </button>
                    <span class="muted small">
                        "One row per order, plus a stock movement sheet for the same day."
                    </span>
                </div>
            </div>

            {move || {
                if loading.get() && report.get().is_none() {
                    return view! {
                        <div class="panel">
                            <Spinner />
                        </div>
                    }
                        .into_any();
                }
                let Some(data) = report.get() else {
                    return view! { <Empty text="No report loaded for this day." /> }.into_any();
                };
                if data.paid_orders == 0 && data.held_orders == 0 && data.voided_orders == 0 {
                    return view! {
                        <div class="panel">
                            <Empty text="No orders were recorded on this day." />
                        </div>
                    }
                        .into_any();
                }

                let payments = data.payments;
                let items = data.items;
                let order_mix = format!(
                    "{} dine-in · {} takeaway · {} held",
                    data.dine_in_orders, data.takeaway_orders, data.held_orders
                );
                view! {
                    <div class="report-kpis">
                        <div class="report-kpi">
                            <p class="eyebrow">"Net sales"</p>
                            <p class="report-big">{money(data.total_kobo)}</p>
                            <p class="muted small">{format!("{} paid tickets", data.paid_orders)}</p>
                        </div>
                        <div class="report-kpi">
                            <p class="eyebrow">"Average ticket"</p>
                            <p class="report-big">{money(data.average_kobo)}</p>
                            <p class="muted small">{order_mix}</p>
                        </div>
                        <div class="report-kpi">
                            <p class="eyebrow">"Packaging collected"</p>
                            <p class="report-big">{money(data.packaging_kobo)}</p>
                            <p class="muted small">"Takeaway only"</p>
                        </div>
                        <div class="report-kpi">
                            <p class="eyebrow">"Change handed back"</p>
                            <p class="report-big">{money(data.change_kobo)}</p>
                            <p class="muted small">{format!("{} taken in total", money(data.tendered_kobo))}</p>
                        </div>
                    </div>

                    {(data.voided_orders > 0)
                        .then(|| {
                            view! {
                                <p class="report-note">
                                    {format!(
                                        "{} voided ticket(s) worth {} are excluded from net sales.",
                                        data.voided_orders,
                                        money(data.voided_total_kobo),
                                    )}
                                </p>
                            }
                        })}

                    <div class="report-grid">
                        <div class="panel">
                            <div class="panel-head">
                                <h2>"Payments"</h2>
                                <span class="muted small">"Recorded amounts"</span>
                            </div>
                            <table class="table">
                                <thead>
                                    <tr>
                                        <th>"Method"</th>
                                        <th>"Orders"</th>
                                        <th>"Tendered"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {payments
                                        .into_iter()
                                        .map(|payment| {
                                            view! {
                                                <tr>
                                                    <td>{method_label(payment.method)}</td>
                                                    <td>{payment.orders}</td>
                                                    <td class="num">{money(payment.amount_kobo)}</td>
                                                </tr>
                                            }
                                        })
                                        .collect_view()}
                                </tbody>
                            </table>
                            <p class="muted small">
                                "Cash tender includes change, so the methods add up to more than net sales."
                            </p>
                        </div>

                        <div class="panel">
                            <div class="panel-head">
                                <h2>"Item mix"</h2>
                                <span class="muted small">"Paid tickets"</span>
                            </div>
                            {if items.is_empty() {
                                view! { <Empty text="Nothing sold yet on this day." /> }.into_any()
                            } else {
                                view! {
                                    <table class="table">
                                        <thead>
                                            <tr>
                                                <th>"Item"</th>
                                                <th>"Qty"</th>
                                                <th>"Value"</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {items
                                                .into_iter()
                                                .map(|item| {
                                                    view! {
                                                        <tr>
                                                            <td>{item.item_name}</td>
                                                            <td>{format!(
                                                                "{} {}",
                                                                item.quantity,
                                                                item.unit_type.label(),
                                                            )}</td>
                                                            <td class="num">{money(item.revenue_kobo)}</td>
                                                        </tr>
                                                    }
                                                })
                                                .collect_view()}
                                        </tbody>
                                    </table>
                                }
                                    .into_any()
                            }}
                        </div>
                    </div>
                }
                    .into_any()
            }}
        </div>
    }
}
