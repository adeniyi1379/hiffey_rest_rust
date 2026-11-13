//! Typed HTTP client for the LAN server.

use gloo_net::http::{Method, Request, Response};
use pos_types::{
    ApiErrorBody, CreateCategoryRequest, CreateItemRequest, CreateOrderRequest, CreatePackRequest,
    CreatePrintJobRequest, CreateWorkerRequest, CreatedWorker, DailyReport, LoginResponse,
    MenuResponse, OrderDto, PayOrderRequest, PayOrderResponse, PinResetResponse, PrintJobDetail,
    RecordBatchRequest, RestockRequest, SettingsDto, SetupRequest, SetupResponse, SetupStatus,
    StockBalanceDto, StockCountRequest, StockCountResultDto, StockLedgerEntryDto, SyncStatus,
    UpdateCategoryRequest, UpdateItemRequest, UpdateOrderRequest, UpdatePackRequest,
    UpdateSettingsRequest, UserSummary, VoidOrderRequest, WastageRequest, WorkerSummary,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl ApiError {
    fn network(message: impl Into<String>) -> Self {
        Self {
            status: 0,
            code: "offline".to_owned(),
            message: message.into(),
        }
    }

    #[must_use]
    pub fn is_auth(&self) -> bool {
        self.status == 401
    }

    #[must_use]
    pub fn is_offline(&self) -> bool {
        self.status == 0
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}]", self.message, self.code)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Health {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct Api {
    pub base_url: String,
    pub terminal_id: String,
    pub token: Option<String>,
}

