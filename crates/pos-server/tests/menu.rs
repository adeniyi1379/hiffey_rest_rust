use std::io::Cursor;

use axum::{
    body::Body,
    http::{header, HeaderMap, Method, Request, StatusCode},
    Router,
};
use chrono::{Duration, Utc};
use image::{ImageFormat, Rgba, RgbaImage};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tower::ServiceExt;

const TERMINAL: &str = "menu-test";

async fn test_app() -> (Router, SqlitePool) {
    let pool = pos_server::db::connect_memory().await.unwrap();
    pos_server::db::migrate(&pool).await.unwrap();
    (
        pos_server::router(pos_server::AppState::new(pool.clone())),
        pool,
    )
}

fn request(method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-terminal-id", TERMINAL);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let (status, _, bytes) = send_raw(app, request).await;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

async fn send_raw(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, bytes.to_vec())
}

fn raw_request(
    method: Method,
    uri: &str,
    token: Option<&str>,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-terminal-id", TERMINAL);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    builder.body(Body::from(body)).unwrap()
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([(x % 255) as u8, (y % 255) as u8, 180, 255])
    });
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

async fn get(app: &Router, uri: &str, token: &str) -> (StatusCode, Value) {
    send(app, request(Method::GET, uri, Some(token), None)).await
}

async fn post(app: &Router, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    send(app, request(Method::POST, uri, Some(token), Some(body))).await
}

async fn put(app: &Router, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    send(app, request(Method::PUT, uri, Some(token), Some(body))).await
}

