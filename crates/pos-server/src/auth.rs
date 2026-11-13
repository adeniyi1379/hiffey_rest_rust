use std::fmt::Write as _;

use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, HeaderMap},
};
use chrono::{DateTime, Duration, Utc};
use pos_types::{Role, UserSummary};
use rand::Rng;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::{error::ApiError, AppState};

/// A session ends after this many minutes without an authenticated request.
pub const SESSION_IDLE_MINUTES: i64 = 30;

/// Wrong PINs allowed per terminal before a temporary lockout.
pub const MAX_FAILED_ATTEMPTS: i64 = 5;

/// How long a terminal stays locked after too many wrong PINs.
pub const LOCKOUT_MINUTES: i64 = 2;

/// The authenticated worker behind a request.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: UserSummary,
    pub token: String,
    pub terminal_id: String,
}

/// Generates a 6-digit PIN. PINs are never chosen by hand.
pub fn generate_pin() -> String {
    let value: u32 = rand::rng().random_range(0..1_000_000);
    format!("{value:06}")
}

/// Hashes a PIN with argon2 for storage.
pub fn hash_pin(pin: &str) -> Result<String, ApiError> {
    let salt_bytes: [u8; 16] = rand::rng().random();
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|error| ApiError::internal(format!("salt: {error}")))?;
    Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| ApiError::internal(format!("hash pin: {error}")))
}

fn verify_pin(pin: &str, pin_hash: &str) -> bool {
    PasswordHash::new(pin_hash).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(pin.as_bytes(), &parsed)
            .is_ok()
    })
}

/// True when the PIN already belongs to any worker, active or not.
pub async fn pin_in_use(pool: &SqlitePool, pin: &str) -> Result<bool, ApiError> {
    let rows = sqlx::query("SELECT pin_hash FROM users")
        .fetch_all(pool)
        .await?;
    let hashes: Vec<String> = rows.iter().map(|row| row.get("pin_hash")).collect();
    let pin = pin.to_owned();
    let found =
        tokio::task::spawn_blocking(move || hashes.iter().any(|hash| verify_pin(&pin, hash)))
            .await
            .map_err(|error| ApiError::internal(format!("pin check: {error}")))?;
    Ok(found)
}

/// Generates a PIN that is not already in use.
pub async fn generate_unique_pin(pool: &SqlitePool) -> Result<String, ApiError> {
    for _ in 0..100 {
        let pin = generate_pin();
        if !pin_in_use(pool, &pin).await? {
            return Ok(pin);
        }
    }
    Err(ApiError::internal("could not allocate a unique PIN"))
}

/// Checks a PIN against every active worker and returns the matching identity.
pub async fn authenticate(pool: &SqlitePool, pin: &str) -> Result<Option<UserSummary>, ApiError> {
    let rows = sqlx::query("SELECT id, name, role, pin_hash FROM users WHERE active = 1")
        .fetch_all(pool)
        .await?;

    let candidates: Vec<(String, String, String, String)> = rows
        .iter()
        .map(|row| {
            (
                row.get("id"),
                row.get("name"),
                row.get("role"),
                row.get("pin_hash"),
            )
        })
        .collect();
    let pin = pin.to_owned();

    let matched = tokio::task::spawn_blocking(move || {
        candidates
            .into_iter()
            .find(|(_, _, _, hash)| verify_pin(&pin, hash))
    })
    .await
    .map_err(|error| ApiError::internal(format!("pin login: {error}")))?;

    Ok(matched.and_then(|(id, name, role, _)| {
        role.parse::<Role>()
            .ok()
            .map(|role| UserSummary { id, name, role })
    }))
}

fn random_token() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    let mut token = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(token, "{byte:02x}");
    }
    token
}

