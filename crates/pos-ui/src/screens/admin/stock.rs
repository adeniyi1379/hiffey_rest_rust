//! The stock room: balances, movements and end-of-day counts.

use leptos::prelude::*;
use pos_types::{
    RecordBatchRequest, RestockRequest, StockBalanceDto, StockCountInput, StockCountRequest,
    StockCountResultDto, StockLedgerEntryDto, WastageRequest,
};
use wasm_bindgen_futures::spawn_local;

use crate::components::{Badge, Empty, Modal, Spinner};
use crate::format::short_time;
use crate::state::{refresh_balances, refresh_menu, App, ToastKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Balances,
    History,
    Count,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Movement {
    Batch,
    Restock,
    Wastage,
}

#[derive(Clone)]
struct MovementForm {
    item: StockBalanceDto,
    kind: Movement,
}

#[component]
pub fn StockAdmin(app: App) -> impl IntoView {
    let tab = RwSignal::new(Tab::Balances);
    let movement = RwSignal::new(None::<MovementForm>);
    let ledger = RwSignal::new(Vec::<StockLedgerEntryDto>::new());
    let loading = RwSignal::new(false);
    let counts = RwSignal::new(Vec::<(StockBalanceDto, String)>::new());
    let results = RwSignal::new(Vec::<StockCountResultDto>::new());
    let busy = RwSignal::new(false);

    Effect::new(move |_| {
        let app = app;
        spawn_local(async move {
            refresh_balances(app).await;
        });
    });

    Effect::new(move |_| {
        if tab.get() != Tab::History {
            return;
        }
        loading.set(true);
        spawn_local(async move {
            match app.api().stock_ledger(None).await {
                Ok(entries) => ledger.set(entries),
                Err(error) => app.fail(&error),
            }
            loading.set(false);
        });
    });

    let open = move |item: StockBalanceDto, kind: Movement| {
        movement.set(Some(MovementForm { item, kind }));
    };

    let start_count = move |_| {
        let rows = app
            .balances
            .get_untracked()
            .into_iter()
            .map(|balance| (balance, String::new()))
            .collect();
        counts.set(rows);
        results.set(Vec::new());
    };

    let submit_count = move |_| {
        if busy.get_untracked() {
            return;
        }
        let mut entries = Vec::new();
        for (balance, value) in counts.get_untracked() {
            if value.trim().is_empty() {
                continue;
            }
            let Ok(counted) = value.trim().parse::<i64>() else {
                app.toast(ToastKind::Error, "Counts must be whole numbers.");
                return;
            };
            entries.push(StockCountInput {
                item_id: balance.item_id,
                counted_qty: counted,
            });
        }
        if entries.is_empty() {
            app.toast(ToastKind::Error, "Enter at least one counted quantity.");
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match app
                .api()
                .stock_count(&StockCountRequest {
                    counts: entries,
                    reason: None,
                })
                .await
            {
                Ok(result) => {
                    results.set(result);
                    app.notify("Count saved.");
                    counts.set(Vec::new());
                    refresh_balances(app).await;
                    refresh_menu(app).await;
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="admin">
            <div class="segmented wide-segments">
                <button
                    class=move || if tab.get() == Tab::Balances { "segment active" } else { "segment" }
                    on:click=move |_| tab.set(Tab::Balances)
                >
                    "On the shelf"
                </button>
                <button
                    class=move || if tab.get() == Tab::History { "segment active" } else { "segment" }
                    on:click=move |_| tab.set(Tab::History)
                >
                    "History"
                </button>
                <button
                    class=move || if tab.get() == Tab::Count { "segment active" } else { "segment" }
                    on:click=move |_| tab.set(Tab::Count)
                >
                    "End-of-day count"
                </button>
            </div>

            {move || match tab.get() {
                Tab::Balances => {
                    view! {
                        <div class="panel">
                            <div class="panel-head">
                                <h2>"Balances"</h2>
                                <button
                                    class="button ghost small"
                                    on:click=move |_| {
                                        let app = app;
                                        spawn_local(async move {
                                            refresh_balances(app).await;
                                        });
                                    }
                                >
                                    "Refresh"
                                </button>
                            </div>
                            <table class="table">
                                <thead>
                                    <tr>
                                        <th>"Item"</th>
                                        <th>"Unit"</th>
                                        <th>"On shelf"</th>
                                        <th>"Warning at"</th>
                                        <th></th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {move || {
                                        let rows = app.balances.get();
                                        if rows.is_empty() {
                                            return view! {
                                                <tr><td colspan="5"><Empty text="No stock recorded yet." /></td></tr>
                                            }.into_any();
                                        }
                                        rows.into_iter()
                                            .map(|balance| {
                                                let low = balance.balance <= balance.low_stock_level;
                                                let batch_row = balance.clone();
                                                let restock_row = balance.clone();
                                                let wastage_row = balance.clone();
                                                view! {
                                                    <tr>
                                                        <td>{balance.item_name.clone()}</td>
                                                        <td>{balance.unit_type.label()}</td>
                                                        <td>
                                                            <strong>{balance.balance}</strong>
                                                            {low.then(|| view! {
                                                                <Badge text="Low" tone="warn" />
                                                            })}
                                                        </td>
                                                        <td>{balance.low_stock_level}</td>
                                                        <td>
                                                            <div class="row gap tight">
                                                                <button class="link" on:click=move |_| open(batch_row.clone(), Movement::Batch)>
                                                                    "Opening batch"
                                                                </button>
                                                                <button class="link" on:click=move |_| open(restock_row.clone(), Movement::Restock)>
                                                                    "Restock"
                                                                </button>
                                                                <button class="link danger-link" on:click=move |_| open(wastage_row.clone(), Movement::Wastage)>
                                                                    "Wastage"
                                                                </button>
                                                            </div>
                                                        </td>
                                                    </tr>
                                                }
                                            })
                                            .collect_view()
                                            .into_any()
                                    }}
                                </tbody>
                            </table>
                        </div>
                    }
                    .into_any()
                }
                Tab::History => {
                    view! {
                        <div class="panel">
                            <div class="panel-head">
                                <h2>"Recent movements"</h2>
                                {move || loading.get().then(|| view! { <Spinner /> })}
                            </div>
                            {move || {
                                let entries = ledger.get();
                                if entries.is_empty() {
                                    return view! { <Empty text="Nothing recorded yet." /> }.into_any();
                                }
                                view! {
                                    <table class="table">
                                        <thead>
                                            <tr>
                                                <th>"When"</th>
                                                <th>"Item"</th>
                                                <th>"Movement"</th>
                                                <th>"Change"</th>
                                                <th>"Reason"</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {entries
                                                .into_iter()
                                                .map(|entry| {
                                                    let sign = if entry.quantity > 0 { "+" } else { "" };
                                                    view! {
                                                        <tr>
                                                            <td>{short_time(&entry.created_at)}</td>
                                                            <td>{entry.item_name}</td>
                                                            <td>{entry.movement_type.clone()}</td>
                                                            <td class=if entry.quantity < 0 { "negative" } else { "positive" }>
                                                                {format!("{sign}{}", entry.quantity)}
                                                            </td>
                                                            <td>{entry.reason.unwrap_or_default()}</td>
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
                    }
                    .into_any()
                }
                Tab::Count => {
                    view! {
                        <div class="panel">
                            <div class="panel-head">
                                <h2>"Count what is on the shelf"</h2>
                                {move || {
                                    (counts.get().is_empty()).then(|| view! {
                                        <button class="button primary small" on:click=start_count>"Start count"</button>
                                    })
                                }}
                            </div>

                            {move || {
                                let rows = counts.get();
                                if rows.is_empty() {
                                    return view! { <Empty text="Start a count to walk the shelf item by item." /> }.into_any();
                                }
                                view! {
                                    <table class="table">
                                        <thead>
                                            <tr>
                                                <th>"Item"</th>
                                                <th>"Expected"</th>
                                                <th>"Counted"</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {rows
                                                .into_iter()
                                                .enumerate()
                                                .map(|(index, (balance, value))| {
                                                    view! {
                                                        <tr>
                                                            <td>{balance.item_name.clone()}</td>
                                                            <td>{balance.balance}</td>
                                                            <td>
                                                                <input
                                                                    class="count-input"
                                                                    type="number"
                                                                    prop:value=value
                                                                    on:input=move |event| {
                                                                        let value = event_target_value(&event);
                                                                        counts.update(|rows| {
                                                                            if let Some((_, cell)) = rows.get_mut(index) {
                                                                                *cell = value;
                                                                            }
                                                                        });
                                                                    }
                                                                />
                                                            </td>
                                                        </tr>
                                                    }
                                                })
                                                .collect_view()}
                                        </tbody>
                                    </table>
                                    <button class="button primary" on:click=submit_count disabled=move || busy.get()>
                                        "Save count"
                                    </button>
                                }
                                .into_any()
                            }}

                            {move || {
                                let results = results.get();
                                if results.is_empty() {
                                    return None;
                                }
                                Some(
                                    view! {
                                        <div class="count-results">
                                            <p class="eyebrow">"Saved differences"</p>
                                            {results
                                                .into_iter()
                                                .map(|row| {
                                                    view! {
                                                        <div class="total-row">
                                                            <span>{row.item_name}</span>
                                                            <span class=if row.difference < 0 { "negative" } else { "positive" }>
                                                                {format!("{} -> {} ({:+})", row.expected_qty, row.counted_qty, row.difference)}
                                                            </span>
                                                        </div>
                                                    }
                                                })
                                                .collect_view()}
                                        </div>
                                    }
                                    .into_any(),
                                )
                            }}
                        </div>
                    }
                    .into_any()
                }
            }}

            {move || {
                movement
                    .get()
                    .map(|form| {
                        view! {
                            <MovementModal
                                app
                                form
                                on_close=Callback::new(move |_| movement.set(None))
                            />
                        }
                    })
            }}
        </div>
    }
}

#[component]
fn MovementModal(app: App, form: MovementForm, on_close: Callback<()>) -> impl IntoView {
    let item = form.item.clone();
    let kind = form.kind;
    let quantity = RwSignal::new(String::new());
    let reason = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let title = match kind {
        Movement::Batch => format!("Opening batch · {}", item.item_name),
        Movement::Restock => format!("Restock · {}", item.item_name),
        Movement::Wastage => format!("Wastage · {}", item.item_name),
    };

    let submit_item = item.clone();
    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let Ok(qty) = quantity.get_untracked().trim().parse::<i64>() else {
            app.toast(ToastKind::Error, "Enter a whole quantity.");
            return;
        };
        if qty <= 0 {
            app.toast(ToastKind::Error, "Quantity must be more than zero.");
            return;
        }
        let reason_text = reason.get_untracked().trim().to_owned();
        if kind == Movement::Wastage && reason_text.is_empty() {
            app.toast(ToastKind::Error, "Say why the stock left the shelf.");
            return;
        }
        busy.set(true);
        let item = submit_item.clone();
        spawn_local(async move {
            let api = app.api();
            let result = match kind {
                Movement::Batch => {
                    api.add_batch(&RecordBatchRequest {
                        item_id: item.item_id.clone(),
                        quantity: qty,
                        reason: (!reason_text.is_empty()).then_some(reason_text),
                    })
                    .await
                }
                Movement::Restock => {
                    api.restock(&RestockRequest {
                        item_id: item.item_id.clone(),
                        quantity: qty,
                        reason: (!reason_text.is_empty()).then_some(reason_text),
                    })
                    .await
                }
                Movement::Wastage => {
                    api.wastage(&WastageRequest {
                        item_id: item.item_id.clone(),
                        quantity: qty,
                        reason: reason_text,
                    })
                    .await
                }
            };
            match result {
                Ok(_) => {
                    app.notify(format!("{} updated.", item.item_name));
                    on_close.run(());
                    refresh_balances(app).await;
                    refresh_menu(app).await;
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    view! {
        <Modal title=title on_close=on_close>
            <p class="muted">
                {format!(
                    "Currently {} {} on the shelf, sold by the {}.",
                    item.balance,
                    item.item_name,
                    item.unit_type.label(),
                )}
            </p>
            <label class="field">
                <span>"Quantity"</span>
                <input
                    type="number"
                    min="1"
                    prop:value=move || quantity.get()
                    on:input=move |event| quantity.set(event_target_value(&event))
                />
            </label>
            <label class="field">
                <span>{if kind == Movement::Wastage { "Reason (required)" } else { "Reason (optional)" }}</span>
                <input
                    type="text"
                    prop:value=move || reason.get()
                    on:input=move |event| reason.set(event_target_value(&event))
                    placeholder=if kind == Movement::Wastage { "spillage, staff meal, mistake…" } else { "" }
                />
            </label>
            <p class="muted small">
                {match kind {
                    Movement::Batch => "Use this once when opening the shop, for what is already in the kitchen.",
                    Movement::Restock => "Adds newly bought stock to the shelf.",
                    Movement::Wastage => "Takes spoiled or given-away food off the shelf.",
                }}
            </p>
            <button class="button primary wide" on:click=submit disabled=move || busy.get()>
                {move || if busy.get() { view! { <Spinner /> }.into_any() } else { view! { "Save" }.into_any() }}
            </button>
        </Modal>
    }
}
