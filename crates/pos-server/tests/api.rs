use axum::{
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tower::ServiceExt;

const TERMINAL: &str = "test-terminal";

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
    body: Value,
}

async fn send(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    Reply {
        status,
        headers,
        body,
    }
}

async fn get(app: &Router, uri: &str, token: Option<&str>) -> Reply {
    send(app, request(Method::GET, uri, token, TERMINAL, None)).await
}

async fn post(app: &Router, uri: &str, token: Option<&str>, body: Value) -> Reply {
    send(app, request(Method::POST, uri, token, TERMINAL, Some(body))).await
}

async fn setup(app: &Router) -> (String, String) {
    let reply = post(
        app,
        "/setup",
        None,
        json!({
            "business_name": "Hiffey Restaurant",
            "owner_name": "Chidi",
            "business_address": "12 Market Road",
            "business_phone": "08030000000",
        }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let pin = reply.body["pin"].as_str().unwrap().to_owned();
    let token = reply.body["session"]["token"].as_str().unwrap().to_owned();
    (pin, token)
}

fn wrong_pin(pin: &str) -> String {
    let first = if pin.starts_with('0') { '1' } else { '0' };
    format!("{first}{}", &pin[1..])
}

#[tokio::test]
async fn health_is_public() {
    let (app, _pool) = test_app().await;
    let reply = get(&app, "/health", None).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["status"], "ok");
}

#[tokio::test]
async fn setup_runs_once_and_returns_owner_session() {
    let (app, _pool) = test_app().await;

    let before = get(&app, "/setup/status", None).await;
    assert_eq!(before.status, StatusCode::OK);
    assert_eq!(before.body["configured"], false);

    let (pin, token) = setup(&app).await;
    assert_eq!(pin.len(), 6);
    assert!(pin.chars().all(|ch| ch.is_ascii_digit()));

    let after = get(&app, "/setup/status", None).await;
    assert_eq!(after.body["configured"], true);
    assert_eq!(after.body["business_name"], "Hiffey Restaurant");

    let second = post(
        &app,
        "/setup",
        None,
        json!({ "business_name": "Other", "owner_name": "Other" }),
    )
    .await;
    assert_eq!(second.status, StatusCode::CONFLICT);

    let me = get(&app, "/auth/me", Some(&token)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["role"], "owner");
    assert_eq!(me.body["name"], "Chidi");
}

#[tokio::test]
async fn login_accepts_the_generated_pin_and_rejects_wrong_ones() {
    let (app, _pool) = test_app().await;
    let (pin, _token) = setup(&app).await;

    let wrong = post(&app, "/auth/login", None, json!({ "pin": wrong_pin(&pin) })).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.body["code"], "invalid_pin");

    let malformed = post(&app, "/auth/login", None, json!({ "pin": "12345" })).await;
    assert_eq!(malformed.status, StatusCode::UNAUTHORIZED);

    let good = post(&app, "/auth/login", None, json!({ "pin": pin })).await;
    assert_eq!(good.status, StatusCode::OK, "{}", good.body);
    assert_eq!(good.body["user"]["role"], "owner");
    assert!(good.body["token"].as_str().is_some_and(|t| !t.is_empty()));
}