impl Api {
    #[must_use]
    pub fn new(base_url: &str, terminal_id: &str, token: Option<&str>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            terminal_id: terminal_id.to_owned(),
            token: token.map(str::to_owned),
        }
    }

    #[must_use]
    pub fn asset_url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn finish<T: DeserializeOwned>(response: Response) -> Result<T, ApiError> {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if (200..300).contains(&status) {
            if text.trim().is_empty() {
                return serde_json::from_str("null").map_err(|error| ApiError {
                    status,
                    code: "bad_response".to_owned(),
                    message: error.to_string(),
                });
            }
            return serde_json::from_str(&text).map_err(|error| ApiError {
                status,
                code: "bad_response".to_owned(),
                message: format!("Unexpected server response: {error}"),
            });
        }
        let body: ApiErrorBody = serde_json::from_str(&text).unwrap_or(ApiErrorBody {
            code: "error".to_owned(),
            message: format!("The server returned status {status}."),
        });
        Err(ApiError {
            status,
            code: body.code,
            message: body.message,
        })
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<T, ApiError> {
        let url = format!("{}{}", self.base_url, path);
        let mut builder = match method {
            Method::GET => Request::get(&url),
            Method::POST => Request::post(&url),
            Method::PUT => Request::put(&url),
            Method::DELETE => Request::delete(&url),
            _ => Request::get(&url),
        };
        builder = builder.header("x-terminal-id", &self.terminal_id);
        if let Some(token) = &self.token {
            builder = builder.header("authorization", &format!("Bearer {token}"));
        }
        let request = match body {
            Some(body) => builder
                .json(body)
                .map_err(|error| ApiError::network(format!("Cannot build request: {error}")))?,
            None => builder
                .build()
                .map_err(|error| ApiError::network(format!("Cannot build request: {error}")))?,
        };
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                return Err(ApiError::network(format!(
                    "Cannot reach the server at {} ({error}).",
                    self.base_url
                )))
            }
        };
        Self::finish(response).await
    }

    async fn send_bytes<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<T, ApiError> {
        let url = format!("{}{}", self.base_url, path);
        let mut builder = match method {
            Method::POST => Request::post(&url),
            Method::PUT => Request::put(&url),
            Method::DELETE => Request::delete(&url),
            _ => Request::post(&url),
        };
        builder = builder
            .header("x-terminal-id", &self.terminal_id)
            .header("content-type", content_type);
        if let Some(token) = &self.token {
            builder = builder.header("authorization", &format!("Bearer {token}"));
        }
        let array = js_sys::Uint8Array::from(body);
        let request = builder
            .body(array.buffer())
            .map_err(|error| ApiError::network(format!("Cannot build request: {error}")))?;
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                return Err(ApiError::network(format!(
                    "Cannot reach the server at {} ({error}).",
                    self.base_url
                )))
            }
        };
        Self::finish(response).await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        self.call(Method::GET, path, None).await
    }

    async fn get_text(&self, path: &str) -> Result<String, ApiError> {
        let url = format!("{}{}", self.base_url, path);
        let mut builder = Request::get(&url).header("x-terminal-id", &self.terminal_id);
        if let Some(token) = &self.token {
            builder = builder.header("authorization", &format!("Bearer {token}"));
        }
        let request = builder
            .build()
            .map_err(|error| ApiError::network(format!("Cannot build request: {error}")))?;
        let response = request.send().await.map_err(|error| {
            ApiError::network(format!(
                "Cannot reach the server at {} ({error}).",
                self.base_url
            ))
        })?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if (200..300).contains(&status) {
            return Ok(text);
        }
        let body: ApiErrorBody = serde_json::from_str(&text).unwrap_or(ApiErrorBody {
            code: "error".to_owned(),
            message: format!("The server returned status {status}."),
        });
        Err(ApiError {
            status,
            code: body.code,
            message: body.message,
        })
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T, ApiError> {
        self.call(Method::POST, path, Some(body)).await
    }

    async fn put<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T, ApiError> {
        self.call(Method::PUT, path, Some(body)).await
    }

    fn json<T: serde::Serialize>(value: &T) -> Value {
        serde_json::to_value(value).unwrap_or(Value::Null)
    }

    pub async fn health(&self) -> Result<Health, ApiError> {
        self.get("/health").await
    }

    pub async fn setup_status(&self) -> Result<SetupStatus, ApiError> {
        self.get("/setup/status").await
    }

    pub async fn complete_setup(&self, request: &SetupRequest) -> Result<SetupResponse, ApiError> {
        self.post("/setup", &Self::json(request)).await
    }

    pub async fn login(&self, pin: &str) -> Result<LoginResponse, ApiError> {
        self.post("/auth/login", &serde_json::json!({ "pin": pin }))
            .await
    }

    pub async fn logout(&self) -> Result<(), ApiError> {
        self.call(Method::POST, "/auth/logout", None).await
    }

    pub async fn me(&self) -> Result<UserSummary, ApiError> {
        self.get("/auth/me").await
    }

    pub async fn menu(&self) -> Result<MenuResponse, ApiError> {
        self.get("/menu").await
    }

    pub async fn settings(&self) -> Result<SettingsDto, ApiError> {
        self.get("/settings").await
    }

    pub async fn update_settings(
        &self,
        request: &UpdateSettingsRequest,
    ) -> Result<SettingsDto, ApiError> {
        self.put("/settings", &Self::json(request)).await
    }

    pub async fn held_orders(&self) -> Result<Vec<OrderDto>, ApiError> {
        self.get("/orders/held").await
    }

    pub async fn order(&self, id: &str) -> Result<OrderDto, ApiError> {
        self.get(&format!("/orders/{id}")).await
    }

    pub async fn create_order(&self, request: &CreateOrderRequest) -> Result<OrderDto, ApiError> {
        self.post("/orders", &Self::json(request)).await
    }

    pub async fn update_order(
        &self,
        id: &str,
        request: &UpdateOrderRequest,
    ) -> Result<OrderDto, ApiError> {
        self.put(&format!("/orders/{id}"), &Self::json(request))
            .await
    }

    pub async fn pay_order(
        &self,
        id: &str,
        request: &PayOrderRequest,
    ) -> Result<PayOrderResponse, ApiError> {
        self.post(&format!("/orders/{id}/pay"), &Self::json(request))
            .await
    }

    pub async fn void_order(
        &self,
        id: &str,
        request: &VoidOrderRequest,
    ) -> Result<OrderDto, ApiError> {
        self.post(&format!("/orders/{id}/void"), &Self::json(request))
            .await
    }

    pub async fn print_order(
        &self,
        id: &str,
        request: &CreatePrintJobRequest,
    ) -> Result<PrintJobDetail, ApiError> {
        self.post(&format!("/orders/{id}/print"), &Self::json(request))
            .await
    }

    pub async fn workers(&self) -> Result<Vec<WorkerSummary>, ApiError> {
        self.get("/admin/workers").await
    }

    pub async fn create_worker(
        &self,
        request: &CreateWorkerRequest,
    ) -> Result<CreatedWorker, ApiError> {
        self.post("/admin/workers", &Self::json(request)).await
    }

    pub async fn reset_pin(&self, id: &str) -> Result<PinResetResponse, ApiError> {
        self.post(&format!("/admin/workers/{id}/reset-pin"), &Value::Null)
            .await
    }

    pub async fn deactivate_worker(&self, id: &str) -> Result<(), ApiError> {
        self.call(
            Method::POST,
            &format!("/admin/workers/{id}/deactivate"),
            Some(&Value::Null),
        )
        .await
    }

    pub async fn balances(&self) -> Result<Vec<StockBalanceDto>, ApiError> {
        self.get("/stock/balances").await
    }

    pub async fn add_batch(
        &self,
        request: &RecordBatchRequest,
    ) -> Result<StockBalanceDto, ApiError> {
        self.post("/stock/batches", &Self::json(request)).await
    }

    pub async fn restock(&self, request: &RestockRequest) -> Result<StockBalanceDto, ApiError> {
        self.post("/stock/restock", &Self::json(request)).await
    }

    pub async fn wastage(&self, request: &WastageRequest) -> Result<StockBalanceDto, ApiError> {
        self.post("/stock/wastage", &Self::json(request)).await
    }

    pub async fn stock_count(
        &self,
        request: &StockCountRequest,
    ) -> Result<Vec<StockCountResultDto>, ApiError> {
        self.post("/stock/counts", &Self::json(request)).await
    }

    pub async fn stock_ledger(
        &self,
        item_id: Option<&str>,
    ) -> Result<Vec<StockLedgerEntryDto>, ApiError> {
        let query = match item_id {
            Some(id) => format!("/stock/ledger?limit=200&item_id={id}"),
            None => "/stock/ledger?limit=200".to_owned(),
        };
        self.get(&query).await
    }

    pub async fn daily_report(&self, date: &str) -> Result<DailyReport, ApiError> {
        self.get(&format!("/reports/daily?date={date}")).await
    }

    pub async fn sales_csv(&self, date: &str) -> Result<String, ApiError> {
        self.get_text(&format!("/reports/sales.csv?date={date}"))
            .await
    }

    pub async fn stock_csv(&self, date: &str) -> Result<String, ApiError> {
        self.get_text(&format!("/reports/stock.csv?date={date}"))
            .await
    }

    pub async fn sync_status(&self) -> Result<SyncStatus, ApiError> {
        self.get("/sync/status").await
    }

    pub async fn create_category(
        &self,
        request: &CreateCategoryRequest,
    ) -> Result<MenuResponse, ApiError> {
        self.post("/admin/categories", &Self::json(request)).await
    }

    pub async fn update_category(
        &self,
        id: &str,
        request: &UpdateCategoryRequest,
    ) -> Result<MenuResponse, ApiError> {
        self.put(&format!("/admin/categories/{id}"), &Self::json(request))
            .await
    }

    pub async fn create_item(&self, request: &CreateItemRequest) -> Result<MenuResponse, ApiError> {
        self.post("/admin/items", &Self::json(request)).await
    }

    pub async fn update_item(
        &self,
        id: &str,
        request: &UpdateItemRequest,
    ) -> Result<MenuResponse, ApiError> {
        self.put(&format!("/admin/items/{id}"), &Self::json(request))
            .await
    }

    pub async fn create_pack(&self, request: &CreatePackRequest) -> Result<MenuResponse, ApiError> {
        self.post("/admin/packs", &Self::json(request)).await
    }

    pub async fn update_pack(
        &self,
        id: &str,
        request: &UpdatePackRequest,
    ) -> Result<MenuResponse, ApiError> {
        self.put(&format!("/admin/packs/{id}"), &Self::json(request))
            .await
    }

    pub async fn duplicate_pack(&self, id: &str) -> Result<MenuResponse, ApiError> {
        self.post(&format!("/admin/packs/{id}/duplicate"), &Value::Null)
            .await
    }

    pub async fn upload_item_image(
        &self,
        id: &str,
        mime: &str,
        bytes: &[u8],
    ) -> Result<MenuResponse, ApiError> {
        self.send_bytes(
            Method::POST,
            &format!("/admin/items/{id}/image"),
            mime,
            bytes,
        )
        .await
    }

    pub async fn delete_item_image(&self, id: &str) -> Result<MenuResponse, ApiError> {
        self.call(Method::DELETE, &format!("/admin/items/{id}/image"), None)
            .await
    }

    pub async fn upload_pack_image(
        &self,
        id: &str,
        mime: &str,
        bytes: &[u8],
    ) -> Result<MenuResponse, ApiError> {
        self.send_bytes(
            Method::POST,
            &format!("/admin/packs/{id}/image"),
            mime,
            bytes,
        )
        .await
    }

    pub async fn delete_pack_image(&self, id: &str) -> Result<MenuResponse, ApiError> {
        self.call(Method::DELETE, &format!("/admin/packs/{id}/image"), None)
            .await
    }
}
