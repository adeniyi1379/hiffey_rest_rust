use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use pos_types::{LoginRequest, LoginResponse, UserSummary};

use crate::{
    audit::{self, AuditEntry},
    auth::{self, AuthUser},
    error::ApiError,
    AppState,
};

/// Worker PIN login. Returns a session token, the worker identity and the role.
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let terminal_id = auth::terminal_id(&headers);
    let pin = request.pin.trim().to_owned();

    if pin.len() != 6 || !pin.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(ApiError::unauthorized(
            "invalid_pin",
            "That PIN is not correct.",
        ));
    }

    if auth::is_locked_out(&state.pool, &terminal_id).await? {
        return Err(ApiError::locked(
            "Too many wrong PINs on this terminal. Wait a moment and try again.",
            auth::LOCKOUT_MINUTES as u64 * 60,
        ));
    }

    let matched = auth::authenticate(&state.pool, &pin).await?;

    match matched {
        Some(user) => {
            auth::record_attempt(&state.pool, &terminal_id, true).await?;
            auth::clear_failures(&state.pool, &terminal_id).await?;
            let (token, expires_at) =
                auth::create_session(&state.pool, &user.id, &terminal_id).await?;

            audit::record(
                &state.pool,
                AuditEntry {
                    user_id: Some(&user.id),
                    terminal_id: Some(&terminal_id),
                    action: "auth.login",
                    entity: Some("users"),
                    entity_id: Some(&user.id),
                    details: None,
                },
            )
            .await?;
            state.events.send(
                "session.started",
                serde_json::json!({ "user_id": &user.id, "name": &user.name }),
            );

            Ok(Json(LoginResponse {
                token,
                user,
                expires_at,
            }))
        }
        None => {
            auth::record_attempt(&state.pool, &terminal_id, false).await?;
            audit::record(
                &state.pool,
                AuditEntry {
                    user_id: None,
                    terminal_id: Some(&terminal_id),
                    action: "auth.login_failed",
                    entity: Some("users"),
                    entity_id: None,
                    details: None,
                },
            )
            .await?;
            Err(ApiError::unauthorized(
                "invalid_pin",
                "That PIN is not correct.",
            ))
        }
    }
}

/// Ends the current session.
pub async fn logout(State(state): State<AppState>, user: AuthUser) -> Result<StatusCode, ApiError> {
    auth::revoke_session(&state.pool, &user.token).await?;
    audit::record(
        &state.pool,
        AuditEntry {
            user_id: Some(&user.user.id),
            terminal_id: Some(&user.terminal_id),
            action: "auth.logout",
            entity: Some("sessions"),
            entity_id: None,
            details: None,
        },
    )
    .await?;
    state.events.send(
        "session.ended",
        serde_json::json!({ "user_id": user.user.id }),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Returns the worker behind the current session.
pub async fn me(user: AuthUser) -> Json<UserSummary> {
    Json(user.user)
}
