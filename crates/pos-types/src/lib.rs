//! Shared data types and API request/response models.

use serde::{Deserialize, Serialize};

/// Who a worker is, and therefore what they may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Manager,
    Cashier,
}

impl Role {
    /// Owner and manager may change the menu, packs, workers and settings.
    #[must_use]
    pub const fn can_manage(self) -> bool {
        matches!(self, Self::Owner | Self::Manager)
    }

    /// Owner and manager may void sales and record stock movements.
    #[must_use]
    pub const fn can_void(self) -> bool {
        matches!(self, Self::Owner | Self::Manager)
    }

    #[must_use]
    pub const fn can_backup(self) -> bool {
        matches!(self, Self::Owner)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Manager => "manager",
            Self::Cashier => "cashier",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "owner" => Ok(Self::Owner),
            "manager" => Ok(Self::Manager),
            "cashier" => Ok(Self::Cashier),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a desktop install is configured on first run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalMode {
    /// Terminal A: hosts the database and the LAN server, and runs the UI.
    ServerClient,
    /// Terminal B: connects to Terminal A over the shop LAN.
    Client,
}

/// The worker identity returned to the UI. Never contains the PIN hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSummary {
    pub id: String,
    pub name: String,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub pin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserSummary,
    pub expires_at: String,
}

/// Whether this server has been configured yet (i.e. has an owner account).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupStatus {
    pub configured: bool,
    pub business_name: Option<String>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupRequest {
    pub business_name: String,
    pub owner_name: String,
    #[serde(default)]
    pub business_address: String,
    #[serde(default)]
    pub business_phone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupResponse {
    /// The generated owner PIN. Shown exactly once.
    pub pin: String,
    pub session: LoginResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsDto {
    pub business_name: String,
    pub business_address: String,
    pub business_phone: String,
    pub receipt_footer: String,
    pub packaging_fee_kobo: i64,
    pub low_stock_default: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettingsRequest {
    pub business_name: String,
    pub business_address: String,
    pub business_phone: String,
    pub receipt_footer: String,
    pub packaging_fee_kobo: i64,
    pub low_stock_default: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSummary {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub active: bool,
    pub created_at: String,
    pub pin_reset_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkerRequest {
    pub name: String,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedWorker {
    pub worker: WorkerSummary,
    /// The generated PIN. Shown exactly once.
    pub pin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinResetResponse {
    /// The new generated PIN. Shown exactly once.
    pub pin: String,
}

/// How an item is counted: food by the standard spoon, drinks/pastries by piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitType {
    Spoon,
    Count,
}

impl UnitType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spoon => "spoon",
            Self::Count => "count",
        }
    }

    /// The word shown next to quantities in the UI and on receipts.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Spoon => "spoon",
            Self::Count => "piece",
        }
    }
}

impl std::str::FromStr for UnitType {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "spoon" => Ok(Self::Spoon),
            "count" => Ok(Self::Count),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for UnitType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a pack's price is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackPricingMode {
    /// Sum of the current component prices.
    Calculated,
    /// A price the manager fixed for the whole pack.
    Fixed,
}

impl PackPricingMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Calculated => "calculated",
            Self::Fixed => "fixed",
        }
    }
}