#[tokio::test]
async fn five_wrong_pins_lock_the_terminal() {
    let (app, _pool) = test_app().await;
    let (pin, _token) = setup(&app).await;
    let wrong = wrong_pin(&pin);

    for _ in 0..5 {
        let reply = post(&app, "/auth/login", None, json!({ "pin": wrong })).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    }

    let locked = post(&app, "/auth/login", None, json!({ "pin": wrong })).await;
    assert_eq!(locked.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(locked.body["code"], "locked_out");
    assert_eq!(locked.headers.get("retry-after").unwrap(), "120");

    let still_locked = post(&app, "/auth/login", None, json!({ "pin": pin })).await;
    assert_eq!(still_locked.status, StatusCode::TOO_MANY_REQUESTS);

    let other_terminal = send(
        &app,
        request(
            Method::POST,
            "/auth/login",
            None,
            "other-terminal",
            Some(json!({ "pin": pin })),
        ),
    )
    .await;
    assert_eq!(other_terminal.status, StatusCode::OK);
}

#[tokio::test]
async fn requests_need_a_valid_session() {
    let (app, _pool) = test_app().await;
    setup(&app).await;

    let anonymous = get(&app, "/settings", None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(anonymous.body["code"], "missing_token");

    let bogus = get(&app, "/settings", Some("not-a-token")).await;
    assert_eq!(bogus.status, StatusCode::UNAUTHORIZED);
    assert_eq!(bogus.body["code"], "invalid_session");
}

#[tokio::test]
async fn logout_invalidates_the_token() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;

    let reply = post(&app, "/auth/logout", Some(&token), json!({})).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);

    let after = get(&app, "/auth/me", Some(&token)).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn workers_are_created_with_pins_and_roles_are_enforced() {
    let (app, _pool) = test_app().await;
    let (_owner_pin, owner_token) = setup(&app).await;

    let created = post(
        &app,
        "/admin/workers",
        Some(&owner_token),
        json!({ "name": "Ada", "role": "cashier" }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["worker"]["role"], "cashier");
    let cashier_pin = created.body["pin"].as_str().unwrap().to_owned();
    let cashier_id = created.body["worker"]["id"].as_str().unwrap().to_owned();

    let cashier = post(&app, "/auth/login", None, json!({ "pin": cashier_pin })).await;
    assert_eq!(cashier.status, StatusCode::OK);
    let cashier_token = cashier.body["token"].as_str().unwrap().to_owned();

    let list = get(&app, "/admin/workers", Some(&owner_token)).await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.body.as_array().unwrap().len(), 2);

    let denied = get(&app, "/admin/workers", Some(&cashier_token)).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);

    let settings = get(&app, "/settings", Some(&cashier_token)).await;
    assert_eq!(settings.status, StatusCode::OK);

    let cannot_edit = send(
        &app,
        request(
            Method::PUT,
            "/settings",
            Some(&cashier_token),
            TERMINAL,
            Some(json!({
                "business_name": "Hacked",
                "business_address": "",
                "business_phone": "",
                "receipt_footer": "",
                "packaging_fee_kobo": 0,
                "low_stock_default": 5,
            })),
        ),
    )
    .await;
    assert_eq!(cannot_edit.status, StatusCode::FORBIDDEN);

    let duplicate = post(
        &app,
        "/admin/workers",
        Some(&owner_token),
        json!({ "name": "ada", "role": "cashier" }),
    )
    .await;
    assert_eq!(duplicate.status, StatusCode::CONFLICT);

    let deactivated = post(
        &app,
        &format!("/admin/workers/{cashier_id}/deactivate"),
        Some(&owner_token),
        json!({}),
    )
    .await;
    assert_eq!(deactivated.status, StatusCode::NO_CONTENT);

    let after_deactivation = get(&app, "/auth/me", Some(&cashier_token)).await;
    assert_eq!(after_deactivation.status, StatusCode::UNAUTHORIZED);

    let login_again = post(&app, "/auth/login", None, json!({ "pin": cashier_pin })).await;
    assert_eq!(login_again.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn pin_reset_replaces_the_pin_and_signs_the_worker_out() {
    let (app, _pool) = test_app().await;
    let (_owner_pin, owner_token) = setup(&app).await;

    let created = post(
        &app,
        "/admin/workers",
        Some(&owner_token),
        json!({ "name": "Bola", "role": "manager" }),
    )
    .await;
    let old_pin = created.body["pin"].as_str().unwrap().to_owned();
    let worker_id = created.body["worker"]["id"].as_str().unwrap().to_owned();

    let login = post(&app, "/auth/login", None, json!({ "pin": old_pin })).await;
    let worker_token = login.body["token"].as_str().unwrap().to_owned();

    let reset = post(
        &app,
        &format!("/admin/workers/{worker_id}/reset-pin"),
        Some(&owner_token),
        json!({}),
    )
    .await;
    assert_eq!(reset.status, StatusCode::OK);
    let new_pin = reset.body["pin"].as_str().unwrap().to_owned();
    assert_ne!(new_pin, old_pin);

    let old_session = get(&app, "/auth/me", Some(&worker_token)).await;
    assert_eq!(old_session.status, StatusCode::UNAUTHORIZED);

    let old_login = post(&app, "/auth/login", None, json!({ "pin": old_pin })).await;
    assert_eq!(old_login.status, StatusCode::UNAUTHORIZED);

    let new_login = post(&app, "/auth/login", None, json!({ "pin": new_pin })).await;
    assert_eq!(new_login.status, StatusCode::OK);
    assert_eq!(new_login.body["user"]["role"], "manager");
}

#[tokio::test]
async fn settings_update_round_trips() {
    let (app, _pool) = test_app().await;
    let (_pin, token) = setup(&app).await;

    let reply = send(
        &app,
        request(
            Method::PUT,
            "/settings",
            Some(&token),
            TERMINAL,
            Some(json!({
                "business_name": "Hiffey Restaurant",
                "business_address": "12 Market Road",
                "business_phone": "08030000000",
                "receipt_footer": "Thank you",
                "packaging_fee_kobo": 20000,
                "low_stock_default": 3,
            })),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);

    let loaded = get(&app, "/settings", Some(&token)).await;
    assert_eq!(loaded.body["packaging_fee_kobo"], 20000);
    assert_eq!(loaded.body["receipt_footer"], "Thank you");
    assert_eq!(loaded.body["low_stock_default"], 3);

    let negative = send(
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
                "packaging_fee_kobo": -1,
                "low_stock_default": 3,
            })),
        ),
    )
    .await;
    assert_eq!(negative.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sync_is_disabled_but_the_outbox_already_counts_changes() {
    let (app, pool) = test_app().await;
    let (_pin, token) = setup(&app).await;

    let status = get(&app, "/sync/status", Some(&token)).await;
    assert_eq!(status.status, StatusCode::OK);
    assert_eq!(status.body["enabled"], false);
    assert_eq!(status.body["pending_count"], 7);

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_outbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 7);

    let audit_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_log WHERE action = 'setup.completed'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audit_rows, 1);
}
