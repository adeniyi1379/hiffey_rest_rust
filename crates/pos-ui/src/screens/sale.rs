//! The till: pick food, take payment, park a ticket.

use leptos::prelude::*;
use pos_types::{
    CreateOrderRequest, CreatePrintJobRequest, OrderDto, OrderType, PayOrderRequest, PaymentInput,
    PaymentMethod, PrintJobType, UpdateOrderRequest, VoidOrderRequest,
};
use wasm_bindgen_futures::spawn_local;

use crate::api::ApiError;
use crate::cart::{Cart, LineKind};
use crate::components::{Empty, Modal, Spinner};
use crate::format::{money, short_time};
use crate::state::{
    refresh_balances, refresh_held, refresh_menu, App, Page, ToastKind, PACKS_CATEGORY,
};

fn methods() -> [(PaymentMethod, &'static str); 3] {
    [
        (PaymentMethod::Cash, "Cash"),
        (PaymentMethod::Transfer, "Transfer"),
        (PaymentMethod::Card, "Card"),
    ]
}

#[component]
fn ProductArtwork(app: App, url: Option<String>, glyph: Callback<(), AnyView>) -> impl IntoView {
    let absolute = url.map(|url| app.api().asset_url(&url));
    let show = RwSignal::new(absolute.is_some());
    view! {
        {move || {
            if show.get() {
                absolute
                    .clone()
                    .map(|source| {
                        view! {
                            <img
                                class="product-photo"
                                src=source
                                alt=""
                                on:error=move |_| show.set(false)
                            />
                        }
                            .into_any()
                    })
                    .unwrap_or_else(|| {
                        view! { <span class="product-glyph">{glyph.run(())}</span> }.into_any()
                    })
            } else {
                view! { <span class="product-glyph">{glyph.run(())}</span> }.into_any()
            }
        }}
    }
}

#[component]
pub fn Sale(app: App) -> impl IntoView {
    let payment_open = RwSignal::new(false);
    let processing = RwSignal::new(false);
    let paid = RwSignal::new(None::<String>);
    let table_error = RwSignal::new(None::<String>);
    let search = RwSignal::new(String::new());
    let hide_sold_out = RwSignal::new(false);
    let preferred = RwSignal::new(PaymentMethod::Cash);
    let table_edit = RwSignal::new(app.cart.get_untracked().table_no.trim().is_empty());

    let subtotal = Signal::derive(move || app.cart.get().subtotal_kobo());
    let fee = Signal::derive(move || {
        app.cart
            .get()
            .packaging_fee_kobo(app.settings.get().as_ref())
    });
    let total = Signal::derive(move || subtotal.get() + fee.get());

    let open_payment = move |_| {
        if app.cart.get_untracked().is_empty() {
            app.toast(ToastKind::Error, "Add something to the ticket first.");
            return;
        }
        if !table_ok(app) {
            return;
        }
        payment_open.set(true);
    };

    let hold = move |_| {
        if app.cart.get_untracked().is_empty() {
            app.toast(ToastKind::Error, "Add something to the ticket first.");
            return;
        }
        if !table_ok(app) {
            return;
        }
        processing.set(true);
        spawn_local(async move {
            let cart = app.cart.get_untracked();
            match persist(app, &cart).await {
                Ok(order) => {
                    app.toast(ToastKind::Success, format!("Held as {}.", order.order_no));
                    app.cart.update(|cart| cart.reset());
                    refresh_held(app).await;
                    refresh_menu(app).await;
                }
                Err(error) => app.fail(&error),
            }
            processing.set(false);
        });
    };

    view! {
        <div class="sale">
            <section class="menu-zone">
                <div class="menu-toolbar">
                    <label class="search-field">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
                            <circle cx="11" cy="11" r="6.5" />
                            <path d="m16 16 4 4" />
                        </svg>
                        <input
                            type="search"
                            placeholder="Search dishes"
                            aria-label="Search the menu"
                            prop:value=move || search.get()
                            on:input=move |event| search.set(event_target_value(&event))
                        />
                        {move || {
                            (!search.get().is_empty())
                                .then(|| {
                                    view! {
                                        <button
                                            type="button"
                                            class="search-clear"
                                            aria-label="Clear search"
                                            on:click=move |_| search.set(String::new())
                                        >
                                            "\u{00d7}"
                                        </button>
                                    }
                                })
                        }}
                    </label>
                    <button
                        type="button"
                        class=move || {
                            if hide_sold_out.get() { "filter-button active" } else { "filter-button" }
                        }
                        aria-pressed=move || hide_sold_out.get().to_string()
                        title="Show only dishes that are in stock"
                        on:click=move |_| hide_sold_out.update(|on| *on = !*on)
                    >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
                            <path d="M4 7h10M18 7h2M4 17h2M10 17h10" />
                            <circle cx="16" cy="7" r="2" />
                            <circle cx="8" cy="17" r="2" />
                        </svg>
                        <span>"Available"</span>
                    </button>
                </div>

                <CategoryTabs app />

                <div class="menu-scroll">
                    {move || {
                        let Some(menu) = app.menu.get() else {
                            return view! { <p class="soon">"Loading the menu…"</p> }.into_any();
                        };
                        let selected = app.selected_category.get();
                        let needle = search.get().trim().to_lowercase();
                        let hide = hide_sold_out.get();
                        if selected.as_deref() == Some(PACKS_CATEGORY) {
                            let packs = menu
                                .packs
                                .iter()
                                .filter(|pack| {
                                    pack.active
                                        && (needle.is_empty()
                                            || pack.name.to_lowercase().contains(&needle))
                                        && (!hide || !pack.sold_out)
                                })
                                .cloned()
                                .collect::<Vec<_>>();
                            if packs.is_empty() {
                                return view! { <Empty text="No packs match this view." /> }.into_any();
                            }
                            return view! {
                                <div class="product-grid">
                                    {packs
                                        .into_iter()
                                        .map(|pack| {
                                            let label = pack.components
                                                .iter()
                                                .map(|component| {
                                                    format!(
                                                        "{} {}",
                                                        component.quantity,
                                                        component.item_name
                                                    )
                                                })
                                                .collect::<Vec<_>>()
                                                .join(", ");
                                            let id = pack.id.clone();
                                            let qty_id = id.clone();
                                            let qty = Signal::derive(move || {
                                                line_quantity(&app, LineKind::Pack, &qty_id)
                                            });
                                            let add_pack = pack.clone();
                                            let add = Callback::new(move |_| {
                                                app.cart.update(|cart| cart.add_pack(&add_pack, 1));
                                            });
                                            let step_id = id.clone();
                                            let dec = Callback::new(move |_| {
                                                step_line(&app, LineKind::Pack, &step_id, -1);
                                            });
                                            let inc_id = id.clone();
                                            let inc = Callback::new(move |_| {
                                                step_line(&app, LineKind::Pack, &inc_id, 1);
                                            });
                                            let sold = pack.sold_out;
                                            view! {
                                                <article
                                                    class=move || {
                                                        let mut classes = String::from("product-card");
                                                        if qty.get() > 0 {
                                                            classes.push_str(" selected");
                                                        }
                                                        if sold {
                                                            classes.push_str(" sold-out");
                                                        }
                                                        classes
                                                    }
                                                >
                                                    <div class="product-media">
                                                        <ProductArtwork
                                                            app
                                                            url=pack.image_url.clone()
                                                            glyph=Callback::new(move |_| pack_icon())
                                                        />
                                                        {sold
                                                            .then(|| view! { <span class="product-flag">"Sold out"</span> })}
                                                    </div>
                                                    <div class="product-info">
                                                        <h3>{pack.name.clone()}</h3>
                                                        <p class="product-detail">{label}</p>
                                                        <div class="product-bottom">
                                                            <span class="product-price">{money(pack.price_kobo)}</span>
                                                            <span class="unit-tag">"Pack"</span>
                                                        </div>
                                                    </div>
                                                    <div class="product-actions">
                                                        <button
                                                            class=move || {
                                                                if qty.get() > 0 { "add-dish is-hidden" } else { "add-dish" }
                                                            }
                                                            disabled=sold
                                                            on:click=move |_| add.run(())
                                                        >
                                                            "Add to dish"
                                                        </button>
                                                        <div
                                                            class=move || {
                                                                if qty.get() == 0 { "stepper is-hidden" } else { "stepper" }
                                                            }
                                                        >
                                                            <button
                                                                class="step"
                                                                aria-label="Less"
                                                                on:click=move |_| dec.run(())
                                                            >
                                                                "\u{2212}"
                                                            </button>
                                                            <span class="qty">{move || qty.get()}</span>
                                                            <button
                                                                class="step"
                                                                aria-label="More"
                                                                on:click=move |_| inc.run(())
                                                            >
                                                                "+"
                                                            </button>
                                                        </div>
                                                    </div>
                                                </article>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            }
                            .into_any();
                        }
                        let items = menu
                            .items
                            .iter()
                            .filter(|item| {
                                item.active
                                    && selected.as_ref().is_none_or(|id| &item.category_id == id)
                                    && (needle.is_empty()
                                        || item.name.to_lowercase().contains(&needle))
                                    && (!hide || !item.sold_out)
                            })
                            .cloned()
                            .collect::<Vec<_>>();
                        if items.is_empty() {
                            if !needle.is_empty() || hide {
                                return view! {
                                    <div class="empty">
                                        <em>"No dishes match this view"</em>
                                        <span>"Try another search or show sold-out dishes again."</span>
                                        <button
                                            class="button primary small"
                                            on:click=move |_| {
                                                search.set(String::new());
                                                hide_sold_out.set(false);
                                            }
                                        >
                                            "Reset filters"
                                        </button>
                                    </div>
                                }
                                .into_any();
                            }
                            let manage = app.role().is_some_and(pos_types::Role::can_manage);
                            return view! {
                                <div class="empty">
                                    <em>"Nothing in this section yet"</em>
                                    <span>"Add dishes in Menu, or pick another section."</span>
                                    {manage
                                        .then(|| {
                                            view! {
                                                <button
                                                    class="button primary small"
                                                    on:click=move |_| app.page.set(Page::Menu)
                                                >
                                                    "Open menu"
                                                </button>
                                            }
                                        })}
                                </div>
                            }
                            .into_any();
                        }
                        view! {
                            <div class="product-grid">
                                {items
                                    .into_iter()
                                    .map(|item| {
                                        let low = item.low_stock;
                                        let sold = item.sold_out;
                                        let id = item.id.clone();
                                        let qty_id = id.clone();
                                        let qty = Signal::derive(move || {
                                            line_quantity(&app, LineKind::Item, &qty_id)
                                        });
                                        let add_item = item.clone();
                                        let add = Callback::new(move |_| {
                                            app.cart.update(|cart| cart.add_item(&add_item, 1));
                                        });
                                        let step_id = id.clone();
                                        let dec = Callback::new(move |_| {
                                            step_line(&app, LineKind::Item, &step_id, -1);
                                        });
                                        let inc_id = id.clone();
                                        let inc = Callback::new(move |_| {
                                            step_line(&app, LineKind::Item, &inc_id, 1);
                                        });
                                        let category = menu
                                            .categories
                                            .iter()
                                            .find(|category| category.id == item.category_id)
                                            .map(|category| category.name.as_str())
                                            .unwrap_or_default();
                                        let glyph_category = category.to_owned();
                                        view! {
                                            <article
                                                class=move || {
                                                    let mut classes = String::from("product-card");
                                                    if qty.get() > 0 {
                                                        classes.push_str(" selected");
                                                    }
                                                    if sold {
                                                        classes.push_str(" sold-out");
                                                    }
                                                    classes
                                                }
                                            >
                                                <div class="product-media">
                                                    <ProductArtwork
                                                        app
                                                        url=item.image_url.clone()
                                                        glyph=Callback::new(move |_| dish_icon(&glyph_category))
                                                    />
                                                    {sold
                                                        .then(|| view! { <span class="product-flag">"Sold out"</span> })}
                                                    {(low && !sold)
                                                        .then(|| {
                                                            view! {
                                                                <span class="product-flag warn">
                                                                    {format!("{} left", item.stock_balance)}
                                                                </span>
                                                            }
                                                        })}
                                                </div>
                                                <div class="product-info">
                                                    <h3>{item.name.clone()}</h3>
                                                    <p class="product-detail">
                                                        {format!("per {}", item.unit_type.label())}
                                                    </p>
                                                    <div class="product-bottom">
                                                        <span class="product-price">
                                                            {money(item.price_per_unit_kobo)}
                                                        </span>
                                                        <span class="unit-tag">{category.to_uppercase()}</span>
                                                    </div>
                                                </div>
                                                <div class="product-actions">
                                                    <button
                                                        class=move || {
                                                            if qty.get() > 0 { "add-dish is-hidden" } else { "add-dish" }
                                                        }
                                                        disabled=sold
                                                        on:click=move |_| add.run(())
                                                    >
                                                        "Add to dish"
                                                    </button>
                                                    <div
                                                        class=move || {
                                                            if qty.get() == 0 { "stepper is-hidden" } else { "stepper" }
                                                        }
                                                    >
                                                        <button
                                                            class="step"
                                                            aria-label="Less"
                                                            on:click=move |_| dec.run(())
                                                        >
                                                            "\u{2212}"
                                                        </button>
                                                        <span class="qty">{move || qty.get()}</span>
                                                        <button
                                                            class="step"
                                                            aria-label="More"
                                                            on:click=move |_| inc.run(())
                                                        >
                                                            "+"
                                                        </button>
                                                    </div>
                                                </div>
                                            </article>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        }
                        .into_any()
                    }}
                </div>

                <OpenTickets app table_edit />
            </section>

            <aside class="ticket">
                <div class="ticket-head">
                    <div class="ticket-title-row">
                        <div class="ticket-title">
                            <span class="eyebrow">
                                {move || {
                                    if app.cart.get().order_type == OrderType::DineIn {
                                        "Dine in"
                                    } else {
                                        "Takeaway"
                                    }
                                }}
                            </span>
                            <h2>
                                {move || {
                                    let cart = app.cart.get();
                                    if cart.order_type == OrderType::Takeaway {
                                        "Takeaway".to_owned()
                                    } else if cart.table_no.trim().is_empty() {
                                        "New table".to_owned()
                                    } else {
                                        format!("Table {}", cart.table_no.trim())
                                    }
                                }}
                            </h2>
                            <p class="ticket-sub">
                                {move || {
                                    let cart = app.cart.get();
                                    let count = cart.lines.iter().map(|line| line.quantity).sum::<i64>();
                                    let items = if count == 1 { "1 item".to_owned() } else { format!("{count} items") };
                                    let server = app
                                        .user()
                                        .map(|user| user.name)
                                        .unwrap_or_else(|| "Counter".to_owned());
                                    format!("Served by {server} · {items}")
                                }}
                            </p>
                        </div>
                        {move || {
                            (app.cart.get().order_type == OrderType::DineIn)
                                .then(|| {
                                    view! {
                                        <button
                                            class="icon-button quiet"
                                            aria-label="Edit table"
                                            title="Edit table"
                                            on:click=move |_| table_edit.update(|open| *open = !*open)
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                                                <path d="m4 20 4.5-1 10-10a2.1 2.1 0 0 0-3-3l-10 10L4 20Z" />
                                                <path d="m13.5 8.5 3 3" />
                                            </svg>
                                        </button>
                                    }
                                })
                        }}
                    </div>
                    {move || {
                        app.cart
                            .get()
                            .editing_no
                            .map(|order_no| {
                                let cancel = move |_| app.cart.update(|cart| cart.reset());
                                view! {
                                    <div class="banner banner-info">
                                        <span>"Editing " <strong>{order_no}</strong></span>
                                        <button class="link" on:click=cancel>"Start fresh"</button>
                                    </div>
                                }
                            })
                    }}
                    <div class="segmented">
                        <button
                            class=move || {
                                if app.cart.get().order_type == OrderType::DineIn { "segment active" } else { "segment" }
                            }
                            on:click=move |_| {
                                app.cart.update(|cart| cart.order_type = OrderType::DineIn);
                                table_edit.set(true);
                            }
                        >
                            "Dine in"
                        </button>
                        <button
                            class=move || {
                                if app.cart.get().order_type == OrderType::Takeaway { "segment active" } else { "segment" }
                            }
                            on:click=move |_| {
                                app.cart.update(|cart| cart.order_type = OrderType::Takeaway);
                                table_edit.set(false);
                            }
                        >
                            "Take away"
                        </button>
                    </div>

                    {move || {
                        (app.cart.get().order_type == OrderType::DineIn && table_edit.get())
                            .then(|| {
                                view! {
                                    <label class="field compact table-field">
                                        <span>"Table number"</span>
                                        <input
                                            type="text"
                                            inputmode="numeric"
                                            placeholder="e.g. 4"
                                            prop:value=move || app.cart.get().table_no
                                            on:input=move |event| {
                                                let value = event_target_value(&event);
                                                app.cart.update(|cart| cart.table_no = value);
                                                table_error.set(None);
                                            }
                                        />
                                    </label>
                                }
                            })
                    }}
                    {move || {
                        table_error
                            .get()
                            .map(|message| view! { <p class="error-text">{message}</p> })
                    }}
                </div>

                <div class="ticket-lines">
                    {move || {
                        let cart = app.cart.get();
                        if cart.lines.is_empty() {
                            return view! {
                                <div class="ticket-empty">
                                    <span class="ticket-empty-icon">
                                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
                                            <path d="M5 8h14l-1 11a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 8Z" />
                                            <path d="M9 8V6a3 3 0 0 1 6 0v2" />
                                        </svg>
                                    </span>
                                    <strong>"The ticket is empty"</strong>
                                    <p class="muted">"Tap a dish to start the order."</p>
                                </div>
                            }
                            .into_any();
                        }
                        cart.lines
                            .iter()
                            .enumerate()
                            .map(|(index, line)| {
                                let name = line.name.clone();
                                let initial = name
                                    .chars()
                                    .next()
                                    .unwrap_or('H')
                                    .to_uppercase()
                                    .to_string();
                                let details = line.details.clone();
                                let is_pack = line.kind == LineKind::Pack;
                                let quantity = line.quantity;
                                let line_total = line.unit_price_kobo * line.quantity;
                                view! {
                                    <div class="ticket-line">
                                        <span class="line-thumb">{initial}</span>
                                        <div class="line-main">
                                            <div class="line-top">
                                                <strong>{name}</strong>
                                                <span class="line-total">{money(line_total)}</span>
                                            </div>
                                            {is_pack
                                                .then(|| view! {
                                                    <small class="line-detail">{details.join(" · ")}</small>
                                                })}
                                            <small class="muted">
                                                {format!("{} × {}", quantity, money(line.unit_price_kobo))}
                                            </small>
                                            <div class="line-controls">
                                                <button
                                                    class="step"
                                                    on:click=move |_| app.cart.update(|cart| cart.adjust(index, -1))
                                                    aria-label="Less"
                                                >
                                                    "\u{2212}"
                                                </button>
                                                <span class="qty">{quantity}</span>
                                                <button
                                                    class="step"
                                                    on:click=move |_| app.cart.update(|cart| cart.adjust(index, 1))
                                                    aria-label="More"
                                                >
                                                    "+"
                                                </button>
                                                <button
                                                    class="step remove"
                                                    on:click=move |_| app.cart.update(|cart| cart.remove(index))
                                                    aria-label="Remove"
                                                >
                                                    "\u{00d7}"
                                                </button>
                                            </div>
                                        </div>
                                    </div>
                                }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </div>

                <div class="ticket-foot">
                    <div class="summary-card">
                        <div class="summary-head">
                            <span>"Order summary"</span>
                            <span>{move || {
                                let count = app.cart.get().lines.iter().map(|line| line.quantity).sum::<i64>();
                                if count == 1 { "1 item".to_owned() } else { format!("{count} items") }
                            }}</span>
                        </div>
                        <div class="total-row">
                            <span>"Subtotal"</span>
                            <span>{move || money(subtotal.get())}</span>
                        </div>
                        {move || {
                            (fee.get() > 0)
                                .then(|| {
                                    view! {
                                        <div class="total-row muted-row">
                                            <span>"Packaging"</span>
                                            <span>{money(fee.get())}</span>
                                        </div>
                                    }
                                })
                        }}
                        <div class="total-row grand">
                            <span>"Total"</span>
                            <span>{move || money(total.get())}</span>
                        </div>
                    </div>

                    <div class="payment-picker">
                        <span class="eyebrow">"How is it being paid?"</span>
                        <div class="payment-tiles">
                            {methods()
                                .into_iter()
                                .map(|(method, label)| {
                                    view! {
                                        <button
                                            class=move || {
                                                if preferred.get() == method { "payment-tile active" } else { "payment-tile" }
                                            }
                                            aria-pressed=move || (preferred.get() == method).to_string()
                                            on:click=move |_| preferred.set(method)
                                        >
                                            {method_icon(method)}
                                            <span>{label}</span>
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </div>

                    <div class="ticket-actions">
                        <div class="ticket-secondary">
                            <button
                                class="text-button"
                                disabled=move || app.cart.get().lines.is_empty()
                                on:click=move |_| app.cart.update(|cart| cart.reset())
                            >
                                "Clear"
                            </button>
                            <button
                                class="text-button"
                                on:click=hold
                                disabled=move || processing.get() || app.cart.get().lines.is_empty()
                            >
                                "Hold ticket"
                            </button>
                        </div>
                        <button
                            class="button primary pay-button"
                            on:click=open_payment
                            disabled=move || processing.get() || app.cart.get().lines.is_empty()
                        >
                            <span>"Take payment"</span>
                            <strong>{move || money(total.get())}</strong>
                        </button>
                    </div>
                </div>
            </aside>
        </div>

        {move || {
            payment_open
                .get()
                .then(|| {
                    view! {
                        <PaymentModal
                            app
                            method=preferred
                            processing
                            paid
                            on_done=Callback::new(move |_| payment_open.set(false))
                        />
                    }
                })
        }}

        {move || {
            paid
                .get()
                .map(|order_no| {
                    view! {
                        <Modal title="Payment taken" on_close=Callback::new(move |_| paid.set(None))>
                            <div class="paid-card">
                                <span class="stamp" aria-hidden="true">"Paid"</span>
                                <p class="eyebrow">"Sales receipt queued"</p>
                                <h3>{order_no}</h3>
                                <p class="muted">"Printing is handled on this PC once the printer agent is running."</p>
                                <button
                                    class="button primary wide"
                                    on:click=move |_| paid.set(None)
                                >
                                    "Next sale"
                                </button>
                            </div>
                        </Modal>
                    }
                })
        }}
    }
}

fn line_quantity(app: &App, kind: LineKind, id: &str) -> i64 {
    app.cart
        .get()
        .lines
        .iter()
        .find(|line| line.kind == kind && line.reference_id == id)
        .map_or(0, |line| line.quantity)
}

fn step_line(app: &App, kind: LineKind, id: &str, delta: i64) {
    app.cart.update(|cart| {
        let Some(index) = cart
            .lines
            .iter()
            .position(|line| line.kind == kind && line.reference_id == id)
        else {
            return;
        };
        if delta < 0 && cart.lines[index].quantity <= 1 {
            cart.remove(index);
        } else {
            cart.adjust(index, delta);
        }
    });
}

fn count_label(count: usize) -> String {
    if count == 1 {
        "1 item".to_owned()
    } else {
        format!("{count} items")
    }
}

fn dish_icon(label: &str) -> AnyView {
    let key = label.to_lowercase();
    if key.contains("drink") || key.contains("water") || key.contains("juice") {
        view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M7 4h9l-1.4 16H8.4L7 4Z" />
                <path d="M6.5 9h10" />
                <path d="m14 4 3-2" />
            </svg>
        }
        .into_any()
    } else if key.contains("pastr") || key.contains("bread") || key.contains("cake") {
        view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M4 13c0-3.3 3.6-6 8-6s8 2.7 8 6v5H4v-5Z" />
                <path d="M4 15h16" />
                <path d="M9 7.4V5m6 2.4V5" />
            </svg>
        }
        .into_any()
    } else if key.contains("side") || key.contains("chicken") || key.contains("meat") {
        view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M15.5 4a4.5 4.5 0 0 1 2 8.5l-5 5-4-4 5-5A4.5 4.5 0 0 1 15.5 4Z" />
                <path d="m8.5 13.5-4 4M6.5 15.5l2 2" />
            </svg>
        }
        .into_any()
    } else if key.contains("all") {
        view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <rect x="4" y="4" width="6" height="6" rx="1.5" />
                <rect x="14" y="4" width="6" height="6" rx="1.5" />
                <rect x="4" y="14" width="6" height="6" rx="1.5" />
                <rect x="14" y="14" width="6" height="6" rx="1.5" />
            </svg>
        }
        .into_any()
    } else {
        view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M4 12h16a8 8 0 0 1-16 0Z" />
                <path d="M8 8c0-1 1-2 2-2m2 0c0-1 1-2 2-2m2 2c0-1 1-2 2-2" />
            </svg>
        }
        .into_any()
    }
}

fn pack_icon() -> AnyView {
    view! {
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="m4 8 8-4 8 4-8 4-8-4Z" />
            <path d="M4 8v8l8 4 8-4V8M12 12v8" />
        </svg>
    }
    .into_any()
}

fn method_icon(method: PaymentMethod) -> AnyView {
    match method {
        PaymentMethod::Cash => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                <rect x="3" y="7" width="18" height="10" rx="2" />
                <circle cx="12" cy="12" r="2.3" />
                <path d="M7 10v4M17 10v4" />
            </svg>
        }
        .into_any(),
        PaymentMethod::Transfer => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                <path d="M4 8h15l-3-3M20 16H5l3 3" />
            </svg>
        }
        .into_any(),
        PaymentMethod::Card => view! {
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                <rect x="3" y="6" width="18" height="12" rx="2" />
                <path d="M3 10h18M7 15h4" />
            </svg>
        }
        .into_any(),
    }
}

#[component]
fn CategoryTabs(app: App) -> impl IntoView {
    view! {
        <div class="category-strip">
            {move || {
                let Some(menu) = app.menu.get() else {
                    return view! { <span></span> }.into_any();
                };
                let selected = app.selected_category.get();
                let active_items = menu.items.iter().filter(|item| item.active).collect::<Vec<_>>();
                let all_count = active_items.len();
                let mut cards: Vec<AnyView> = vec![
                    view! {
                        <button
                            class=if selected.is_none() { "category-card active" } else { "category-card" }
                            on:click=move |_| app.selected_category.set(None)
                        >
                            <span class="category-icon">{dish_icon("all")}</span>
                            <span class="category-copy">
                                <strong>"All"</strong>
                                <small>{count_label(all_count)}</small>
                            </span>
                        </button>
                    }
                    .into_any(),
                ];
                for category in menu.categories.iter().filter(|category| category.active) {
                    let id = category.id.clone();
                    let name = category.name.clone();
                    let active = selected.as_deref() == Some(id.as_str());
                    let count = active_items
                        .iter()
                        .filter(|item| item.category_id == id)
                        .count();
                    let icon_name = name.clone();
                    cards.push(
                        view! {
                            <button
                                class=if active { "category-card active" } else { "category-card" }
                                on:click=move |_| app.selected_category.set(Some(id.clone()))
                            >
                                <span class="category-icon">{dish_icon(&icon_name)}</span>
                                <span class="category-copy">
                                    <strong>{name}</strong>
                                    <small>{count_label(count)}</small>
                                </span>
                            </button>
                        }
                        .into_any(),
                    );
                }
                if !menu.packs.is_empty() {
                    let active = selected.as_deref() == Some(PACKS_CATEGORY);
                    let count = menu.packs.iter().filter(|pack| pack.active).count();
                    cards.push(
                        view! {
                            <button
                                class=if active { "category-card active" } else { "category-card" }
                                on:click=move |_| app.selected_category.set(Some(PACKS_CATEGORY.to_owned()))
                            >
                                <span class="category-icon">{pack_icon()}</span>
                                <span class="category-copy">
                                    <strong>"Packs"</strong>
                                    <small>{count_label(count)}</small>
                                </span>
                            </button>
                        }
                        .into_any(),
                    );
                }
                view! { <div class="category-strip-inner">{cards}</div> }.into_any()
            }}
        </div>
    }
}

#[component]
fn OpenTickets(app: App, table_edit: RwSignal<bool>) -> impl IntoView {
    view! {
        {move || {
            let orders = app.held.get();
            if orders.is_empty() {
                return None;
            }
            Some(
                view! {
                    <div class="open-tickets">
                        <div class="open-tickets-head">
                            <span class="eyebrow">"Open tickets"</span>
                            <span class="muted">{format!("{} parked", orders.len())}</span>
                        </div>
                        <div class="table-strip">
                            {orders
                                .into_iter()
                                .map(|order| {
                                    let table = order.table_no.clone();
                                    let label = table
                                        .as_ref()
                                        .map(|table| format!("Table {table}"))
                                        .unwrap_or_else(|| "Takeaway".to_owned());
                                    let badge = table
                                        .as_ref()
                                        .map(|table| format!("T{table}"))
                                        .unwrap_or_else(|| "TA".to_owned());
                                    let count = order.lines.iter().map(|line| line.quantity).sum::<i64>();
                                    let total = order.total_kobo;
                                    let order_no = order.order_no.clone();
                                    let resume_order = order.clone();
                                    view! {
                                        <button
                                            class="table-pill"
                                            title=order_no
                                            on:click=move |_| {
                                                let order = resume_order.clone();
                                                spawn_local(async move {
                                                    match app.api().order(&order.id).await {
                                                        Ok(fresh) => {
                                                            let needs_table = fresh.order_type == OrderType::DineIn
                                                                && fresh
                                                                    .table_no
                                                                    .as_deref()
                                                                    .is_none_or(|table| table.trim().is_empty());
                                                            app.cart.update(|cart| cart.resume(&fresh));
                                                            table_edit.set(needs_table);
                                                        }
                                                        Err(error) => app.fail(&error),
                                                    }
                                                });
                                            }
                                        >
                                            <span class="table-badge">{badge}</span>
                                            <span class="table-copy">
                                                <strong>{label}</strong>
                                                <small>{format!("{count} items · {}", money(total))}</small>
                                            </span>
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </div>
                }
                .into_any(),
            )
        }}
    }
}

#[component]
fn PaymentModal(
    app: App,
    method: RwSignal<PaymentMethod>,
    processing: RwSignal<bool>,
    paid: RwSignal<Option<String>>,
    on_done: Callback<()>,
) -> impl IntoView {
    let total = Signal::derive(move || app.cart.get().total_kobo(app.settings.get().as_ref()));

    let submit = move |_| {
        if processing.get_untracked() {
            return;
        }
        let amount = total.get_untracked();
        if amount <= 0 {
            app.toast(ToastKind::Error, "Add something to the ticket first.");
            return;
        }
        let payments = vec![PaymentInput {
            method: method.get_untracked(),
            amount_kobo: amount,
            reference: None,
        }];
        processing.set(true);
        spawn_local(async move {
            let cart = app.cart.get_untracked();
            match persist(app, &cart).await {
                Ok(order) => {
                    let order_id = order.id.clone();
                    app.cart.update(|cart| {
                        cart.editing_id = Some(order_id.clone());
                        cart.editing_no = Some(order.order_no.clone());
                    });
                    match app
                        .api()
                        .pay_order(&order_id, &PayOrderRequest { payments })
                        .await
                    {
                        Ok(_) => {
                            let _ = app
                                .api()
                                .print_order(
                                    &order.id,
                                    &CreatePrintJobRequest {
                                        job_type: PrintJobType::Receipt,
                                        paper_width: None,
                                    },
                                )
                                .await;
                            app.cart.update(|cart| cart.reset());
                            paid.set(Some(order.order_no.clone()));
                            on_done.run(());
                            refresh_held(app).await;
                            refresh_menu(app).await;
                            refresh_balances(app).await;
                        }
                        Err(error) => app.fail(&error),
                    }
                }
                Err(error) => app.fail(&error),
            }
            processing.set(false);
        });
    };

    view! {
        <Modal title="Confirm payment" wide=true on_close=on_done>
            <div class="payment-total">
                <span class="muted">"Total due"</span>
                <strong>{move || money(total.get())}</strong>
            </div>

            <div class="payment-confirm">
                <p class="eyebrow">"Has the money been paid?"</p>
                <p class="muted">
                    "Confirm the payment using the selected method. The exact total will be recorded."
                </p>
            </div>

            <div class="payment-tiles">
                {methods()
                    .into_iter()
                    .map(|(value, label)| {
                        view! {
                            <button
                                class=move || {
                                    if method.get() == value { "payment-tile active" } else { "payment-tile" }
                                }
                                aria-pressed=move || (method.get() == value).to_string()
                                on:click=move |_| method.set(value)
                            >
                                {method_icon(value)}
                                <span>{label}</span>
                            </button>
                        }
                    })
                    .collect_view()}
            </div>

            <div class="payment-choice-actions">
                <button
                    class="button ghost wide"
                    on:click=move |_| on_done.run(())
                    disabled=move || processing.get()
                >
                    "No, go back"
                </button>
                <button
                    class="button primary wide"
                    on:click=submit
                    disabled=move || processing.get()
                >
                    {move || {
                        if processing.get() {
                            view! { <Spinner /> }.into_any()
                        } else {
                            view! { {format!("Yes, paid · {}", money(total.get()))} }.into_any()
                        }
                    }}
                </button>
            </div>
        </Modal>
    }
}

fn table_ok(app: App) -> bool {
    let cart = app.cart.get_untracked();
    if cart.order_type == OrderType::DineIn && cart.table_no.trim().is_empty() {
        app.toast(
            ToastKind::Error,
            "Enter the table number for a dine-in order.",
        );
        return false;
    }
    true
}

async fn persist(app: App, cart: &Cart) -> Result<OrderDto, ApiError> {
    let table_no = match cart.order_type {
        OrderType::DineIn => Some(cart.table_no.trim().to_owned()),
        OrderType::Takeaway => None,
    };
    let lines = cart.to_inputs();
    match &cart.editing_id {
        Some(id) => {
            app.api()
                .update_order(
                    id,
                    &UpdateOrderRequest {
                        order_type: cart.order_type,
                        table_no,
                        lines,
                    },
                )
                .await
        }
        None => {
            app.api()
                .create_order(&CreateOrderRequest {
                    order_type: cart.order_type,
                    table_no,
                    lines,
                })
                .await
        }
    }
}

#[component]
pub fn Held(app: App) -> impl IntoView {
    let voiding = RwSignal::new(None::<OrderDto>);
    let busy = RwSignal::new(false);

    Effect::new(move |_| {
        let app = app;
        spawn_local(async move {
            refresh_held(app).await;
        });
    });

    let resume = move |order: OrderDto| {
        busy.set(true);
        spawn_local(async move {
            match app.api().order(&order.id).await {
                Ok(fresh) => {
                    app.cart.update(|cart| cart.resume(&fresh));
                    app.page.set(Page::Sale);
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    let confirm_void = move |_| {
        let Some(order) = voiding.get_untracked() else {
            return;
        };
        busy.set(true);
        spawn_local(async move {
            match app
                .api()
                .void_order(
                    &order.id,
                    &VoidOrderRequest {
                        manager_pin: None,
                        reason: "discarded".to_owned(),
                        return_to_stock: true,
                    },
                )
                .await
            {
                Ok(_) => {
                    app.notify(format!("{} discarded.", order.order_no));
                    voiding.set(None);
                    refresh_held(app).await;
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="held">
            {move || {
                let orders = app.held.get();
                if orders.is_empty() {
                    return view! { <Empty text="No tickets are parked." /> }.into_any();
                }
                view! {
                    <div class="held-grid">
                        {orders
                            .into_iter()
                            .map(|order| {
                                let summary = format!(
                                    "{} · {} · {}",
                                    order.order_type_label(),
                                    order.lines.iter().map(|line| line.quantity).sum::<i64>(),
                                    short_time(&order.created_at),
                                );
                                let total = order.total_kobo;
                                let order_no = order.order_no.clone();
                                let table = order.table_no.clone();
                                let resume_order = order.clone();
                                let void_order = order.clone();
                                view! {
                                    <article class="held-card">
                                        <header>
                                            <strong>{order_no}</strong>
                                            <span class="badge badge-neutral">{order.status.as_str()}</span>
                                        </header>
                                        <p class="muted">{summary}</p>
                                        {table.map(|table| view! { <p>"Table "<strong>{table}</strong></p> })}
                                        <p class="held-total">{money(total)}</p>
                                        <div class="row gap">
                                            <button
                                                class="button primary small"
                                                on:click=move |_| resume(resume_order.clone())
                                                disabled=move || busy.get()
                                            >
                                                "Resume"
                                            </button>
                                            <button
                                                class="button ghost small"
                                                on:click=move |_| voiding.set(Some(void_order.clone()))
                                            >
                                                "Discard"
                                            </button>
                                        </div>
                                    </article>
                                }
                            })
                            .collect_view()}
                    </div>
                }
                .into_any()
            }}

            {move || {
                voiding
                    .get()
                    .map(|order| {
                        let order_no = order.order_no.clone();
                        view! {
                            <Modal title="Discard this ticket?" on_close=Callback::new(move |_| voiding.set(None))>
                                <p>
                                    {format!("{} will be voided. Nothing has been paid yet, and no stock has left the shelf.", order_no)}
                                </p>
                                <div class="row gap">
                                    <button class="button ghost" on:click=move |_| voiding.set(None)>"Keep it"</button>
                                    <button class="button danger" on:click=confirm_void>"Discard"</button>
                                </div>
                            </Modal>
                        }
                    })
            }}
        </div>
    }
}

trait OrderLabels {
    fn order_type_label(&self) -> &'static str;
}

impl OrderLabels for OrderDto {
    fn order_type_label(&self) -> &'static str {
        match self.order_type {
            OrderType::DineIn => "Dine in",
            OrderType::Takeaway => "Takeaway",
        }
    }
}
