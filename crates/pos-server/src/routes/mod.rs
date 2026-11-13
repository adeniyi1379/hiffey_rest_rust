pub mod admin;
pub mod admin_menu;
pub mod auth;
pub mod menu;
pub mod misc;
pub mod orders;
pub mod print;
pub mod reports;
pub mod settings;
pub mod setup;
pub mod stock;

use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::{error::ApiError, AppState};

/// The full HTTP surface of the LAN server.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/setup/status", get(setup::status))
        .route("/setup", post(setup::complete))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/settings", get(settings::get).put(settings::update))
        .route("/menu", get(menu::get))
        .route("/menu/items/{id}/image", get(menu::get_item_image))
        .route("/menu/packs/{id}/image", get(menu::get_pack_image))
        .route("/orders", post(orders::create))
        .route("/orders/held", get(orders::held))
        .route("/orders/{id}", get(orders::get).put(orders::update))
        .route("/orders/{id}/pay", post(orders::pay))
        .route("/orders/{id}/void", post(orders::void))
        .route("/stock/balances", get(stock::balances))
        .route("/stock/batches", post(stock::add_batch))
        .route("/stock/restock", post(stock::restock))
        .route("/stock/wastage", post(stock::record_wastage))
        .route("/stock/counts", post(stock::count))
        .route("/stock/ledger", get(stock::ledger))
        .route("/orders/{id}/print", post(print::create))
        .route("/print/jobs", get(print::list))
        .route("/print/jobs/{id}", get(print::get_job))
        .route("/print/jobs/{id}/result", post(print::result))
        .route("/admin/workers", get(admin::list).post(admin::create))
        .route("/admin/workers/{id}/reset-pin", post(admin::reset_pin))
        .route("/admin/workers/{id}/deactivate", post(admin::deactivate))
        .route("/admin/categories", post(admin_menu::create_category))
        .route("/admin/categories/{id}", put(admin_menu::update_category))
        .route("/admin/items", post(admin_menu::create_item))
        .route("/admin/items/{id}", put(admin_menu::update_item))
        .route(
            "/admin/items/{id}/image",
            post(admin_menu::upload_item_image)
                .delete(admin_menu::delete_item_image)
                .layer(DefaultBodyLimit::max(12 * 1024 * 1024)),
        )
        .route("/admin/packs", post(admin_menu::create_pack))
        .route("/admin/packs/{id}", put(admin_menu::update_pack))
        .route(
            "/admin/packs/{id}/image",
            post(admin_menu::upload_pack_image)
                .delete(admin_menu::delete_pack_image)
                .layer(DefaultBodyLimit::max(12 * 1024 * 1024)),
        )
        .route(
            "/admin/packs/{id}/duplicate",
            post(admin_menu::duplicate_pack),
        )
        .route("/reports/daily", get(reports::daily))
        .route("/reports/sales.csv", get(reports::sales_csv))
        .route("/reports/stock.csv", get(reports::stock_csv))
        .route("/sync/status", get(misc::sync_status))
        .route("/events", get(misc::events))
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "hiffey-pos",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Trims and validates a required text field.
pub(crate) fn require_text(value: &str, field: &str, max_len: usize) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiError::bad_request(format!("{field} is required.")));
    }
    if trimmed.chars().count() > max_len {
        return Err(ApiError::bad_request(format!(
            "{field} must be {max_len} characters or fewer."
        )));
    }
    Ok(trimmed.to_owned())
}
