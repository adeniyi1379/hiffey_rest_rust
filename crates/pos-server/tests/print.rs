use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tower::ServiceExt;

const TERMINAL: &str = "print-terminal";

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

async fn create_count_item(
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
            "unit_type": "count",
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

async fn paid_order(app: &Router, token: &str, item_id: &str) -> Value {
    let order = post(
        app,
        "/orders",
        token,
        json!({
            "order_type": "dine_in",
            "table_no": "4",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 2 }],
        }),
    )
    .await;
    assert_eq!(order.status, StatusCode::CREATED, "{}", order.body);
    let id = order.body["id"].as_str().unwrap().to_owned();
    let paid = post(
        app,
        &format!("/orders/{id}/pay"),
        token,
        json!({ "payments": [{ "method": "cash", "amount_kobo": 100_000 }] }),
    )
    .await;
    assert_eq!(paid.status, StatusCode::OK, "{}", paid.body);
    paid.body["order"].clone()
}

#[tokio::test]
async fn receipt_is_queued_only_after_payment() {
    let (app, pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 10).await;

    let order = post(
        &app,
        "/orders",
        &token,
        json!({
            "order_type": "dine_in",
            "table_no": "4",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 2 }],
        }),
    )
    .await;
    let id = order.body["id"].as_str().unwrap().to_owned();

    let early = post(
        &app,
        &format!("/orders/{id}/print"),
        &token,
        json!({ "job_type": "receipt" }),
    )
    .await;
    assert_eq!(early.status, StatusCode::CONFLICT);
    assert_eq!(early.body["code"], "order_not_paid");

    let paid = post(
        &app,
        &format!("/orders/{id}/pay"),
        &token,
        json!({ "payments": [{ "method": "cash", "amount_kobo": 100_000 }] }),
    )
    .await;
    assert_eq!(paid.status, StatusCode::OK, "{}", paid.body);

    let receipt = post(
        &app,
        &format!("/orders/{id}/print"),
        &token,
        json!({ "job_type": "receipt", "paper_width": "mm58" }),
    )
    .await;
    assert_eq!(receipt.status, StatusCode::CREATED, "{}", receipt.body);
    assert_eq!(receipt.body["job"]["status"], "queued");
    assert_eq!(
        receipt.body["job"]["order_no"],
        paid.body["order"]["order_no"]
    );
    assert_eq!(receipt.body["job"]["paper_width"], "mm58");
    let payload = receipt.body["payload"].as_str().unwrap();
    for expected in [
        "Hiffey Restaurant",
        "12 Market Road",
        "HIF-",
        "Dine-in",
        "Table 4",
        "Cashier: Chidi",
        "Jollof Rice",
        "TOTAL",
        "Cash",
        "Change",
    ] {
        assert!(
            payload.contains(expected),
            "missing {expected} in\n{payload}"
        );
    }
    assert!(!payload.contains("DUPLICATE"));
    for line in payload.lines() {
        assert!(line.chars().count() <= 32, "line too wide: {line}");
    }

    let job_id = receipt.body["job"]["id"].as_str().unwrap().to_owned();
    let queued = get(&app, "/print/jobs", &token).await;
    assert_eq!(queued.status, StatusCode::OK);
    assert_eq!(queued.body.as_array().unwrap().len(), 1);

    let detail = get(&app, &format!("/print/jobs/{job_id}"), &token).await;
    assert_eq!(detail.status, StatusCode::OK);
    assert_eq!(detail.body["payload"], payload);

    let printed = post(
        &app,
        &format!("/print/jobs/{job_id}/result"),
        &token,
        json!({ "success": true }),
    )
    .await;
    assert_eq!(printed.status, StatusCode::OK, "{}", printed.body);
    assert_eq!(printed.body["status"], "printed");
    assert_eq!(printed.body["attempts"], 1);

    let again = post(
        &app,
        &format!("/print/jobs/{job_id}/result"),
        &token,
        json!({ "success": true }),
    )
    .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.body["code"], "already_printed");

    let requested: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = 'print.requested'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let printed_logs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = 'print.printed'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(requested, 1);
    assert_eq!(printed_logs, 1);
}