async fn owner_token(app: &Router) -> String {
    let (status, body) = post(
        app,
        "/setup",
        "",
        json!({ "business_name": "Hiffey Restaurant", "owner_name": "Chidi" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["session"]["token"].as_str().unwrap().to_owned()
}

async fn create_item(
    app: &Router,
    token: &str,
    category_id: &str,
    name: &str,
    price: i64,
) -> Value {
    let (status, body) = post(
        app,
        "/admin/items",
        token,
        json!({
            "name": name,
            "category_id": category_id,
            "unit_type": "spoon",
            "price_per_unit_kobo": price,
            "low_stock_level": 5,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn item_by_name<'a>(menu: &'a Value, name: &str) -> &'a Value {
    menu["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("item {name} not in menu"))
}

fn category_id(menu: &Value, name: &str) -> String {
    menu["categories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|category| category["name"] == name)
        .unwrap_or_else(|| panic!("category {name} not seeded"))["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn menu_needs_a_session() {
    let (app, _pool) = test_app().await;
    let (status, _) = send(&app, request(Method::GET, "/menu", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn setup_seeds_the_default_categories() {
    let (app, _pool) = test_app().await;
    let token = owner_token(&app).await;
    let (status, menu) = get(&app, "/menu", &token).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = menu["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|category| category["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Rice", "Sides", "Pastries", "Drinks", "Packs"]);
    assert!(menu["items"].as_array().unwrap().is_empty());
    assert!(menu["packs"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn owner_creates_items_and_their_prices_appear() {
    let (app, _pool) = test_app().await;
    let token = owner_token(&app).await;
    let (_, menu) = get(&app, "/menu", &token).await;
    let rice = category_id(&menu, "Rice");

    let menu = create_item(&app, &token, &rice, "Jollof rice", 35_000).await;
    let jollof = item_by_name(&menu, "Jollof rice");
    assert_eq!(jollof["price_per_unit_kobo"], 35_000);
    assert_eq!(jollof["unit_type"], "spoon");
    assert_eq!(jollof["category_name"], "Rice");
    assert_eq!(jollof["low_stock_level"], 5);
}

#[tokio::test]
async fn cashier_cannot_change_the_menu() {
    let (app, _pool) = test_app().await;
    let token = owner_token(&app).await;
    let (_, menu) = get(&app, "/menu", &token).await;
    let rice = category_id(&menu, "Rice");

    let (status, created) = post(
        &app,
        "/admin/workers",
        &token,
        json!({ "name": "Ada", "role": "cashier" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let pin = created["pin"].as_str().unwrap().to_owned();

    let (status, login) = post(&app, "/auth/login", "", json!({ "pin": pin })).await;
    assert_eq!(status, StatusCode::OK);
    let cashier = login["token"].as_str().unwrap().to_owned();

    let (status, _) = post(
        &app,
        "/admin/items",
        &cashier,
        json!({
            "name": "Jollof rice",
            "category_id": rice,
            "unit_type": "spoon",
            "price_per_unit_kobo": 35_000,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = get(&app, "/menu", &cashier).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn duplicate_names_and_bad_prices_are_rejected() {
    let (app, _pool) = test_app().await;
    let token = owner_token(&app).await;
    let (_, menu) = get(&app, "/menu", &token).await;
    let rice = category_id(&menu, "Rice");
    create_item(&app, &token, &rice, "Jollof rice", 35_000).await;

    let (status, body) = post(
        &app,
        "/admin/items",
        &token,
        json!({
            "name": "jollof rice",
            "category_id": rice,
            "unit_type": "spoon",
            "price_per_unit_kobo": 40_000,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, _) = post(
        &app,
        "/admin/items",
        &token,
        json!({
            "name": "Fried rice",
            "category_id": rice,
            "unit_type": "spoon",
            "price_per_unit_kobo": -1,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn future_prices_wait_until_their_time() {
    let (app, pool) = test_app().await;
    let token = owner_token(&app).await;
    let (_, menu) = get(&app, "/menu", &token).await;
    let rice = category_id(&menu, "Rice");
    let menu = create_item(&app, &token, &rice, "Jollof rice", 35_000).await;
    let id = item_by_name(&menu, "Jollof rice")["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let future = (Utc::now() + Duration::days(1)).to_rfc3339();
    let (status, menu) = put(
        &app,
        &format!("/admin/items/{id}"),
        &token,
        json!({
            "name": "Jollof rice",
            "category_id": rice,
            "unit_type": "spoon",
            "price_per_unit_kobo": 40_000,
            "low_stock_level": 5,
            "active": true,
            "effective_from": future,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        item_by_name(&menu, "Jollof rice")["price_per_unit_kobo"],
        35_000
    );

    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM item_prices WHERE item_id = ?")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 2);

    let (status, menu) = put(
        &app,
        &format!("/admin/items/{id}"),
        &token,
        json!({
            "name": "Jollof rice",
            "category_id": rice,
            "unit_type": "spoon",
            "price_per_unit_kobo": 40_000,
            "low_stock_level": 5,
            "active": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        item_by_name(&menu, "Jollof rice")["price_per_unit_kobo"],
        40_000
    );
}

#[tokio::test]
async fn packs_price_from_components_or_a_fixed_price() {
    let (app, _pool) = test_app().await;
    let token = owner_token(&app).await;
    let (_, menu) = get(&app, "/menu", &token).await;
    let rice = category_id(&menu, "Rice");
    let menu = create_item(&app, &token, &rice, "Jollof rice", 35_000).await;
    let jollof = item_by_name(&menu, "Jollof rice")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let menu = create_item(&app, &token, &rice, "Fried rice", 40_000).await;
    let fried = item_by_name(&menu, "Fried rice")["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, menu) = post(
        &app,
        "/admin/packs",
        &token,
        json!({
            "name": "Standard Pack",
            "pricing_mode": "calculated",
            "components": [
                { "item_id": jollof, "quantity": 2 },
                { "item_id": fried, "quantity": 2 },
            ],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pack = &menu["packs"][0];
    assert_eq!(pack["name"], "Standard Pack");
    assert_eq!(pack["price_kobo"], 150_000);
    assert_eq!(pack["components"].as_array().unwrap().len(), 2);

    let pack_id = pack["id"].as_str().unwrap().to_owned();
    let (status, menu) = put(
        &app,
        &format!("/admin/packs/{pack_id}"),
        &token,
        json!({
            "name": "Standard Pack",
            "pricing_mode": "fixed",
            "fixed_price_kobo": 140_000,
            "active": true,
            "components": [
                { "item_id": jollof, "quantity": 2 },
                { "item_id": fried, "quantity": 2 },
            ],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(menu["packs"][0]["price_kobo"], 140_000);

    let (status, menu) = post(
        &app,
        &format!("/admin/packs/{pack_id}/duplicate"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(menu["packs"].as_array().unwrap().len(), 2);
    let names: Vec<&str> = menu["packs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pack| pack["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Standard Pack (copy)"));
}

#[tokio::test]
async fn packs_reject_missing_items_and_empty_contents() {
    let (app, _pool) = test_app().await;
    let token = owner_token(&app).await;

    let (status, _) = post(
        &app,
        "/admin/packs",
        &token,
        json!({ "name": "Empty", "pricing_mode": "calculated", "components": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post(
        &app,
        "/admin/packs",
        &token,
        json!({
            "name": "Ghost",
            "pricing_mode": "calculated",
            "components": [{ "item_id": "missing", "quantity": 1 }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn item_photos_are_resized_served_publicly_and_removed() {
    let (app, _pool) = test_app().await;
    let token = owner_token(&app).await;
    let (_, menu) = get(&app, "/menu", &token).await;
    let rice = category_id(&menu, "Rice");
    let menu = create_item(&app, &token, &rice, "Jollof rice", 35_000).await;
    let id = item_by_name(&menu, "Jollof rice")["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let upload_uri = format!("/admin/items/{id}/image");
    let (status, _, body) = send_raw(
        &app,
        raw_request(
            Method::POST,
            &upload_uri,
            Some(&token),
            Some("image/png"),
            png_bytes(1600, 800),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, menu) = get(&app, "/menu", &token).await;
    assert_eq!(status, StatusCode::OK);
    let image_url = item_by_name(&menu, "Jollof rice")["image_url"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(image_url.starts_with(&format!("/menu/items/{id}/image?v=")));

    let (status, headers, bytes) = send_raw(
        &app,
        raw_request(Method::GET, &image_url, None, None, Vec::new()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get(header::CONTENT_TYPE).unwrap().to_str().unwrap(),
        "image/jpeg"
    );
    assert!(headers
        .get(header::CACHE_CONTROL)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("immutable"));
    assert!(bytes.starts_with(&[0xFF, 0xD8, 0xFF]));
    let resized = image::load_from_memory(&bytes).unwrap();
    assert_eq!((resized.width(), resized.height()), (1200, 600));

    let (status, menu) = send(
        &app,
        raw_request(Method::DELETE, &upload_uri, Some(&token), None, Vec::new()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(item_by_name(&menu, "Jollof rice")["image_url"].is_null());

    let (status, _, _) = send_raw(
        &app,
        raw_request(Method::GET, &image_url, None, None, Vec::new()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_photos_are_rejected_and_cashiers_cannot_upload() {
    let (app, _pool) = test_app().await;
    let token = owner_token(&app).await;
    let (_, menu) = get(&app, "/menu", &token).await;
    let rice = category_id(&menu, "Rice");
    let menu = create_item(&app, &token, &rice, "Jollof rice", 35_000).await;
    let id = item_by_name(&menu, "Jollof rice")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let upload_uri = format!("/admin/items/{id}/image");

    let (status, _, _) = send_raw(
        &app,
        raw_request(
            Method::POST,
            &upload_uri,
            Some(&token),
            Some("text/plain"),
            png_bytes(10, 10),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _, _) = send_raw(
        &app,
        raw_request(
            Method::POST,
            &upload_uri,
            Some(&token),
            Some("image/png"),
            b"not an image".to_vec(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, created) = post(
        &app,
        "/admin/workers",
        &token,
        json!({ "name": "Ada", "role": "cashier" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let pin = created["pin"].as_str().unwrap().to_owned();
    let (_, login) = post(&app, "/auth/login", "", json!({ "pin": pin })).await;
    let cashier = login["token"].as_str().unwrap().to_owned();

    let (status, _, _) = send_raw(
        &app,
        raw_request(
            Method::POST,
            &upload_uri,
            Some(&cashier),
            Some("image/png"),
            png_bytes(10, 10),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn duplicating_a_pack_copies_its_photo() {
    let (app, _pool) = test_app().await;
    let token = owner_token(&app).await;
    let (_, menu) = get(&app, "/menu", &token).await;
    let rice = category_id(&menu, "Rice");
    let menu = create_item(&app, &token, &rice, "Jollof rice", 35_000).await;
    let jollof = item_by_name(&menu, "Jollof rice")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, menu) = post(
        &app,
        "/admin/packs",
        &token,
        json!({
            "name": "Standard Pack",
            "pricing_mode": "calculated",
            "components": [{ "item_id": jollof, "quantity": 2 }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pack_id = menu["packs"][0]["id"].as_str().unwrap().to_owned();

    let upload_uri = format!("/admin/packs/{pack_id}/image");
    let (status, _, _) = send_raw(
        &app,
        raw_request(
            Method::POST,
            &upload_uri,
            Some(&token),
            Some("image/png"),
            png_bytes(40, 20),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, menu) = post(
        &app,
        &format!("/admin/packs/{pack_id}/duplicate"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let original = menu["packs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pack| pack["name"] == "Standard Pack")
        .unwrap();
    let copy = menu["packs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pack| pack["name"] == "Standard Pack (copy)")
        .unwrap();
    let original_url = original["image_url"].as_str().unwrap();
    let copy_url = copy["image_url"].as_str().unwrap();
    assert_ne!(original_url, copy_url);

    let (status, _, bytes) = send_raw(
        &app,
        raw_request(Method::GET, copy_url, None, None, Vec::new()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(bytes.starts_with(&[0xFF, 0xD8, 0xFF]));
}
