//! Categories, items and packs, including price changes and activation.

use leptos::prelude::*;
use pos_types::{
    CategoryDto, CreateCategoryRequest, CreateItemRequest, CreatePackRequest, MenuItemDto,
    MenuResponse, PackComponentInput, PackDto, PackPricingMode, UnitType, UpdateCategoryRequest,
    UpdateItemRequest, UpdatePackRequest,
};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};

use crate::api::{Api, ApiError};
use crate::components::{Badge, Empty, Modal, Spinner};
use crate::format::{money, parse_naira};
use crate::state::{App, ToastKind};

const MAX_PHOTO_BYTES: f64 = 12.0 * 1024.0 * 1024.0;

#[derive(Clone)]
struct SelectedPhoto {
    bytes: Vec<u8>,
    mime: String,
    preview_url: String,
}

fn revoke_preview(photo: &SelectedPhoto) {
    let _ = web_sys::Url::revoke_object_url(&photo.preview_url);
}

fn photo_mime(file: &web_sys::File) -> Option<&'static str> {
    match file.type_().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/png" => Some("image/png"),
        "image/webp" => Some("image/webp"),
        _ => {
            let name = file.name().to_ascii_lowercase();
            if name.ends_with(".jpg") || name.ends_with(".jpeg") {
                Some("image/jpeg")
            } else if name.ends_with(".png") {
                Some("image/png")
            } else if name.ends_with(".webp") {
                Some("image/webp")
            } else {
                None
            }
        }
    }
}

async fn apply_item_photo(
    api: &Api,
    menu: MenuResponse,
    id: Option<String>,
    name: &str,
    photo: Option<SelectedPhoto>,
    remove: bool,
) -> Result<MenuResponse, ApiError> {
    let target = id.or_else(|| {
        menu.items
            .iter()
            .find(|item| item.name.eq_ignore_ascii_case(name))
            .map(|item| item.id.clone())
    });
    match (photo, target) {
        (Some(photo), Some(id)) => api.upload_item_image(&id, &photo.mime, &photo.bytes).await,
        (None, Some(id)) if remove => api.delete_item_image(&id).await,
        _ => Ok(menu),
    }
}

async fn apply_pack_photo(
    api: &Api,
    menu: MenuResponse,
    id: Option<String>,
    name: &str,
    photo: Option<SelectedPhoto>,
    remove: bool,
) -> Result<MenuResponse, ApiError> {
    let target = id.or_else(|| {
        menu.packs
            .iter()
            .find(|pack| pack.name.eq_ignore_ascii_case(name))
            .map(|pack| pack.id.clone())
    });
    match (photo, target) {
        (Some(photo), Some(id)) => api.upload_pack_image(&id, &photo.mime, &photo.bytes).await,
        (None, Some(id)) if remove => api.delete_pack_image(&id).await,
        _ => Ok(menu),
    }
}