/// Creates a session and returns its token and expiry.
pub async fn create_session(
    pool: &SqlitePool,
    user_id: &str,
    terminal_id: &str,
) -> Result<(String, String), ApiError> {
    let token = random_token();
    let now = Utc::now();
    let expires_at = now + Duration::minutes(SESSION_IDLE_MINUTES);
    sqlx::query(
        "INSERT INTO sessions (token, user_id, terminal_id, created_at, last_seen_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&token)
    .bind(user_id)
    .bind(terminal_id)
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(expires_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok((token, expires_at.to_rfc3339()))
}

/// Loads a session, rejects it when expired, and slides the idle timeout.
pub async fn load_session(
    pool: &SqlitePool,
    token: &str,
    terminal_id: &str,
) -> Result<AuthUser, ApiError> {
    let row = sqlx::query(
        "SELECT s.user_id, s.expires_at, u.name, u.role, u.active
         FROM sessions s
         JOIN users u ON u.id = s.user_id
         WHERE s.token = ?",
    )
    .bind(token)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::unauthorized("invalid_session", "Please sign in again."))?;

    let active: i64 = row.get("active");
    if active == 0 {
        return Err(ApiError::unauthorized(
            "worker_inactive",
            "This worker account is no longer active.",
        ));
    }

    let expires_at: String = row.get("expires_at");
    let expired = DateTime::parse_from_rfc3339(&expires_at)
        .map(|expiry| expiry <= Utc::now())
        .unwrap_or(true);
    if expired {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(pool)
            .await?;
        return Err(ApiError::unauthorized(
            "session_expired",
            "Your session expired. Please sign in again.",
        ));
    }

    let user_id: String = row.get("user_id");
    let name: String = row.get("name");
    let role: String = row.get("role");
    let role = role
        .parse::<Role>()
        .map_err(|()| ApiError::internal(format!("unknown role in database: {role}")))?;

    let now = Utc::now();
    sqlx::query("UPDATE sessions SET last_seen_at = ?, expires_at = ? WHERE token = ?")
        .bind(now.to_rfc3339())
        .bind((now + Duration::minutes(SESSION_IDLE_MINUTES)).to_rfc3339())
        .bind(token)
        .execute(pool)
        .await?;

    Ok(AuthUser {
        user: UserSummary {
            id: user_id,
            name,
            role,
        },
        token: token.to_owned(),
        terminal_id: terminal_id.to_owned(),
    })
}

pub async fn revoke_session(pool: &SqlitePool, token: &str) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM sessions WHERE token = ?")
        .bind(token)
        .execute(pool)
        .await?;
    Ok(())
}

/// Removes every session belonging to a worker (used on PIN reset and deactivation).
pub async fn revoke_user_sessions(pool: &SqlitePool, user_id: &str) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// A terminal is locked when it has too many failed PIN attempts in the lockout window.
pub async fn is_locked_out(pool: &SqlitePool, terminal_id: &str) -> Result<bool, ApiError> {
    let cutoff = (Utc::now() - Duration::minutes(LOCKOUT_MINUTES)).to_rfc3339();
    let failures: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM login_attempts
         WHERE terminal_id = ? AND success = 0 AND at > ?",
    )
    .bind(terminal_id)
    .bind(cutoff)
    .fetch_one(pool)
    .await?;
    Ok(failures >= MAX_FAILED_ATTEMPTS)
}

pub async fn record_attempt(
    pool: &SqlitePool,
    terminal_id: &str,
    success: bool,
) -> Result<(), ApiError> {
    let cutoff = (Utc::now() - Duration::days(1)).to_rfc3339();
    sqlx::query("DELETE FROM login_attempts WHERE at < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO login_attempts (id, terminal_id, success, at) VALUES (?, ?, ?, ?)")
        .bind(Uuid::now_v7().to_string())
        .bind(terminal_id)
        .bind(i64::from(success))
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear_failures(pool: &SqlitePool, terminal_id: &str) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM login_attempts WHERE terminal_id = ? AND success = 0")
        .bind(terminal_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// The terminal id sent by the UI, sanitised so it is safe to store and log.
#[must_use]
pub fn terminal_id(headers: &HeaderMap) -> String {
    headers
        .get("x-terminal-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
                .take(64)
                .collect::<String>()
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Rejects the request unless the worker's role is one of `allowed`.
pub fn require(actor: &AuthUser, allowed: &[Role]) -> Result<(), ApiError> {
    if allowed.contains(&actor.user.role) {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "Your role cannot perform this action. Ask a manager.",
        ))
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| ApiError::unauthorized("missing_token", "Sign in to continue."))?;

        load_session(&state.pool, token, &terminal_id(&parts.headers)).await
    }
}