#[tokio::test]
async fn duplicates_are_labelled_and_slips_carry_no_prices() {
    let (app, pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let jollof = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    let fried = create_count_item(&app, &token, &category, "Fried Rice", 40_000).await;
    add_stock(&app, &token, &jollof, 10).await;
    add_stock(&app, &token, &fried, 10).await;

    let order = paid_order(&app, &token, &jollof).await;
    let order_id = order["id"].as_str().unwrap().to_owned();

    let duplicate = post(
        &app,
        &format!("/orders/{order_id}/print"),
        &token,
        json!({ "job_type": "duplicate", "paper_width": "mm80" }),
    )
    .await;
    assert_eq!(duplicate.status, StatusCode::CREATED, "{}", duplicate.body);
    let payload = duplicate.body["payload"].as_str().unwrap();
    assert!(payload.contains("*** DUPLICATE ***"));
    for line in payload.lines() {
        assert!(line.chars().count() <= 48, "line too wide: {line}");
    }

    let pack = post(
        &app,
        "/admin/packs",
        &token,
        json!({
            "name": "Standard Pack",
            "pricing_mode": "calculated",
            "components": [
                { "item_id": jollof, "quantity": 2 },
                { "item_id": fried, "quantity": 1 },
            ],
        }),
    )
    .await;
    assert_eq!(pack.status, StatusCode::OK, "{}", pack.body);
    let pack_id = pack.body["packs"][0]["id"].as_str().unwrap().to_owned();
    add_stock(&app, &token, &jollof, 10).await;
    add_stock(&app, &token, &fried, 10).await;

    let pack_order = post(
        &app,
        "/orders",
        &token,
        json!({
            "order_type": "takeaway",
            "lines": [{ "kind": "pack", "pack_id": pack_id, "quantity": 1 }],
        }),
    )
    .await;
    assert_eq!(
        pack_order.status,
        StatusCode::CREATED,
        "{}",
        pack_order.body
    );
    let pack_order_id = pack_order.body["id"].as_str().unwrap().to_owned();

    let slip = post(
        &app,
        &format!("/orders/{pack_order_id}/print"),
        &token,
        json!({ "job_type": "slip" }),
    )
    .await;
    assert_eq!(slip.status, StatusCode::CREATED, "{}", slip.body);
    let payload = slip.body["payload"].as_str().unwrap();
    assert!(payload.contains("SERVING SLIP"));
    assert!(payload.contains("1 x Standard Pack"));
    assert!(payload.contains("2 spoons Jollof Rice"));
    assert!(payload.contains("1 piece Fried Rice"));
    assert!(!payload.contains('\u{20a6}'));

    let reprints: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_log WHERE action = 'print.requested' AND details = 'duplicate'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reprints, 1);
}

#[tokio::test]
async fn failed_jobs_are_kept_for_retry() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 10).await;

    let order = paid_order(&app, &token, &item_id).await;
    let order_id = order["id"].as_str().unwrap().to_owned();
    let job = post(
        &app,
        &format!("/orders/{order_id}/print"),
        &token,
        json!({ "job_type": "receipt" }),
    )
    .await;
    let job_id = job.body["job"]["id"].as_str().unwrap().to_owned();

    let failed = post(
        &app,
        &format!("/print/jobs/{job_id}/result"),
        &token,
        json!({ "success": false, "error": "Printer out of paper" }),
    )
    .await;
    assert_eq!(failed.status, StatusCode::OK, "{}", failed.body);
    assert_eq!(failed.body["status"], "failed");
    assert_eq!(failed.body["last_error"], "Printer out of paper");
    assert_eq!(failed.body["attempts"], 1);

    let kept = get(&app, &format!("/print/jobs/{job_id}"), &token).await;
    assert_eq!(kept.body["job"]["status"], "failed");
    assert_eq!(kept.body["payload"], job.body["payload"]);

    let failed_list = get(&app, "/print/jobs?status=failed", &token).await;
    assert_eq!(failed_list.body.as_array().unwrap().len(), 1);
    assert_eq!(
        get(&app, "/print/jobs", &token)
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let retried = post(
        &app,
        &format!("/print/jobs/{job_id}/result"),
        &token,
        json!({ "success": true }),
    )
    .await;
    assert_eq!(retried.status, StatusCode::OK, "{}", retried.body);
    assert_eq!(retried.body["status"], "printed");
    assert_eq!(retried.body["attempts"], 2);

    let bad_status = get(&app, "/print/jobs?status=lost", &token).await;
    assert_eq!(bad_status.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn cashiers_can_print_but_voided_orders_have_no_receipt() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;
    let menu = get(&app, "/menu", &token).await;
    let category = find_category(&menu.body, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 10).await;
    let cashier_pin = create_worker(&app, &token, "Amaka", "cashier").await;
    let cashier = login(&app, "print-terminal", &cashier_pin).await;

    let order = paid_order(&app, &cashier, &item_id).await;
    let order_id = order["id"].as_str().unwrap().to_owned();

    let receipt = post(
        &app,
        &format!("/orders/{order_id}/print"),
        &cashier,
        json!({ "job_type": "receipt" }),
    )
    .await;
    assert_eq!(receipt.status, StatusCode::CREATED, "{}", receipt.body);
    assert_eq!(receipt.body["job"]["requested_by_name"], "Amaka");

    let held = post(
        &app,
        "/orders",
        &cashier,
        json!({
            "order_type": "dine_in",
            "table_no": "2",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 1 }],
        }),
    )
    .await;
    let held_id = held.body["id"].as_str().unwrap().to_owned();
    let voided = post(
        &app,
        &format!("/orders/{held_id}/void"),
        &cashier,
        json!({}),
    )
    .await;
    assert_eq!(voided.status, StatusCode::OK, "{}", voided.body);

    let slip = post(
        &app,
        &format!("/orders/{held_id}/print"),
        &cashier,
        json!({ "job_type": "slip" }),
    )
    .await;
    assert_eq!(slip.status, StatusCode::CONFLICT);
    assert_eq!(slip.body["code"], "order_closed");
}
