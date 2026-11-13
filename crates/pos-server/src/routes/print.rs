//! Receipts, duplicate reprints and serving slips.
//!
//! A print request renders the paper text immediately and stores it with the
//! job, so a retry after a jam matches what the customer was charged. The
//! printing agent on this terminal polls the queued jobs and reports back.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use pos_print::PaperWidth;
use pos_types::{
    CreatePrintJobRequest, OrderStatus, PrintJobDetail, PrintJobDto, PrintJobResultRequest,
    PrintJobStatus, PrintJobType,
};
use serde::Deserialize;
use sqlx::{sqlite::SqliteRow, Row};
use uuid::Uuid;

use crate::{
    audit::{self, AuditEntry},
    auth::AuthUser,
    error::ApiError,
    receipt, sales, AppState,
};

const JOB_COLUMNS: &str = "p.id, p.order_id, o.order_no, p.job_type, p.status, p.attempts,
        p.last_error, p.paper_width, p.user_id, u.name AS user_name, p.created_at, p.updated_at";

const JOB_FROM: &str = " FROM print_jobs p
     JOIN orders o ON o.id = p.order_id
     LEFT JOIN users u ON u.id = p.user_id";

#[derive(Debug, Deserialize)]
pub struct JobListQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

fn parse_job_type(value: &str) -> Result<PrintJobType, ApiError> {
    value
        .parse::<PrintJobType>()
        .map_err(|()| ApiError::internal(format!("unknown print job type in database: {value}")))
}

fn parse_status(value: &str) -> Result<PrintJobStatus, ApiError> {
    value
        .parse::<PrintJobStatus>()
        .map_err(|()| ApiError::internal(format!("unknown print job status in database: {value}")))
}

