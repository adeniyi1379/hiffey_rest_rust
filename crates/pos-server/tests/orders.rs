use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tower::ServiceExt;

const TERMINAL: &str = "order-terminal";

async fn test_app() -> (Router, SqlitePool) {
    let pool = pos_server::db::connect_memory().await.unwrap();
    pos_server::db::migrate(&pool).await.unwrap();
    (
        pos_server::router(pos_server::AppState::new(pool.clone())),
        pool,
    )
}

fn request(
    method: Method,
    uri: &str,
    token: Option<&str>,
    terminal: &str,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-terminal-id", terminal);
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

struct Reply {
    status: StatusCode,
    body: Value,
}

async fn send(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    Reply { status, body }
}

async fn get(app: &Router, uri: &str, token: &str) -> Reply {
    send(app, request(Method::GET, uri, Some(token), TERMINAL, None)).await
}

async fn post(app: &Router, uri: &str, token: &str, body: Value) -> Reply {
    send(
        app,
        request(Method::POST, uri, Some(token), TERMINAL, Some(body)),
    )
    .await
}

async fn put(app: &Router, uri: &str, token: &str, body: Value) -> Reply {
    send(
        app,
        request(Method::PUT, uri, Some(token), TERMINAL, Some(body)),
    )
    .await
}

async fn setup(app: &Router) -> (String, String) {
    let reply = send(
        app,
        request(
            Method::POST,
            "/setup",
            None,
            TERMINAL,
            Some(json!({
                "business_name": "Hiffey Restaurant",
                "owner_name": "Chidi",
                "business_address": "12 Market Road",
                "business_phone": "08030000000",
            })),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let pin = reply.body["pin"].as_str().unwrap().to_owned();
    let token = reply.body["session"]["token"].as_str().unwrap().to_owned();
    (pin, token)
}

async fn login(app: &Router, terminal: &str, pin: &str) -> String {
    let reply = send(
        app,
        request(
            Method::POST,
            "/auth/login",
            None,
            terminal,
            Some(json!({ "pin": pin })),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["token"].as_str().unwrap().to_owned()
}

fn find_category(menu: &Value, name: &str) -> String {
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

async fn create_item(
    app: &Router,
    token: &str,
    category_id: &str,
    name: &str,
    price: i64,
) -> String {
    let reply = post(
        app,
        "/admin/items",
        token,
        json!({
            "name": name,
            "category_id": category_id,
            "unit_type": "spoon",
            "price_per_unit_kobo": price,
            "low_stock_level": 2,
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn add_stock(app: &Router, token: &str, item_id: &str, quantity: i64) {
    let reply = post(
        app,
        "/stock/batches",
        token,
        json!({ "item_id": item_id, "quantity": quantity }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
}

async fn create_worker(app: &Router, owner_token: &str, name: &str, role: &str) -> String {
    let reply = post(
        app,
        "/admin/workers",
        owner_token,
        json!({ "name": name, "role": role }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["pin"].as_str().unwrap().to_owned()
}

async fn create_order(app: &Router, token: &str, body: Value) -> Reply {
    post(app, "/orders", token, body).await
}

#[tokio::test]
async fn held_orders_take_the_next_number() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 20).await;

    let first = create_order(
        &app,
        &token,
        json!({
            "order_type": "dine_in",
            "table_no": "4",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 2 }],
        }),
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.body);
    assert_eq!(first.body["status"], "held");
    assert_eq!(first.body["subtotal_kobo"], 70_000);
    assert_eq!(first.body["total_kobo"], 70_000);
    assert_eq!(first.body["lines"].as_array().unwrap().len(), 1);
    assert_eq!(
        first.body["lines"][0]["components"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let first_no = first.body["order_no"].as_str().unwrap().to_owned();
    assert!(
        first_no.starts_with("HIF-") && first_no.ends_with("-0001"),
        "unexpected order number {first_no}"
    );

    let second = create_order(
        &app,
        &token,
        json!({
            "order_type": "takeaway",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 1 }],
        }),
    )
    .await;
    assert_eq!(second.status, StatusCode::CREATED, "{}", second.body);
    assert!(second.body["order_no"].as_str().unwrap().ends_with("-0002"));

    let held = get(&app, "/orders/held", &token).await;
    assert_eq!(held.status, StatusCode::OK);
    assert_eq!(held.body.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn dine_in_must_have_a_table() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;

    let reply = create_order(
        &app,
        &token,
        json!({
            "order_type": "dine_in",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 1 }],
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn takeaway_carries_the_packaging_fee() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 10).await;

    let settings = put(
        &app,
        "/settings",
        &token,
        json!({
            "business_name": "Hiffey Restaurant",
            "business_address": "",
            "business_phone": "",
            "receipt_footer": "",
            "packaging_fee_kobo": 20_000,
            "low_stock_default": 5,
        }),
    )
    .await;
    assert_eq!(settings.status, StatusCode::OK, "{}", settings.body);

    let takeaway = create_order(
        &app,
        &token,
        json!({
            "order_type": "takeaway",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 2 }],
        }),
    )
    .await;
    assert_eq!(takeaway.status, StatusCode::CREATED, "{}", takeaway.body);
    assert_eq!(takeaway.body["packaging_fee_kobo"], 20_000);
    assert_eq!(takeaway.body["total_kobo"], 90_000);

    let dine_in = create_order(
        &app,
        &token,
        json!({
            "order_type": "dine_in",
            "table_no": "2",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 2 }],
        }),
    )
    .await;
    assert_eq!(dine_in.body["packaging_fee_kobo"], 0);
    assert_eq!(dine_in.body["total_kobo"], 70_000);
}

#[tokio::test]
async fn paying_deducts_stock_and_returns_change() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 10).await;

    let order = create_order(
        &app,
        &token,
        json!({
            "order_type": "takeaway",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 3 }],
        }),
    )
    .await;
    let order_id = order.body["id"].as_str().unwrap().to_owned();

    let paid = post(
        &app,
        &format!("/orders/{order_id}/pay"),
        &token,
        json!({ "payments": [{ "method": "cash", "amount_kobo": 120_000 }] }),
    )
    .await;
    assert_eq!(paid.status, StatusCode::OK, "{}", paid.body);
    assert_eq!(paid.body["change_kobo"], 15_000);
    assert_eq!(paid.body["order"]["status"], "paid");
    assert_eq!(paid.body["order"]["payments"].as_array().unwrap().len(), 1);

    let balances = get(&app, "/stock/balances", &token).await;
    let item = balances
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["item_id"] == item_id)
        .unwrap();
    assert_eq!(item["balance"], 7);

    let again = post(
        &app,
        &format!("/orders/{order_id}/pay"),
        &token,
        json!({ "payments": [{ "method": "cash", "amount_kobo": 1 }] }),
    )
    .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn cannot_pay_for_stock_that_is_not_there() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 1).await;

    let order = create_order(
        &app,
        &token,
        json!({
            "order_type": "takeaway",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 2 }],
        }),
    )
    .await;
    assert_eq!(order.status, StatusCode::CREATED, "{}", order.body);
    let order_id = order.body["id"].as_str().unwrap().to_owned();

    let reply = post(
        &app,
        &format!("/orders/{order_id}/pay"),
        &token,
        json!({ "payments": [{ "method": "cash", "amount_kobo": 70_000 }] }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.body);
    assert_eq!(reply.body["code"], "insufficient_stock");

    let still_held = get(&app, &format!("/orders/{order_id}"), &token).await;
    assert_eq!(still_held.body["status"], "held");

    let balances = get(&app, "/stock/balances", &token).await;
    let item = balances
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["item_id"] == item_id)
        .unwrap();
    assert_eq!(item["balance"], 1);
}

