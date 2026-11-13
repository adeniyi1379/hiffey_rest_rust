//! Reads the menu with prices resolved for a point in time.
//!
//! Both the menu screen and the sale transaction use these helpers, so a sale
//! always charges exactly the price the menu showed.

use std::collections::HashMap;

use pos_core::{pack_price, Money};
use pos_types::{
    CategoryDto, MenuItemDto, MenuResponse, PackComponentDto, PackDto, PackPricingMode, UnitType,
};
use sqlx::{Row, SqlitePool};

use crate::error::ApiError;

/// An item with the price that applies at the requested moment.
#[derive(Debug, Clone)]
pub struct PricedItem {
    pub id: String,
    pub name: String,
    pub category_id: String,
    pub category_name: String,
    pub unit_type: UnitType,
    pub price_kobo: i64,
    pub low_stock_level: i64,
    pub active: bool,
    pub updated_at: String,
    pub image_updated_at: Option<String>,
}

impl PricedItem {
    #[must_use]
    pub fn unit_price(&self) -> Money {
        Money::from_kobo(self.price_kobo)
    }
}

/// A pack with its components priced at the requested moment.
#[derive(Debug, Clone)]
pub struct PricedPack {
    pub id: String,
    pub name: String,
    pub pricing_mode: PackPricingMode,
    pub fixed_price_kobo: Option<i64>,
    pub price_kobo: i64,
    pub active: bool,
    pub components: Vec<PackComponentDto>,
    pub image_updated_at: Option<String>,
}

fn image_url(kind: &str, id: &str, updated_at: Option<&str>) -> Option<String> {
    updated_at.map(|stamp| {
        let version: String = stamp
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .collect();
        format!("/menu/{kind}/{id}/image?v={version}")
    })
}

fn parse_unit(value: &str) -> Result<UnitType, ApiError> {
    value
        .parse::<UnitType>()
        .map_err(|()| ApiError::internal(format!("unknown unit type in database: {value}")))
}

fn parse_mode(value: &str) -> Result<PackPricingMode, ApiError> {
    value
        .parse::<PackPricingMode>()
        .map_err(|()| ApiError::internal(format!("unknown pack pricing mode: {value}")))
}

/// Loads every item, keyed by id, with its effective price.
pub async fn priced_items(
    pool: &SqlitePool,
    at: &str,
) -> Result<HashMap<String, PricedItem>, ApiError> {
    let rows = sqlx::query(
        "SELECT i.id, i.name, i.category_id, c.name AS category_name, i.unit_type,
                COALESCE(
                    (SELECT p.price_kobo FROM item_prices p
                     WHERE p.item_id = i.id AND p.effective_from <= ?
                     ORDER BY p.effective_from DESC LIMIT 1),
                    i.price_per_unit_kobo
                ) AS current_price,
                i.low_stock_level, i.active, i.updated_at, i.image_updated_at
         FROM items i
         JOIN categories c ON c.id = i.category_id
         ORDER BY c.sort_order, c.name COLLATE NOCASE, i.name COLLATE NOCASE",
    )
    .bind(at)
    .fetch_all(pool)
    .await?;

    let mut items = HashMap::with_capacity(rows.len());
    for row in &rows {
        let unit_type: String = row.get("unit_type");
        let item = PricedItem {
            id: row.get("id"),
            name: row.get("name"),
            category_id: row.get("category_id"),
            category_name: row.get("category_name"),
            unit_type: parse_unit(&unit_type)?,
            price_kobo: row.get("current_price"),
            low_stock_level: row.get("low_stock_level"),
            active: row.get::<i64, _>("active") != 0,
            updated_at: row.get("updated_at"),
            image_updated_at: row.get("image_updated_at"),
        };
        items.insert(item.id.clone(), item);
    }
    Ok(items)
}