fn job_dto(row: &SqliteRow) -> Result<PrintJobDto, ApiError> {
    let job_type: String = row.get("job_type");
    let status: String = row.get("status");
    Ok(PrintJobDto {
        id: row.get("id"),
        order_id: row.get("order_id"),
        order_no: row.get("order_no"),
        job_type: parse_job_type(&job_type)?,
        status: parse_status(&status)?,
        attempts: row.get("attempts"),
        last_error: row.get("last_error"),
        paper_width: row.get("paper_width"),
        requested_by: row.get("user_id"),
        requested_by_name: row.get("user_name"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

async fn load_job(state: &AppState, id: &str) -> Result<(PrintJobDto, String), ApiError> {
    let row = sqlx::query(&format!(
        "SELECT {JOB_COLUMNS}, p.payload {JOB_FROM} WHERE p.id = ?"
    ))
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Print job not found."))?;
    let job = job_dto(&row)?;
    Ok((job, row.get("payload")))
}

pub async fn create(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(order_id): Path<String>,
    Json(request): Json<CreatePrintJobRequest>,
) -> Result<(StatusCode, Json<PrintJobDetail>), ApiError> {
    let order = sales::load_order(&state.pool, &order_id).await?;

    match request.job_type {
        PrintJobType::Receipt | PrintJobType::Duplicate if order.status != OrderStatus::Paid => {
            return Err(ApiError::conflict(
                "order_not_paid",
                "A receipt is only available after payment.",
            ));
        }
        PrintJobType::Slip if order.status == OrderStatus::Voided => {
            return Err(ApiError::conflict("order_closed", "That order was voided."));
        }
        _ => {}
    }

    let business = receipt::business(&state.pool).await?;
    let width = PaperWidth::parse(request.paper_width.as_deref());
    let payload = match request.job_type {
        PrintJobType::Slip => receipt::build_slip(&order, &business, width),
        PrintJobType::Receipt => receipt::build_receipt(&order, &business, width, false),
        PrintJobType::Duplicate => receipt::build_receipt(&order, &business, width, true),
    };

    let id = Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO print_jobs (id, order_id, job_type, status, attempts, paper_width, payload,
             user_id, terminal_id, created_at, updated_at)
         VALUES (?, ?, ?, 'queued', 0, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&order_id)
    .bind(request.job_type.as_str())
    .bind(width.as_str())
    .bind(&payload)
    .bind(&actor.user.id)
    .bind(&actor.terminal_id)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action: "print.requested",
            entity: Some("print_jobs"),
            entity_id: Some(&id),
            details: Some(request.job_type.as_str()),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "print_jobs",
        &id,
        "create",
        &serde_json::json!({
            "id": &id,
            "order_id": &order_id,
            "job_type": request.job_type.as_str(),
            "status": "queued",
        }),
    )
    .await?;
    tx.commit().await?;

    state.events.send(
        "print.jobs",
        serde_json::json!({ "id": &id, "order_id": &order_id, "status": "queued" }),
    );

    let job = PrintJobDto {
        id,
        order_id,
        order_no: order.order_no,
        job_type: request.job_type,
        status: PrintJobStatus::Queued,
        attempts: 0,
        last_error: None,
        paper_width: width.as_str().to_owned(),
        requested_by: Some(actor.user.id),
        requested_by_name: Some(actor.user.name),
        created_at: now.clone(),
        updated_at: now,
    };
    Ok((StatusCode::CREATED, Json(PrintJobDetail { job, payload })))
}

pub async fn list(
    State(state): State<AppState>,
    _actor: AuthUser,
    Query(query): Query<JobListQuery>,
) -> Result<Json<Vec<PrintJobDto>>, ApiError> {
    let status = match query.status.as_deref() {
        Some(value) => value
            .parse::<PrintJobStatus>()
            .map_err(|()| ApiError::bad_request("status must be queued, printed or failed."))?,
        None => PrintJobStatus::Queued,
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let rows = sqlx::query(&format!(
        "SELECT {JOB_COLUMNS} {JOB_FROM} WHERE p.status = ? ORDER BY p.created_at LIMIT ?"
    ))
    .bind(status.as_str())
    .bind(limit)
    .fetch_all(&state.pool)
    .await?;
    let jobs = rows.iter().map(job_dto).collect::<Result<Vec<_>, _>>()?;
    Ok(Json(jobs))
}

pub async fn get_job(
    State(state): State<AppState>,
    _actor: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<PrintJobDetail>, ApiError> {
    let (job, payload) = load_job(&state, &id).await?;
    Ok(Json(PrintJobDetail { job, payload }))
}

pub async fn result(
    State(state): State<AppState>,
    actor: AuthUser,
    Path(id): Path<String>,
    Json(request): Json<PrintJobResultRequest>,
) -> Result<Json<PrintJobDto>, ApiError> {
    let (job, _) = load_job(&state, &id).await?;
    if job.status == PrintJobStatus::Printed {
        return Err(ApiError::conflict(
            "already_printed",
            "That print job was already completed.",
        ));
    }

    let status = if request.success {
        PrintJobStatus::Printed
    } else {
        PrintJobStatus::Failed
    };
    let last_error = request
        .error
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(200).collect::<String>());
    let attempts = job.attempts + 1;
    let now = Utc::now().to_rfc3339();

    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "UPDATE print_jobs SET status = ?, attempts = ?, last_error = ?, updated_at = ? WHERE id = ?",
    )
    .bind(status.as_str())
    .bind(attempts)
    .bind(&last_error)
    .bind(&now)
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    let action = if request.success {
        "print.printed"
    } else {
        "print.failed"
    };
    audit::record(
        &mut *tx,
        AuditEntry {
            user_id: Some(&actor.user.id),
            terminal_id: Some(&actor.terminal_id),
            action,
            entity: Some("print_jobs"),
            entity_id: Some(&id),
            details: last_error.as_deref(),
        },
    )
    .await?;
    audit::enqueue(
        &mut *tx,
        "print_jobs",
        &id,
        "update",
        &serde_json::json!({
            "id": &id,
            "status": status.as_str(),
            "attempts": attempts,
            "last_error": &last_error,
        }),
    )
    .await?;
    tx.commit().await?;

    state.events.send(
        "print.jobs",
        serde_json::json!({
            "id": &id,
            "order_id": &job.order_id,
            "status": status.as_str(),
        }),
    );

    Ok(Json(PrintJobDto {
        status,
        attempts,
        last_error,
        updated_at: now,
        ..job
    }))
}
