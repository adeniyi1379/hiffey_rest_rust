//! People who use the till, their roles and their PINs.

use leptos::prelude::*;
use pos_types::{CreateWorkerRequest, Role, WorkerSummary};
use wasm_bindgen_futures::spawn_local;

use crate::components::{Badge, Empty, Modal, Spinner};
use crate::state::{refresh_workers, App, ToastKind};

#[component]
pub fn WorkersAdmin(app: App) -> impl IntoView {
    let adding = RwSignal::new(false);
    let created_pin = RwSignal::new(None::<(String, String)>);
    let deactivating = RwSignal::new(None::<WorkerSummary>);
    let busy = RwSignal::new(false);

    Effect::new(move |_| {
        let app = app;
        spawn_local(async move {
            refresh_workers(app).await;
        });
    });

    let reset_pin = move |worker: WorkerSummary| {
        busy.set(true);
        spawn_local(async move {
            match app.api().reset_pin(&worker.id).await {
                Ok(response) => {
                    created_pin.set(Some((worker.name, response.pin)));
                    refresh_workers(app).await;
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    let confirm_deactivate = move |_| {
        let Some(worker) = deactivating.get_untracked() else {
            return;
        };
        busy.set(true);
        spawn_local(async move {
            match app.api().deactivate_worker(&worker.id).await {
                Ok(()) => {
                    app.notify(format!("{} can no longer sign in.", worker.name));
                    deactivating.set(None);
                    refresh_workers(app).await;
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="admin">
            <div class="panel">
                <div class="panel-head">
                    <h2>"Workers"</h2>
                    <button class="button primary small" on:click=move |_| adding.set(true)>
                        "Add worker"
                    </button>
                </div>
                <table class="table">
                    <thead>
                        <tr>
                            <th>"Name"</th>
                            <th>"Role"</th>
                            <th>"Status"</th>
                            <th>"PIN reset"</th>
                            <th></th>
                        </tr>
                    </thead>
                    <tbody>
                        {move || {
                            let workers = app.workers.get();
                            if workers.is_empty() {
                                return view! {
                                    <tr><td colspan="5"><Empty text="No workers yet." /></td></tr>
                                }.into_any();
                            }
                            let me = app.user().map(|user| user.id);
                            workers
                                .into_iter()
                                .map(|worker| {
                                    let is_me = me.as_deref() == Some(worker.id.as_str());
                                    let pin_row = worker.clone();
                                    let deactivate_row = worker.clone();
                                    view! {
                                        <tr>
                                            <td>
                                                {worker.name.clone()}
                                                {is_me.then(|| view! { <span class="muted small">" (you)"</span> })}
                                            </td>
                                            <td class="capitalize">{worker.role.to_string()}</td>
                                            <td>
                                                {if worker.active {
                                                    view! { <Badge text="Active" tone="good" /> }.into_any()
                                                } else {
                                                    view! { <Badge text="Inactive" tone="neutral" /> }.into_any()
                                                }}
                                            </td>
                                            <td>{worker.pin_reset_at.as_ref().map(|value| crate::format::short_time(value)).unwrap_or_else(|| "—".to_owned())}</td>
                                            <td>
                                                <div class="row gap tight">
                                                    <button
                                                        class="link"
                                                        on:click=move |_| reset_pin(pin_row.clone())
                                                        disabled=move || busy.get()
                                                    >
                                                        "New PIN"
                                                    </button>
                                                    {worker.active.then(|| view! {
                                                        <button
                                                            class="link danger-link"
                                                            on:click=move |_| deactivating.set(Some(deactivate_row.clone()))
                                                            disabled=move || busy.get()
                                                        >
                                                            "Deactivate"
                                                        </button>
                                                    })}
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

            {move || {
                adding
                    .get()
                    .then(|| {
                        view! {
                            <AddWorker app on_close=Callback::new(move |_| adding.set(false)) />
                        }
                    })
            }}

            {move || {
                created_pin
                    .get()
                    .map(|(name, pin)| {
                        let close = Callback::new(move |_| created_pin.set(None));
                        view! {
                            <Modal title="PIN shown once" on_close=close>
                                <p>{format!("Write this PIN down for {name}. It cannot be looked up later.")}</p>
                                <p class="pin-display">{pin}</p>
                                <p class="muted small">"They can change nothing here; ask a manager if it is lost and a new one is needed."</p>
                                <button class="button primary wide" on:click=move |_| close.run(())>"Done"</button>
                            </Modal>
                        }
                    })
            }}

            {move || {
                deactivating
                    .get()
                    .map(|worker| {
                        let name = worker.name.clone();
                        view! {
                            <Modal title="Deactivate this worker?" on_close=Callback::new(move |_| deactivating.set(None))>
                                <p>{format!("{name} will lose access immediately. Their past orders stay on record.")}</p>
                                <div class="row gap">
                                    <button class="button ghost" on:click=move |_| deactivating.set(None)>"Keep access"</button>
                                    <button class="button danger" on:click=confirm_deactivate disabled=move || busy.get()>
                                        "Deactivate"
                                    </button>
                                </div>
                            </Modal>
                        }
                    })
            }}
        </div>
    }
}

#[component]
fn AddWorker(app: App, on_close: Callback<()>) -> impl IntoView {
    let name = RwSignal::new(String::new());
    let role = RwSignal::new(Role::Cashier);
    let busy = RwSignal::new(false);
    let pin = RwSignal::new(None::<String>);

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let name_text = name.get_untracked().trim().to_owned();
        if name_text.is_empty() {
            app.toast(ToastKind::Error, "Enter the worker's name.");
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match app
                .api()
                .create_worker(&CreateWorkerRequest {
                    name: name_text,
                    role: role.get_untracked(),
                })
                .await
            {
                Ok(created) => {
                    pin.set(Some(created.pin));
                    refresh_workers(app).await;
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    view! {
        <Modal title="New worker" on_close=on_close>
            {move || {
                pin.get()
                    .map(|pin| {
                        let close = Callback::new(move |_| on_close.run(()));
                        view! {
                            <div>
                                <p>"The PIN is shown once. Write it down now."</p>
                                <p class="pin-display">{pin}</p>
                                <button class="button primary wide" on:click=move |_| close.run(())>"Done"</button>
                            </div>
                        }
                        .into_any()
                    })
                    .unwrap_or_else(|| {
                        view! {
                            <div>
                                <label class="field">
                                    <span>"Name"</span>
                                    <input
                                        type="text"
                                        prop:value=move || name.get()
                                        on:input=move |event| name.set(event_target_value(&event))
                                    />
                                </label>
                                <label class="field">
                                    <span>"Role"</span>
                                    <select on:change=move |event| {
                                        role.set(if event_target_value(&event) == "owner" {
                                            Role::Owner
                                        } else if event_target_value(&event) == "manager" {
                                            Role::Manager
                                        } else {
                                            Role::Cashier
                                        });
                                    }>
                                        <option value="cashier" prop:selected=move || role.get() == Role::Cashier>"Cashier"</option>
                                        <option value="manager" prop:selected=move || role.get() == Role::Manager>"Manager"</option>
                                        <option value="owner" prop:selected=move || role.get() == Role::Owner>"Owner"</option>
                                    </select>
                                </label>
                                <button class="button primary wide" on:click=submit disabled=move || busy.get()>
                                    {move || if busy.get() { view! { <Spinner /> }.into_any() } else { view! { "Create PIN" }.into_any() }}
                                </button>
                            </div>
                        }
                        .into_any()
                    })
            }}
        </Modal>
    }
}