#[component]
fn PhotoPicker(
    app: App,
    title: String,
    existing_url: Option<String>,
    selected: RwSignal<Option<SelectedPhoto>>,
    remove: RwSignal<bool>,
) -> impl IntoView {
    let existing_present = existing_url.is_some();
    let absolute_existing = existing_url.as_ref().map(|url| app.api().asset_url(url));
    let preview = Memo::new(move |_| {
        if let Some(photo) = selected.get() {
            Some(photo.preview_url)
        } else if remove.get() {
            None
        } else {
            absolute_existing.clone()
        }
    });

    on_cleanup(move || {
        if let Some(photo) = selected.get_untracked() {
            revoke_preview(&photo);
        }
    });

    let choose = move |event: web_sys::Event| {
        let Some(input) = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::HtmlInputElement>().ok())
        else {
            return;
        };
        let Some(files) = input.files() else {
            return;
        };
        let Some(file) = files.get(0) else {
            return;
        };
        input.set_value("");
        if file.size() > MAX_PHOTO_BYTES {
            app.toast(ToastKind::Error, "Choose a photo smaller than 12 MB.");
            return;
        }
        let Some(mime) = photo_mime(&file) else {
            app.toast(ToastKind::Error, "Choose a JPEG, PNG or WebP photo.");
            return;
        };
        spawn_local(async move {
            let buffer = match JsFuture::from(file.array_buffer()).await {
                Ok(buffer) => buffer,
                Err(_) => {
                    app.toast(ToastKind::Error, "The browser could not read that photo.");
                    return;
                }
            };
            let array = js_sys::Uint8Array::new(&buffer);
            let mut bytes = vec![0; array.length() as usize];
            array.copy_to(&mut bytes);
            let preview_url = match web_sys::Url::create_object_url_with_blob(&file) {
                Ok(url) => url,
                Err(_) => {
                    app.toast(
                        ToastKind::Error,
                        "The browser could not preview that photo.",
                    );
                    return;
                }
            };
            let photo = SelectedPhoto {
                bytes,
                mime: mime.to_owned(),
                preview_url,
            };
            selected.update(|slot| {
                if let Some(old) = slot.replace(photo) {
                    revoke_preview(&old);
                }
            });
            remove.set(false);
        });
    };

    view! {
        <div class="photo-field">
            <div class="photo-label">{title}</div>
            <div class="photo-editor">
                <div class="photo-preview">
                    {move || match preview.get() {
                        Some(url) => view! { <img src=url alt="" /> }.into_any(),
                        None => {
                            view! { <span class="photo-placeholder">"No photo yet"</span> }
                                .into_any()
                        }
                    }}
                </div>
                <div class="photo-actions">
                    <label class="button ghost small photo-choose">
                        "Choose photo"
                        <input
                            type="file"
                            class="photo-input"
                            accept="image/jpeg,image/png,image/webp"
                            on:change=choose
                        />
                    </label>
                    {move || {
                        preview
                            .get()
                            .is_some()
                            .then(|| {
                                view! {
                                    <button
                                        type="button"
                                        class="button ghost small"
                                        on:click=move |_| {
                                            if let Some(photo) = selected.get_untracked() {
                                                revoke_preview(&photo);
                                            }
                                            selected.set(None);
                                            remove.set(existing_present);
                                        }
                                    >
                                        "Remove"
                                    </button>
                                }
                            })
                    }}
                </div>
                <p class="muted small">
                    "JPEG, PNG or WebP. Large photos are resized automatically."
                </p>
            </div>
        </div>
    }
}