#[tokio::test]
async fn short_payment_is_rejected() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 3).await;

    let order = create_order(
        &app,
        &token,
        json!({
            "order_type": "takeaway",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 1 }],
        }),
    )
    .await;
    let order_id = order.body["id"].as_str().unwrap().to_owned();

    let reply = post(
        &app,
        &format!("/orders/{order_id}/pay"),
        &token,
        json!({ "payments": [{ "method": "cash", "amount_kobo": 1_000 }] }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn voiding_a_paid_order_needs_a_manager_and_returns_stock() {
    let (app, _pool) = test_app().await;
    let (owner_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 5).await;

    let cashier_pin = create_worker(&app, &token, "Ada", "cashier").await;
    let _cashier = login(&app, "cashier-terminal", &cashier_pin).await;

    let order = create_order(
        &app,
        &token,
        json!({
            "order_type": "takeaway",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 2 }],
        }),
    )
    .await;
    let order_id = order.body["id"].as_str().unwrap().to_owned();
    let paid = post(
        &app,
        &format!("/orders/{order_id}/pay"),
        &token,
        json!({ "payments": [{ "method": "transfer", "amount_kobo": 70_000, "reference": "TRX1" }] }),
    )
    .await;
    assert_eq!(paid.status, StatusCode::OK, "{}", paid.body);

    let no_pin = post(
        &app,
        &format!("/orders/{order_id}/void"),
        &token,
        json!({ "reason": "Wrong order", "return_to_stock": true }),
    )
    .await;
    assert_eq!(no_pin.status, StatusCode::UNAUTHORIZED);

    let cashier_attempt = post(
        &app,
        &format!("/orders/{order_id}/void"),
        &token,
        json!({
            "manager_pin": cashier_pin,
            "reason": "Wrong order",
            "return_to_stock": true,
        }),
    )
    .await;
    assert_eq!(cashier_attempt.status, StatusCode::FORBIDDEN);

    let by_owner = post(
        &app,
        &format!("/orders/{order_id}/void"),
        &token,
        json!({
            "manager_pin": owner_pin,
            "reason": "Wrong order",
            "return_to_stock": true,
        }),
    )
    .await;
    assert_eq!(by_owner.status, StatusCode::OK, "{}", by_owner.body);
    assert_eq!(by_owner.body["status"], "voided");
    assert_eq!(by_owner.body["void_reason"], "Wrong order");

    let balances = get(&app, "/stock/balances", &token).await;
    let item = balances
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["item_id"] == item_id)
        .unwrap();
    assert_eq!(item["balance"], 5);
}