impl std::str::FromStr for PackPricingMode {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "calculated" => Ok(Self::Calculated),
            "fixed" => Ok(Self::Fixed),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for PackPricingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryDto {
    pub id: String,
    pub name: String,
    pub sort_order: i64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuItemDto {
    pub id: String,
    pub name: String,
    pub category_id: String,
    pub category_name: String,
    pub unit_type: UnitType,
    /// The price that applies right now (dated prices already resolved).
    pub price_per_unit_kobo: i64,
    pub low_stock_level: i64,
    /// What is on the shelf right now.
    pub stock_balance: i64,
    /// True when nothing is left, so the sale screen greys it out.
    pub sold_out: bool,
    /// True when the balance has fallen to the warning level.
    pub low_stock: bool,
    pub active: bool,
    pub updated_at: String,
    /// Relative URL for this item's photo, when one has been uploaded.
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackComponentDto {
    pub item_id: String,
    pub item_name: String,
    pub unit_type: UnitType,
    pub quantity: i64,
    pub unit_price_kobo: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackDto {
    pub id: String,
    pub name: String,
    pub pricing_mode: PackPricingMode,
    pub fixed_price_kobo: Option<i64>,
    /// What the pack costs right now (calculated sum or the fixed price).
    pub price_kobo: i64,
    pub active: bool,
    /// True when at least one component cannot cover a single pack.
    pub sold_out: bool,
    pub components: Vec<PackComponentDto>,
    /// Relative URL for this pack's photo, when one has been uploaded.
    pub image_url: Option<String>,
}

/// Everything the sale screen needs to draw the menu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuResponse {
    pub categories: Vec<CategoryDto>,
    pub items: Vec<MenuItemDto>,
    pub packs: Vec<PackDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCategoryRequest {
    pub name: String,
    #[serde(default)]
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCategoryRequest {
    pub name: String,
    pub sort_order: i64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateItemRequest {
    pub name: String,
    pub category_id: String,
    pub unit_type: UnitType,
    pub price_per_unit_kobo: i64,
    #[serde(default)]
    pub low_stock_level: i64,
    /// RFC 3339. Defaults to now.
    #[serde(default)]
    pub effective_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateItemRequest {
    pub name: String,
    pub category_id: String,
    pub unit_type: UnitType,
    pub price_per_unit_kobo: i64,
    pub low_stock_level: i64,
    pub active: bool,
    /// RFC 3339. Defaults to now when the price changes.
    #[serde(default)]
    pub effective_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackComponentInput {
    pub item_id: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePackRequest {
    pub name: String,
    pub pricing_mode: PackPricingMode,
    #[serde(default)]
    pub fixed_price_kobo: Option<i64>,
    #[serde(default)]
    pub components: Vec<PackComponentInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePackRequest {
    pub name: String,
    pub pricing_mode: PackPricingMode,
    #[serde(default)]
    pub fixed_price_kobo: Option<i64>,
    pub active: bool,
    #[serde(default)]
    pub components: Vec<PackComponentInput>,
}

/// Dine-in or takeaway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    DineIn,
    Takeaway,
}

impl OrderType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DineIn => "dine_in",
            Self::Takeaway => "takeaway",
        }
    }
}

impl std::str::FromStr for OrderType {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "dine_in" => Ok(Self::DineIn),
            "takeaway" => Ok(Self::Takeaway),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for OrderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Held,
    Paid,
    Voided,
}

impl OrderStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Paid => "paid",
            Self::Voided => "voided",
        }
    }
}

impl std::str::FromStr for OrderStatus {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "held" => Ok(Self::Held),
            "paid" => Ok(Self::Paid),
            "voided" => Ok(Self::Voided),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentMethod {
    Cash,
    Transfer,
    Card,
}

impl PaymentMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cash => "cash",
            Self::Transfer => "transfer",
            Self::Card => "card",
        }
    }
}

impl std::str::FromStr for PaymentMethod {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "cash" => Ok(Self::Cash),
            "transfer" => Ok(Self::Transfer),
            "card" => Ok(Self::Card),
            _ => Err(()),
        }
    }
}