#[component]
fn MenuPhoto(app: App, url: Option<String>, class: String) -> impl IntoView {
    let absolute = url.map(|url| app.api().asset_url(&url));
    let show = RwSignal::new(absolute.is_some());
    view! {
        <div class=class>
            {move || {
                if show.get() {
                    absolute
                        .clone()
                        .map(|source| {
                            view! {
                                <img src=source alt="" on:error=move |_| show.set(false) />
                            }
                                .into_any()
                        })
                        .unwrap_or_else(|| {
                            view! { <span class="photo-fallback">"No photo"</span> }.into_any()
                        })
                } else {
                    view! { <span class="photo-fallback">"No photo"</span> }.into_any()
                }
            }}
        </div>
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Categories,
    Items,
    Packs,
}

#[derive(Clone)]
enum Form {
    Category(Option<CategoryDto>),
    Item(Option<MenuItemDto>),
    Pack(Option<PackDto>),
}

#[component]
pub fn MenuAdmin(app: App) -> impl IntoView {
    let tab = RwSignal::new(Tab::Items);
    let form = RwSignal::new(None::<Form>);

    let open_category = move |category: Option<CategoryDto>| {
        form.set(Some(Form::Category(category)));
    };

    view! {
        <div class="admin">
            <div class="segmented wide-segments">
                <button
                    class=move || if tab.get() == Tab::Categories { "segment active" } else { "segment" }
                    on:click=move |_| tab.set(Tab::Categories)
                >
                    "Categories"
                </button>
                <button
                    class=move || if tab.get() == Tab::Items { "segment active" } else { "segment" }
                    on:click=move |_| tab.set(Tab::Items)
                >
                    "Items"
                </button>
                <button
                    class=move || if tab.get() == Tab::Packs { "segment active" } else { "segment" }
                    on:click=move |_| tab.set(Tab::Packs)
                >
                    "Packs"
                </button>
            </div>

            {move || match tab.get() {
                Tab::Categories => {
                    view! {
                        <div class="panel">
                            <div class="panel-head">
                                <h2>"Categories"</h2>
                                <button class="button primary small" on:click=move |_| open_category(None)>
                                    "Add category"
                                </button>
                            </div>
                            <table class="table">
                                <thead>
                                    <tr>
                                        <th>"Name"</th>
                                        <th>"Order"</th>
                                        <th>"Status"</th>
                                        <th></th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {move || {
                                        app.menu
                                            .get()
                                            .map(|menu| {
                                                menu.categories
                                                    .iter()
                                                    .map(|category| {
                                                        let row = category.clone();
                                                        view! {
                                                            <tr>
                                                                <td>{category.name.clone()}</td>
                                                                <td>{category.sort_order}</td>
                                                                <td>
                                                                    {if category.active {
                                                                        view! { <Badge text="Shown" tone="good" /> }.into_any()
                                                                    } else {
                                                                        view! { <Badge text="Hidden" tone="neutral" /> }.into_any()
                                                                    }}
                                                                </td>
                                                                <td>
                                                                    <button
                                                                        class="link"
                                                                        on:click=move |_| open_category(Some(row.clone()))
                                                                    >
                                                                        "Edit"
                                                                    </button>
                                                                </td>
                                                            </tr>
                                                        }
                                                    })
                                                    .collect_view()
                                            })
                                    }}
                                </tbody>
                            </table>
                        </div>
                    }
                    .into_any()
                }
                Tab::Items => {
                    view! {
                        <div class="panel">
                            <div class="panel-head">
                                <h2>"Items"</h2>
                                <button class="button primary small" on:click=move |_| form.set(Some(Form::Item(None)))>
                                    "Add item"
                                </button>
                            </div>
                            <table class="table">
                                <thead>
                                    <tr>
                                        <th>"Item"</th>
                                        <th>"Photo"</th>
                                        <th>"Category"</th>
                                        <th>"Unit"</th>
                                        <th>"Price"</th>
                                        <th>"Low at"</th>
                                        <th>"Status"</th>
                                        <th></th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {move || {
                                        app.menu
                                            .get()
                                            .map(|menu| {
                                                menu.items
                                                    .iter()
                                                    .map(|item| {
                                                        let row = item.clone();
                                                        view! {
                                                            <tr>
                                                                <td>{item.name.clone()}</td>
                                                                <td>
                                                                    <MenuPhoto
                                                                        app
                                                                        url=item.image_url.clone()
                                                                        class="table-photo".to_owned()
                                                                    />
                                                                </td>
                                                                <td>{item.category_name.clone()}</td>
                                                                <td>{item.unit_type.label()}</td>
                                                                <td>{money(item.price_per_unit_kobo)}</td>
                                                                <td>{item.low_stock_level}</td>
                                                                <td>
                                                                    {if item.active {
                                                                        view! { <Badge text="On menu" tone="good" /> }.into_any()
                                                                    } else {
                                                                        view! { <Badge text="Hidden" tone="neutral" /> }.into_any()
                                                                    }}
                                                                </td>
                                                                <td>
                                                                    <button
                                                                        class="link"
                                                                        on:click=move |_| form.set(Some(Form::Item(Some(row.clone()))))
                                                                    >
                                                                        "Edit"
                                                                    </button>
                                                                </td>
                                                            </tr>
                                                        }
                                                    })
                                                    .collect_view()
                                            })
                                    }}
                                </tbody>
                            </table>
                        </div>
                    }
                    .into_any()
                }
                Tab::Packs => {
                    view! {
                        <div class="panel">
                            <div class="panel-head">
                                <h2>"Packs"</h2>
                                <button class="button primary small" on:click=move |_| form.set(Some(Form::Pack(None)))>
                                    "Add pack"
                                </button>
                            </div>
                            {move || {
                                app.menu
                                    .get()
                                    .map(|menu| {
                                        if menu.packs.is_empty() {
                                            return view! { <Empty text="No packs yet." /> }.into_any();
                                        }
                                        view! {
                                            <div class="pack-grid">
                                                {menu.packs
                                                    .iter()
                                                    .map(|pack| {
                                                        let row = pack.clone();
                                                        let pack_id = pack.id.clone();
                                                        view! {
                                                            <article class="pack-card">
                                                                <MenuPhoto
                                                                    app
                                                                    url=pack.image_url.clone()
                                                                    class="pack-photo".to_owned()
                                                                />
                                                                <header>
                                                                    <strong>{pack.name.clone()}</strong>
                                                                    {if pack.active {
                                                                        view! { <Badge text="On menu" tone="good" /> }.into_any()
                                                                    } else {
                                                                        view! { <Badge text="Hidden" tone="neutral" /> }.into_any()
                                                                    }}
                                                                </header>
                                                                <p class="muted small">{pack.pricing_mode.as_str()}</p>
                                                                <p class="pack-price">{money(pack.price_kobo)}</p>
                                                                <ul class="pack-parts">
                                                                    {pack.components
                                                                        .iter()
                                                                        .map(|component| {
                                                                            view! {
                                                                                <li>
                                                                                    {format!(
                                                                                        "{} × {} {}",
                                                                                        component.quantity,
                                                                                        component.item_name,
                                                                                        component.unit_type.label(),
                                                                                    )}
                                                                                </li>
                                                                            }
                                                                        })
                                                                        .collect_view()}
                                                                </ul>
                                                                <div class="row gap">
                                                                    <button
                                                                        class="button ghost small"
                                                                        on:click=move |_| form.set(Some(Form::Pack(Some(row.clone()))))
                                                                    >
                                                                        "Edit"
                                                                    </button>
                                                                    <button
                                                                        class="button ghost small"
                                                                        on:click=move |_| {
                                                                            let app = app;
                                                                            let pack_id = pack_id.clone();
                                                                            spawn_local(async move {
                                                                                match app.api().duplicate_pack(&pack_id).await {
                                                                                    Ok(menu) => {
                                                                                        app.menu.set(Some(menu));
                                                                                        app.notify("Pack duplicated.");
                                                                                    }
                                                                                    Err(error) => app.fail(&error),
                                                                                }
                                                                            });
                                                                        }
                                                                    >
                                                                        "Duplicate"
                                                                    </button>
                                                                </div>
                                                            </article>
                                                        }
                                                    })
                                                    .collect_view()}
                                            </div>
                                        }
                                        .into_any()
                                    })
                            }}
                        </div>
                    }
                    .into_any()
                }
            }}

            {move || {
                form.get()
                    .map(|open| match open {
                        Form::Category(category) => view! {
                            <CategoryForm
                                app
                                editing=category
                                on_close=Callback::new(move |_| form.set(None))
                            />
                        }
                        .into_any(),
                        Form::Item(item) => view! {
                            <ItemForm app editing=item on_close=Callback::new(move |_| form.set(None)) />
                        }
                        .into_any(),
                        Form::Pack(pack) => view! {
                            <PackForm app editing=pack on_close=Callback::new(move |_| form.set(None)) />
                        }
                        .into_any(),
                    })
            }}
        </div>
    }
}

#[component]
fn CategoryForm(app: App, editing: Option<CategoryDto>, on_close: Callback<()>) -> impl IntoView {
    let existing = editing.clone();
    let name = RwSignal::new(editing.as_ref().map(|c| c.name.clone()).unwrap_or_default());
    let sort_order = RwSignal::new(
        editing
            .as_ref()
            .map(|c| c.sort_order.to_string())
            .unwrap_or_else(|| "0".to_owned()),
    );
    let active = RwSignal::new(editing.as_ref().is_none_or(|c| c.active));
    let busy = RwSignal::new(false);

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let name_text = name.get_untracked().trim().to_owned();
        if name_text.is_empty() {
            app.toast(ToastKind::Error, "Give the category a name.");
            return;
        }
        let sort = sort_order
            .get_untracked()
            .trim()
            .parse::<i64>()
            .unwrap_or(0);
        busy.set(true);
        let api = app.api();
        let existing = existing.clone();
        spawn_local(async move {
            let result = match &existing {
                Some(category) => {
                    api.update_category(
                        &category.id,
                        &UpdateCategoryRequest {
                            name: name_text,
                            sort_order: sort,
                            active: active.get_untracked(),
                        },
                    )
                    .await
                }
                None => {
                    api.create_category(&CreateCategoryRequest {
                        name: name_text,
                        sort_order: sort,
                    })
                    .await
                }
            };
            match result {
                Ok(menu) => {
                    app.menu.set(Some(menu));
                    app.notify("Category saved.");
                    on_close.run(());
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    view! {
        <Modal title=if editing.is_some() { "Edit category" } else { "New category" } on_close=on_close>
            <label class="field">
                <span>"Name"</span>
                <input
                    type="text"
                    prop:value=move || name.get()
                    on:input=move |event| name.set(event_target_value(&event))
                />
            </label>
            <label class="field">
                <span>"Sort order"</span>
                <input
                    type="number"
                    prop:value=move || sort_order.get()
                    on:input=move |event| sort_order.set(event_target_value(&event))
                />
            </label>
            {editing.is_some().then(|| view! {
                <label class="check">
                    <input
                        type="checkbox"
                        prop:checked=move || active.get()
                        on:change=move |event| active.set(event_target_checked(&event))
                    />
                    <span>"Shown on the sale screen"</span>
                </label>
            })}
            <button class="button primary wide" on:click=submit disabled=move || busy.get()>
                {move || if busy.get() { view! { <Spinner /> }.into_any() } else { view! { "Save" }.into_any() }}
            </button>
        </Modal>
    }
}

#[component]
fn ItemForm(app: App, editing: Option<MenuItemDto>, on_close: Callback<()>) -> impl IntoView {
    let existing = editing.clone();
    let categories = app
        .menu
        .get_untracked()
        .map(|menu| menu.categories)
        .unwrap_or_default();
    let first_category = categories
        .iter()
        .find(|category| category.active)
        .or_else(|| categories.first())
        .map(|category| category.id.clone())
        .unwrap_or_default();
    let name = RwSignal::new(editing.as_ref().map(|i| i.name.clone()).unwrap_or_default());
    let category_id = RwSignal::new(
        editing
            .as_ref()
            .map(|i| i.category_id.clone())
            .unwrap_or(first_category),
    );
    let unit = RwSignal::new(
        editing
            .as_ref()
            .map(|i| i.unit_type)
            .unwrap_or(UnitType::Spoon),
    );
    let price = RwSignal::new(
        editing
            .as_ref()
            .map(|i| {
                format!(
                    "{}.{:02}",
                    i.price_per_unit_kobo / 100,
                    i.price_per_unit_kobo % 100
                )
            })
            .unwrap_or_default(),
    );
    let low_stock = RwSignal::new(
        editing
            .as_ref()
            .map(|i| i.low_stock_level.to_string())
            .unwrap_or_else(|| "5".to_owned()),
    );
    let active = RwSignal::new(editing.as_ref().is_none_or(|i| i.active));
    let busy = RwSignal::new(false);
    let photo = RwSignal::new(None::<SelectedPhoto>);
    let remove_photo = RwSignal::new(false);
    let image_url = editing.as_ref().and_then(|item| item.image_url.clone());

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let name_text = name.get_untracked().trim().to_owned();
        let Some(price_kobo) = parse_naira(&price.get_untracked()) else {
            app.toast(ToastKind::Error, "Enter a valid price.");
            return;
        };
        if name_text.is_empty() {
            app.toast(ToastKind::Error, "Give the item a name.");
            return;
        }
        let low = low_stock.get_untracked().trim().parse::<i64>().unwrap_or(0);
        busy.set(true);
        let api = app.api();
        let existing = existing.clone();
        let existing_id = existing.as_ref().map(|item| item.id.clone());
        let is_edit = existing.is_some();
        let category = category_id.get_untracked();
        let unit_type = unit.get_untracked();
        let photo = photo.get_untracked();
        let remove_photo = remove_photo.get_untracked();
        let lookup_name = name_text.clone();
        spawn_local(async move {
            let result = match &existing {
                Some(item) => {
                    api.update_item(
                        &item.id,
                        &UpdateItemRequest {
                            name: name_text,
                            category_id: category,
                            unit_type,
                            price_per_unit_kobo: price_kobo,
                            low_stock_level: low,
                            active: active.get_untracked(),
                            effective_from: None,
                        },
                    )
                    .await
                }
                None => {
                    api.create_item(&CreateItemRequest {
                        name: name_text,
                        category_id: category,
                        unit_type,
                        price_per_unit_kobo: price_kobo,
                        low_stock_level: low,
                        effective_from: None,
                    })
                    .await
                }
            };
            match result {
                Ok(saved) => {
                    app.menu.set(Some(saved.clone()));
                    match apply_item_photo(
                        &api,
                        saved,
                        existing_id,
                        &lookup_name,
                        photo,
                        remove_photo,
                    )
                    .await
                    {
                        Ok(menu) => {
                            app.menu.set(Some(menu));
                            app.notify("Item saved.");
                            on_close.run(());
                        }
                        Err(error) => {
                            app.fail(&error);
                            if is_edit {
                                app.toast(
                                    ToastKind::Error,
                                    "Item saved, but the photo could not be updated.",
                                );
                            } else {
                                app.toast(
                                    ToastKind::Error,
                                    "Item saved without the photo. Edit it to try again.",
                                );
                                on_close.run(());
                            }
                        }
                    }
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    view! {
        <Modal title=if editing.is_some() { "Edit item" } else { "New item" } wide=true on_close=on_close>
            <PhotoPicker
                app
                title="Item photo".to_owned()
                existing_url=image_url
                selected=photo
                remove=remove_photo
            />
            <div class="form-grid">
                <label class="field">
                    <span>"Name"</span>
                    <input
                        type="text"
                        prop:value=move || name.get()
                        on:input=move |event| name.set(event_target_value(&event))
                    />
                </label>
                <label class="field">
                    <span>"Category"</span>
                    <select on:change=move |event| category_id.set(event_target_value(&event))>
                        {categories
                            .iter()
                            .map(|category| {
                                let id = category.id.clone();
                                let selected = move || category_id.get() == id;
                                view! {
                                    <option value=category.id.clone() prop:selected=selected>
                                        {category.name.clone()}
                                    </option>
                                }
                            })
                            .collect_view()}
                    </select>
                </label>
                <label class="field">
                    <span>"Sold by"</span>
                    <select on:change=move |event| {
                        unit.set(if event_target_value(&event) == "count" {
                            UnitType::Count
                        } else {
                            UnitType::Spoon
                        });
                    }>
                        <option value="spoon" prop:selected=move || unit.get() == UnitType::Spoon>"Spoon"</option>
                        <option value="count" prop:selected=move || unit.get() == UnitType::Count>"Piece"</option>
                    </select>
                </label>
                <label class="field">
                    <span>"Price"</span>
                    <input
                        type="text"
                        inputmode="decimal"
                        prop:value=move || price.get()
                        on:input=move |event| price.set(event_target_value(&event))
                    />
                </label>
                <label class="field">
                    <span>"Low stock warning at"</span>
                    <input
                        type="number"
                        prop:value=move || low_stock.get()
                        on:input=move |event| low_stock.set(event_target_value(&event))
                    />
                </label>
            </div>
            {editing.is_some().then(|| view! {
                <label class="check">
                    <input
                        type="checkbox"
                        prop:checked=move || active.get()
                        on:change=move |event| active.set(event_target_checked(&event))
                    />
                    <span>"Shown on the sale screen"</span>
                </label>
            })}
            <p class="muted small">"Changing the price records a new dated price; past sales keep their old price."</p>
            <button class="button primary wide" on:click=submit disabled=move || busy.get()>
                {move || if busy.get() { view! { <Spinner /> }.into_any() } else { view! { "Save item" }.into_any() }}
            </button>
        </Modal>
    }
}

#[component]
fn PackForm(app: App, editing: Option<PackDto>, on_close: Callback<()>) -> impl IntoView {
    let existing = editing.clone();
    let items = app
        .menu
        .get_untracked()
        .map(|menu| menu.items)
        .unwrap_or_default();
    let first_item = items
        .iter()
        .find(|item| item.active)
        .map(|item| item.id.clone())
        .unwrap_or_default();
    let name = RwSignal::new(editing.as_ref().map(|p| p.name.clone()).unwrap_or_default());
    let mode = RwSignal::new(
        editing
            .as_ref()
            .map(|p| p.pricing_mode)
            .unwrap_or(PackPricingMode::Calculated),
    );
    let fixed = RwSignal::new(
        editing
            .as_ref()
            .and_then(|p| p.fixed_price_kobo)
            .map(|kobo| format!("{}.{:02}", kobo / 100, kobo % 100))
            .unwrap_or_default(),
    );
    let active = RwSignal::new(editing.as_ref().is_none_or(|p| p.active));
    let parts = RwSignal::new(
        editing
            .as_ref()
            .map(|pack| {
                pack.components
                    .iter()
                    .map(|component| (component.item_id.clone(), component.quantity.to_string()))
                    .collect::<Vec<(String, String)>>()
            })
            .filter(|parts| !parts.is_empty())
            .unwrap_or_else(|| vec![(first_item, "1".to_owned())]),
    );
    let busy = RwSignal::new(false);
    let photo = RwSignal::new(None::<SelectedPhoto>);
    let remove_photo = RwSignal::new(false);
    let image_url = editing.as_ref().and_then(|pack| pack.image_url.clone());

    let add_part = move |_| {
        parts.update(|parts| parts.push((String::new(), "1".to_owned())));
    };

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let name_text = name.get_untracked().trim().to_owned();
        if name_text.is_empty() {
            app.toast(ToastKind::Error, "Give the pack a name.");
            return;
        }
        let mut components = Vec::new();
        for (item_id, quantity) in parts.get_untracked() {
            if item_id.is_empty() {
                app.toast(
                    ToastKind::Error,
                    "Choose an item for every part of the pack.",
                );
                return;
            }
            let Ok(quantity) = quantity.trim().parse::<i64>() else {
                app.toast(ToastKind::Error, "Pack quantities must be whole numbers.");
                return;
            };
            if quantity <= 0 {
                app.toast(ToastKind::Error, "Pack quantities must be at least 1.");
                return;
            }
            components.push(PackComponentInput { item_id, quantity });
        }
        if components.is_empty() {
            app.toast(ToastKind::Error, "A pack needs at least one part.");
            return;
        }
        let pricing = mode.get_untracked();
        let fixed_price = if pricing == PackPricingMode::Fixed {
            match parse_naira(&fixed.get_untracked()) {
                Some(price) => Some(price),
                None => {
                    app.toast(ToastKind::Error, "Enter a valid pack price.");
                    return;
                }
            }
        } else {
            None
        };
        busy.set(true);
        let api = app.api();
        let existing = existing.clone();
        let existing_id = existing.as_ref().map(|pack| pack.id.clone());
        let is_edit = existing.is_some();
        let photo = photo.get_untracked();
        let remove_photo = remove_photo.get_untracked();
        let lookup_name = name_text.clone();
        spawn_local(async move {
            let result = match &existing {
                Some(pack) => {
                    api.update_pack(
                        &pack.id,
                        &UpdatePackRequest {
                            name: name_text,
                            pricing_mode: pricing,
                            fixed_price_kobo: fixed_price,
                            active: active.get_untracked(),
                            components,
                        },
                    )
                    .await
                }
                None => {
                    api.create_pack(&CreatePackRequest {
                        name: name_text,
                        pricing_mode: pricing,
                        fixed_price_kobo: fixed_price,
                        components,
                    })
                    .await
                }
            };
            match result {
                Ok(saved) => {
                    app.menu.set(Some(saved.clone()));
                    match apply_pack_photo(
                        &api,
                        saved,
                        existing_id,
                        &lookup_name,
                        photo,
                        remove_photo,
                    )
                    .await
                    {
                        Ok(menu) => {
                            app.menu.set(Some(menu));
                            app.notify("Pack saved.");
                            on_close.run(());
                        }
                        Err(error) => {
                            app.fail(&error);
                            if is_edit {
                                app.toast(
                                    ToastKind::Error,
                                    "Pack saved, but the photo could not be updated.",
                                );
                            } else {
                                app.toast(
                                    ToastKind::Error,
                                    "Pack saved without the photo. Edit it to try again.",
                                );
                                on_close.run(());
                            }
                        }
                    }
                }
                Err(error) => app.fail(&error),
            }
            busy.set(false);
        });
    };

    view! {
        <Modal title=if editing.is_some() { "Edit pack" } else { "New pack" } wide=true on_close=on_close>
            <PhotoPicker
                app
                title="Pack photo".to_owned()
                existing_url=image_url
                selected=photo
                remove=remove_photo
            />
            <div class="form-grid">
                <label class="field">
                    <span>"Name"</span>
                    <input
                        type="text"
                        prop:value=move || name.get()
                        on:input=move |event| name.set(event_target_value(&event))
                    />
                </label>
                <label class="field">
                    <span>"Pricing"</span>
                    <select on:change=move |event| {
                        mode.set(if event_target_value(&event) == "fixed" {
                            PackPricingMode::Fixed
                        } else {
                            PackPricingMode::Calculated
                        });
                    }>
                        <option value="calculated" prop:selected=move || mode.get() == PackPricingMode::Calculated>
                            "Sum of parts"
                        </option>
                        <option value="fixed" prop:selected=move || mode.get() == PackPricingMode::Fixed>
                            "Fixed price"
                        </option>
                    </select>
                </label>
                {move || (mode.get() == PackPricingMode::Fixed).then(|| view! {
                    <label class="field">
                        <span>"Pack price"</span>
                        <input
                            type="text"
                            inputmode="decimal"
                            prop:value=move || fixed.get()
                            on:input=move |event| fixed.set(event_target_value(&event))
                        />
                    </label>
                })}
            </div>

            <div class="parts">
                <div class="panel-head">
                    <h3>"Contents"</h3>
                    <button class="button ghost small" on:click=add_part>"Add part"</button>
                </div>
                {move || {
                    parts.get()
                        .iter()
                        .enumerate()
                        .map(|(index, (item_id, quantity))| {
                            let selected = item_id.clone();
                            let quantity = quantity.clone();
                            view! {
                                <div class="part-row">
                                    <select on:change=move |event| {
                                        let value = event_target_value(&event);
                                        parts.update(|parts| {
                                            if let Some((item, _)) = parts.get_mut(index) {
                                                *item = value;
                                            }
                                        });
                                    }>
                                        {items
                                            .iter()
                                            .filter(|item| item.active)
                                            .map(|item| {
                                                let id = item.id.clone();
                                                let is_selected = selected == id;
                                                view! {
                                                    <option value=item.id.clone() prop:selected=is_selected>
                                                        {format!("{} ({})", item.name, money(item.price_per_unit_kobo))}
                                                    </option>
                                                }
                                            })
                                            .collect_view()}
                                    </select>
                                    <input
                                        type="number"
                                        min="1"
                                        prop:value=quantity
                                        on:input=move |event| {
                                            let value = event_target_value(&event);
                                            parts.update(|parts| {
                                                if let Some((_, qty)) = parts.get_mut(index) {
                                                    *qty = value;
                                                }
                                            });
                                        }
                                    />
                                    <button
                                        class="step remove"
                                        on:click=move |_| {
                                            parts.update(|parts| {
                                                if parts.len() > 1 {
                                                    parts.remove(index);
                                                }
                                            });
                                        }
                                    >
                                        "\u{00d7}"
                                    </button>
                                </div>
                            }
                        })
                        .collect_view()
                }}
            </div>

            {editing.is_some().then(|| view! {
                <label class="check">
                    <input
                        type="checkbox"
                        prop:checked=move || active.get()
                        on:change=move |event| active.set(event_target_checked(&event))
                    />
                    <span>"Shown on the sale screen"</span>
                </label>
            })}
            <button class="button primary wide" on:click=submit disabled=move || busy.get()>
                {move || if busy.get() { view! { <Spinner /> }.into_any() } else { view! { "Save pack" }.into_any() }}
            </button>
        </Modal>
    }
}