/// Loads every pack with its components and current price.
pub async fn priced_packs(
    pool: &SqlitePool,
    items: &HashMap<String, PricedItem>,
) -> Result<Vec<PricedPack>, ApiError> {
    let pack_rows = sqlx::query(
        "SELECT id, name, pricing_mode, fixed_price_kobo, active, image_updated_at
         FROM packs ORDER BY name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;

    let component_rows = sqlx::query(
        "SELECT pack_id, item_id, quantity FROM pack_components ORDER BY pack_id, item_id",
    )
    .fetch_all(pool)
    .await?;

    let mut components: HashMap<String, Vec<PackComponentDto>> = HashMap::new();
    for row in &component_rows {
        let item_id: String = row.get("item_id");
        let Some(item) = items.get(&item_id) else {
            continue;
        };
        components
            .entry(row.get("pack_id"))
            .or_default()
            .push(PackComponentDto {
                item_id,
                item_name: item.name.clone(),
                unit_type: item.unit_type,
                quantity: row.get("quantity"),
                unit_price_kobo: item.price_kobo,
            });
    }

    let mut packs = Vec::with_capacity(pack_rows.len());
    for row in &pack_rows {
        let mode_text: String = row.get("pricing_mode");
        let mode = parse_mode(&mode_text)?;
        let fixed_price_kobo: Option<i64> = row.get("fixed_price_kobo");
        let id: String = row.get("id");
        let pack_components = components.remove(&id).unwrap_or_default();

        let pairs: Vec<(Money, i64)> = pack_components
            .iter()
            .map(|component| {
                (
                    Money::from_kobo(component.unit_price_kobo),
                    component.quantity,
                )
            })
            .collect();
        let price = pack_price(
            mode == PackPricingMode::Fixed,
            fixed_price_kobo.map(Money::from_kobo),
            &pairs,
        )
        .unwrap_or_default();

        packs.push(PricedPack {
            id,
            name: row.get("name"),
            pricing_mode: mode,
            fixed_price_kobo,
            price_kobo: price.kobo(),
            active: row.get::<i64, _>("active") != 0,
            components: pack_components,
            image_updated_at: row.get("image_updated_at"),
        });
    }
    Ok(packs)
}

/// The complete menu as returned to the UI.
pub async fn load_menu(pool: &SqlitePool, at: &str) -> Result<MenuResponse, ApiError> {
    let category_rows = sqlx::query(
        "SELECT id, name, sort_order, active FROM categories
         ORDER BY sort_order, name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;

    let categories: Vec<CategoryDto> = category_rows
        .iter()
        .map(|row| CategoryDto {
            id: row.get("id"),
            name: row.get("name"),
            sort_order: row.get("sort_order"),
            active: row.get::<i64, _>("active") != 0,
        })
        .collect();

    let balance_rows = sqlx::query("SELECT item_id, balance FROM stock_balances")
        .fetch_all(pool)
        .await?;
    let balances: HashMap<String, i64> = balance_rows
        .iter()
        .map(|row| (row.get("item_id"), row.get("balance")))
        .collect();

    let priced = priced_items(pool, at).await?;
    let mut item_dtos: Vec<MenuItemDto> = priced
        .values()
        .map(|item| {
            let stock_balance = balances.get(&item.id).copied().unwrap_or(0);
            MenuItemDto {
                id: item.id.clone(),
                name: item.name.clone(),
                category_id: item.category_id.clone(),
                category_name: item.category_name.clone(),
                unit_type: item.unit_type,
                price_per_unit_kobo: item.price_kobo,
                low_stock_level: item.low_stock_level,
                stock_balance,
                sold_out: stock_balance <= 0,
                low_stock: stock_balance > 0 && stock_balance <= item.low_stock_level,
                active: item.active,
                updated_at: item.updated_at.clone(),
                image_url: image_url("items", &item.id, item.image_updated_at.as_deref()),
            }
        })
        .collect();
    item_dtos.sort_by(|a, b| {
        a.category_name
            .to_lowercase()
            .cmp(&b.category_name.to_lowercase())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    let packs = priced_packs(pool, &priced)
        .await?
        .into_iter()
        .map(|pack| {
            let sold_out = pack.components.iter().any(|component| {
                balances.get(&component.item_id).copied().unwrap_or(0) < component.quantity
            });
            let image_url = image_url("packs", &pack.id, pack.image_updated_at.as_deref());
            PackDto {
                id: pack.id,
                name: pack.name,
                pricing_mode: pack.pricing_mode,
                fixed_price_kobo: pack.fixed_price_kobo,
                price_kobo: pack.price_kobo,
                active: pack.active,
                sold_out,
                components: pack.components,
                image_url,
            }
        })
        .collect();

    Ok(MenuResponse {
        categories,
        items: item_dtos,
        packs,
    })
}