/// One line as the UI adds it: an item with spoons/pieces, or a whole pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OrderLineInput {
    Item { item_id: String, quantity: i64 },
    Pack { pack_id: String, quantity: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderRequest {
    pub order_type: OrderType,
    /// Required for dine-in.
    #[serde(default)]
    pub table_no: Option<String>,
    pub lines: Vec<OrderLineInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateOrderRequest {
    pub order_type: OrderType,
    #[serde(default)]
    pub table_no: Option<String>,
    pub lines: Vec<OrderLineInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderLineComponentDto {
    pub item_id: String,
    pub item_name: String,
    pub unit_type: UnitType,
    pub quantity: i64,
    pub unit_price_kobo: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderLineDto {
    pub id: String,
    pub kind: String,
    pub item_id: Option<String>,
    pub pack_id: Option<String>,
    pub name: String,
    pub quantity: i64,
    pub unit_price_kobo: i64,
    pub line_total_kobo: i64,
    pub components: Vec<OrderLineComponentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentDto {
    pub id: String,
    pub method: PaymentMethod,
    pub amount_kobo: i64,
    pub reference: Option<String>,
    pub confirmed_by: String,
    pub confirmed_by_name: String,
    pub confirmed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderDto {
    pub id: String,
    pub order_no: String,
    pub trading_day: String,
    pub order_type: OrderType,
    pub table_no: Option<String>,
    pub status: OrderStatus,
    pub subtotal_kobo: i64,
    pub packaging_fee_kobo: i64,
    pub total_kobo: i64,
    pub user_id: String,
    pub user_name: String,
    pub terminal_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub paid_at: Option<String>,
    pub voided_at: Option<String>,
    pub void_reason: Option<String>,
    pub lines: Vec<OrderLineDto>,
    pub payments: Vec<PaymentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentInput {
    pub method: PaymentMethod,
    pub amount_kobo: i64,
    #[serde(default)]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayOrderRequest {
    pub payments: Vec<PaymentInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayOrderResponse {
    pub order: OrderDto,
    /// Amount handed back when cash tendered exceeds the total.
    pub change_kobo: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoidOrderRequest {
    /// Required only when voiding an order that has already been paid.
    #[serde(default)]
    pub manager_pin: Option<String>,
    #[serde(default)]
    pub reason: String,
    /// Put the sold quantity back on the shelf; otherwise the food is treated as gone.
    #[serde(default)]
    pub return_to_stock: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockBalanceDto {
    pub item_id: String,
    pub item_name: String,
    pub unit_type: UnitType,
    pub balance: i64,
    pub low_stock_level: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordBatchRequest {
    pub item_id: String,
    pub quantity: i64,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Spoiled food, staff meals, giveaways and mistakes come off stock here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WastageRequest {
    pub item_id: String,
    pub quantity: i64,
    pub reason: String,
}

/// Bringing pastries, drinks or a fresh batch into stock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestockRequest {
    pub item_id: String,
    pub quantity: i64,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockCountInput {
    pub item_id: String,
    /// What is actually on the shelf, counted by hand.
    pub counted_qty: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockCountRequest {
    pub counts: Vec<StockCountInput>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// One line of an end-of-day count, with the difference from the expected balance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockCountResultDto {
    pub item_id: String,
    pub item_name: String,
    pub expected_qty: i64,
    pub counted_qty: i64,
    pub difference: i64,
}

/// What is being sent to the printer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintJobType {
    Receipt,
    Duplicate,
    Slip,
}

impl PrintJobType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Receipt => "receipt",
            Self::Duplicate => "duplicate",
            Self::Slip => "slip",
        }
    }
}

impl std::str::FromStr for PrintJobType {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "receipt" => Ok(Self::Receipt),
            "duplicate" => Ok(Self::Duplicate),
            "slip" => Ok(Self::Slip),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintJobStatus {
    Queued,
    Printed,
    Failed,
}

impl PrintJobStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Printed => "printed",
            Self::Failed => "failed",
        }
    }
}

impl std::str::FromStr for PrintJobStatus {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "printed" => Ok(Self::Printed),
            "failed" => Ok(Self::Failed),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for PrintJobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJobDto {
    pub id: String,
    pub order_id: String,
    pub order_no: String,
    pub job_type: PrintJobType,
    pub status: PrintJobStatus,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub paper_width: String,
    pub requested_by: Option<String>,
    pub requested_by_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJobDetail {
    pub job: PrintJobDto,
    /// The exact text queued at the time the job was created.
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePrintJobRequest {
    pub job_type: PrintJobType,
    /// `mm58` or `mm80`; defaults to 80 mm.
    #[serde(default)]
    pub paper_width: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJobResultRequest {
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
}

/// A permanent stock movement as shown in the history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockLedgerEntryDto {
    pub id: String,
    pub item_id: String,
    pub item_name: String,
    pub movement_type: String,
    pub quantity: i64,
    pub reason: Option<String>,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub created_at: String,
}

/// Money recorded against one payment method on a trading day.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentBreakdownDto {
    pub method: PaymentMethod,
    /// How many paid orders used this method.
    pub orders: i64,
    /// What the terminal recorded; cash includes any change handed back.
    pub amount_kobo: i64,
}

/// One line of the item mix: how much of an item left the shelf and its value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportItemDto {
    pub item_id: String,
    pub item_name: String,
    pub unit_type: UnitType,
    pub quantity: i64,
    pub revenue_kobo: i64,
}

/// The owner's view of one trading day.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyReport {
    /// The trading day as `YYYY-MM-DD`.
    pub date: String,
    pub paid_orders: i64,
    pub dine_in_orders: i64,
    pub takeaway_orders: i64,
    pub held_orders: i64,
    pub voided_orders: i64,
    pub subtotal_kobo: i64,
    pub packaging_kobo: i64,
    /// Paid sales after voided tickets are excluded.
    pub total_kobo: i64,
    pub average_kobo: i64,
    /// What the terminal recorded across all payment methods.
    pub tendered_kobo: i64,
    /// Cash handed back on paid orders.
    pub change_kobo: i64,
    /// The value of tickets that were paid and later voided.
    pub voided_total_kobo: i64,
    pub payments: Vec<PaymentBreakdownDto>,
    pub items: Vec<ReportItemDto>,
}

/// A push event sent over the `/events` WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEvent {
    pub event: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub enabled: bool,
    pub pending_count: i64,
    pub last_success_at: Option<String>,
}

/// The error body returned by every failing endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::Role;

    #[test]
    fn cashier_cannot_manage() {
        assert!(!Role::Cashier.can_manage());
        assert!(!Role::Cashier.can_void());
        assert!(!Role::Cashier.can_backup());
    }

    #[test]
    fn manager_can_manage_but_not_backup() {
        assert!(Role::Manager.can_manage());
        assert!(Role::Manager.can_void());
        assert!(!Role::Manager.can_backup());
    }

    #[test]
    fn owner_can_do_everything() {
        assert!(Role::Owner.can_manage());
        assert!(Role::Owner.can_void());
        assert!(Role::Owner.can_backup());
    }

    #[test]
    fn role_round_trips_through_string() {
        for role in [Role::Owner, Role::Manager, Role::Cashier] {
            assert_eq!(role.as_str().parse::<Role>(), Ok(role));
        }
    }
}