#[tokio::test]
async fn held_orders_can_be_edited_and_discarded_without_a_pin() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 5).await;

    let order = create_order(
        &app,
        &token,
        json!({
            "order_type": "dine_in",
            "table_no": "1",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 1 }],
        }),
    )
    .await;
    let order_id = order.body["id"].as_str().unwrap().to_owned();

    let updated = put(
        &app,
        &format!("/orders/{order_id}"),
        &token,
        json!({
            "order_type": "takeaway",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 4 }],
        }),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(updated.body["order_type"], "takeaway");
    assert_eq!(updated.body["total_kobo"], 140_000);

    let voided = post(
        &app,
        &format!("/orders/{order_id}/void"),
        &token,
        json!({ "reason": "Customer left" }),
    )
    .await;
    assert_eq!(voided.status, StatusCode::OK, "{}", voided.body);
    assert_eq!(voided.body["status"], "voided");

    let held = get(&app, "/orders/held", &token).await;
    assert!(held.body.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn batch_stock_is_restricted_to_managers() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;

    let cashier_pin = create_worker(&app, &token, "Ada", "cashier").await;
    let cashier_token = login(&app, "cashier-terminal-2", &cashier_pin).await;

    let reply = post(
        &app,
        "/stock/batches",
        &cashier_token,
        json!({ "item_id": item_id, "quantity": 10 }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
}

fn menu_item<'a>(menu: &'a Value, item_id: &str) -> &'a Value {
    menu["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == item_id)
        .unwrap_or_else(|| panic!("item {item_id} not in menu"))
}

#[tokio::test]
async fn menu_shows_stock_and_sold_out_state() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Pastries");
    let item_id = create_item(&app, &token, &category, "Meat Pie", 20_000).await;

    let fresh = get(&app, "/menu", &token).await;
    let item = menu_item(&fresh.body, &item_id);
    assert_eq!(item["stock_balance"], 0);
    assert_eq!(item["sold_out"], true);
    assert_eq!(item["low_stock"], false);

    add_stock(&app, &token, &item_id, 2).await;
    let stocked = get(&app, "/menu", &token).await;
    let item = menu_item(&stocked.body, &item_id);
    assert_eq!(item["stock_balance"], 2);
    assert_eq!(item["sold_out"], false);
    assert_eq!(item["low_stock"], true);

    let restocked = post(
        &app,
        "/stock/restock",
        &token,
        json!({ "item_id": item_id, "quantity": 8, "reason": "Evening tray" }),
    )
    .await;
    assert_eq!(restocked.status, StatusCode::OK, "{}", restocked.body);
    assert_eq!(restocked.body["balance"], 10);

    let item = &get(&app, "/menu", &token).await.body;
    let item = menu_item(item, &item_id);
    assert_eq!(item["stock_balance"], 10);
    assert_eq!(item["low_stock"], false);
}

