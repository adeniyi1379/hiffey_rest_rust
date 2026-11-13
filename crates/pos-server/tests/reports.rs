use axum::{
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode},
    Router,
};
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tower::ServiceExt;

const TERMINAL: &str = "reports-terminal";

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
    headers: HeaderMap,
    text: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap()
    }
}

async fn send(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    Reply {
        status,
        headers,
        text: String::from_utf8(bytes.to_vec()).unwrap(),
    }
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
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);
    let body = reply.json();
    (
        body["pin"].as_str().unwrap().to_owned(),
        body["session"]["token"].as_str().unwrap().to_owned(),
    )
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

async fn create_item(app: &Router, token: &str, category: &str, name: &str, price: i64) -> String {
    let reply = post(
        app,
        "/admin/items",
        token,
        json!({
            "name": name,
            "category_id": category,
            "unit_type": "spoon",
            "price_per_unit_kobo": price,
            "low_stock_level": 2,
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.json()["items"]
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
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
}

fn wat_date() -> String {
    (Utc::now() + Duration::hours(1))
        .format("%Y-%m-%d")
        .to_string()
}

#[tokio::test]
async fn daily_report_and_csv_exports_match_the_day() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;

    let menu = get(&app, "/menu", &token).await.json();
    let category = find_category(&menu, "Rice");
    let item_id = create_item(&app, &token, &category, "Jollof Rice", 35_000).await;
    add_stock(&app, &token, &item_id, 20).await;

    let settings = send(
        &app,
        request(
            Method::PUT,
            "/settings",
            Some(&token),
            TERMINAL,
            Some(json!({
                "business_name": "Hiffey Restaurant",
                "business_address": "",
                "business_phone": "",
                "receipt_footer": "",
                "packaging_fee_kobo": 20_000,
                "low_stock_default": 5,
            })),
        ),
    )
    .await;
    assert_eq!(settings.status, StatusCode::OK, "{}", settings.text);

    let order = post(
        &app,
        "/orders",
        &token,
        json!({
            "order_type": "takeaway",
            "lines": [{ "kind": "item", "item_id": item_id, "quantity": 3 }],
        }),
    )
    .await;
    assert_eq!(order.status, StatusCode::CREATED, "{}", order.text);
    let order = order.json();
    let order_id = order["id"].as_str().unwrap().to_owned();
    let order_no = order["order_no"].as_str().unwrap().to_owned();

    let paid = post(
        &app,
        &format!("/orders/{order_id}/pay"),
        &token,
        json!({ "payments": [{ "method": "cash", "amount_kobo": 130_000 }] }),
    )
    .await;
    assert_eq!(paid.status, StatusCode::OK, "{}", paid.text);

    let date = wat_date();
    let report = get(&app, &format!("/reports/daily?date={date}"), &token).await;
    assert_eq!(report.status, StatusCode::OK, "{}", report.text);
    let report = report.json();
    assert_eq!(report["date"], date);
    assert_eq!(report["paid_orders"], 1);
    assert_eq!(report["takeaway_orders"], 1);
    assert_eq!(report["dine_in_orders"], 0);
    assert_eq!(report["held_orders"], 0);
    assert_eq!(report["voided_orders"], 0);
    assert_eq!(report["subtotal_kobo"], 105_000);
    assert_eq!(report["packaging_kobo"], 20_000);
    assert_eq!(report["total_kobo"], 125_000);
    assert_eq!(report["average_kobo"], 125_000);
    assert_eq!(report["tendered_kobo"], 130_000);
    assert_eq!(report["change_kobo"], 5_000);
    assert_eq!(report["payments"][0]["method"], "cash");
    assert_eq!(report["payments"][0]["orders"], 1);
    assert_eq!(report["payments"][0]["amount_kobo"], 130_000);
    assert_eq!(report["payments"][1]["amount_kobo"], 0);
    assert_eq!(report["items"][0]["item_name"], "Jollof Rice");
    assert_eq!(report["items"][0]["quantity"], 3);
    assert_eq!(report["items"][0]["revenue_kobo"], 105_000);

    let sales = get(&app, &format!("/reports/sales.csv?date={date}"), &token).await;
    assert_eq!(sales.status, StatusCode::OK, "{}", sales.text);
    assert!(sales
        .headers
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/csv"));
    assert!(sales.text.starts_with("order_no,trading_day,status"));
    assert!(sales.text.contains(&order_no));
    assert!(sales.text.contains("1250.00"));
    assert!(sales.text.contains("1300.00"));

    let stock = get(&app, &format!("/reports/stock.csv?date={date}"), &token).await;
    assert_eq!(stock.status, StatusCode::OK, "{}", stock.text);
    assert!(stock.text.starts_with("created_at,item,movement"));
    assert!(stock.text.contains("Jollof Rice,sale,-3"));
}

#[tokio::test]
async fn reports_reject_bad_dates_and_cashiers() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;

    let bad_date = get(&app, "/reports/daily?date=2026-13-01", &token).await;
    assert_eq!(
        bad_date.status,
        StatusCode::BAD_REQUEST,
        "{}",
        bad_date.text
    );

    let worker = post(
        &app,
        "/admin/workers",
        &token,
        json!({ "name": "Ada", "role": "cashier" }),
    )
    .await;
    assert_eq!(worker.status, StatusCode::CREATED, "{}", worker.text);
    let pin = worker.json()["pin"].as_str().unwrap().to_owned();
    let login = send(
        &app,
        request(
            Method::POST,
            "/auth/login",
            None,
            "cashier-terminal",
            Some(json!({ "pin": pin })),
        ),
    )
    .await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.text);
    let cashier = login.json()["token"].as_str().unwrap().to_owned();

    for uri in ["/reports/daily", "/reports/sales.csv", "/reports/stock.csv"] {
        let reply = get(&app, uri, &cashier).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
    }
}