#[tokio::test]
async fn a_pack_is_sold_out_when_a_component_runs_out() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let rice = find_category(&menu.body, "Rice");
    let jollof = create_item(&app, &token, &rice, "Jollof Rice", 35_000).await;
    let fried = create_item(&app, &token, &rice, "Fried Rice", 40_000).await;
    add_stock(&app, &token, &jollof, 10).await;

    let pack = post(
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
    assert_eq!(pack.status, StatusCode::OK, "{}", pack.body);
    assert_eq!(pack.body["packs"][0]["sold_out"], true);

    add_stock(&app, &token, &fried, 3).await;
    let menu = get(&app, "/menu", &token).await;
    assert_eq!(menu.body["packs"][0]["sold_out"], false);
}

#[tokio::test]
async fn wastage_comes_off_stock_and_cannot_go_below_zero() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 10).await;

    let missing_reason = post(
        &app,
        "/stock/wastage",
        &token,
        json!({ "item_id": item_id, "quantity": 1, "reason": "  " }),
    )
    .await;
    assert_eq!(missing_reason.status, StatusCode::BAD_REQUEST);

    let wasted = post(
        &app,
        "/stock/wastage",
        &token,
        json!({ "item_id": item_id, "quantity": 3, "reason": "spoiled" }),
    )
    .await;
    assert_eq!(wasted.status, StatusCode::OK, "{}", wasted.body);
    assert_eq!(wasted.body["balance"], 7);

    let too_much = post(
        &app,
        "/stock/wastage",
        &token,
        json!({ "item_id": item_id, "quantity": 100, "reason": "mistake" }),
    )
    .await;
    assert_eq!(too_much.status, StatusCode::BAD_REQUEST);

    let ledger = get(&app, &format!("/stock/ledger?item_id={item_id}"), &token).await;
    let entries = ledger.body.as_array().unwrap();
    let wastage = entries
        .iter()
        .find(|entry| entry["movement_type"] == "wastage")
        .unwrap();
    assert_eq!(wastage["quantity"], -3);
    assert_eq!(wastage["reason"], "spoiled");
    assert_eq!(wastage["user_name"], "Chidi");
}

#[tokio::test]
async fn a_count_stores_the_difference_and_adjusts_the_balance() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Drinks");
    let item_id = create_item(&app, &token, &category, "Coke", 15_000).await;
    add_stock(&app, &token, &item_id, 5).await;

    let counted = post(
        &app,
        "/stock/counts",
        &token,
        json!({
            "counts": [{ "item_id": item_id, "counted_qty": 6 }],
            "reason": "Evening count",
        }),
    )
    .await;
    assert_eq!(counted.status, StatusCode::OK, "{}", counted.body);
    assert_eq!(counted.body[0]["expected_qty"], 5);
    assert_eq!(counted.body[0]["counted_qty"], 6);
    assert_eq!(counted.body[0]["difference"], 1);

    let balances = get(&app, "/stock/balances", &token).await;
    let balance = balances
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["item_id"] == item_id)
        .unwrap();
    assert_eq!(balance["balance"], 6);

    let ledger = get(&app, &format!("/stock/ledger?item_id={item_id}"), &token).await;
    let diff = ledger
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["movement_type"] == "count_diff")
        .unwrap();
    assert_eq!(diff["quantity"], 1);

    let duplicate = post(
        &app,
        "/stock/counts",
        &token,
        json!({
            "counts": [
                { "item_id": item_id, "counted_qty": 6 },
                { "item_id": item_id, "counted_qty": 5 },
            ],
        }),
    )
    .await;
    assert_eq!(duplicate.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn stock_movements_need_a_manager() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Drinks");
    let item_id = create_item(&app, &token, &category, "Coke", 15_000).await;
    add_stock(&app, &token, &item_id, 5).await;

    let cashier_pin = create_worker(&app, &token, "Ada", "cashier").await;
    let cashier_token = login(&app, "cashier-terminal-3", &cashier_pin).await;

    for (uri, body) in [
        (
            "/stock/restock",
            json!({ "item_id": item_id, "quantity": 1 }),
        ),
        (
            "/stock/wastage",
            json!({ "item_id": item_id, "quantity": 1, "reason": "mistake" }),
        ),
        (
            "/stock/counts",
            json!({ "counts": [{ "item_id": item_id, "counted_qty": 4 }] }),
        ),
    ] {
        let reply = post(&app, uri, &cashier_token, body).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{uri}");
    }
}
