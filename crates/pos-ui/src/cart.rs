//! The sale screen's working order. Pure state so it can be tested on the host.

use pos_types::{MenuItemDto, OrderDto, OrderLineInput, OrderType, PackDto, SettingsDto};

use crate::format::quantity_label;

const MAX_QUANTITY: i64 = 999;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Item,
    Pack,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartLine {
    pub kind: LineKind,
    pub reference_id: String,
    pub name: String,
    pub unit_price_kobo: i64,
    pub quantity: i64,
    /// Pack contents, one short line each; empty for a custom plate.
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cart {
    pub lines: Vec<CartLine>,
    pub order_type: OrderType,
    pub table_no: String,
    /// Set when a held order is being edited, so saving updates instead of creating.
    pub editing_id: Option<String>,
    pub editing_no: Option<String>,
}

impl Default for Cart {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            order_type: OrderType::DineIn,
            table_no: String::new(),
            editing_id: None,
            editing_no: None,
        }
    }
}

fn pack_details(pack: &PackDto) -> Vec<String> {
    pack.components
        .iter()
        .map(|component| {
            format!(
                "{} {}",
                quantity_label(component.quantity, component.unit_type),
                component.item_name
            )
        })
        .collect()
}

fn clamp_quantity(quantity: i64) -> i64 {
    quantity.clamp(1, MAX_QUANTITY)
}

impl Cart {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Adds an item, merging with an existing line for the same item.
    pub fn add_item(&mut self, item: &MenuItemDto, quantity: i64) {
        self.push(
            LineKind::Item,
            &item.id,
            &item.name,
            item.price_per_unit_kobo,
            quantity,
            Vec::new(),
        );
    }

    /// Adds a whole pack with its contents listed under the line.
    pub fn add_pack(&mut self, pack: &PackDto, quantity: i64) {
        self.push(
            LineKind::Pack,
            &pack.id,
            &pack.name,
            pack.price_kobo,
            quantity,
            pack_details(pack),
        );
    }

    fn push(
        &mut self,
        kind: LineKind,
        reference_id: &str,
        name: &str,
        unit_price_kobo: i64,
        quantity: i64,
        details: Vec<String>,
    ) {
        let quantity = clamp_quantity(quantity);
        if let Some(line) = self
            .lines
            .iter_mut()
            .find(|line| line.kind == kind && line.reference_id == reference_id)
        {
            line.quantity = (line.quantity + quantity).min(MAX_QUANTITY);
            return;
        }
        self.lines.push(CartLine {
            kind,
            reference_id: reference_id.to_owned(),
            name: name.to_owned(),
            unit_price_kobo,
            quantity,
            details,
        });
    }

    pub fn adjust(&mut self, index: usize, delta: i64) {
        let Some(line) = self.lines.get_mut(index) else {
            return;
        };
        line.quantity = clamp_quantity(line.quantity + delta);
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.lines.len() {
            self.lines.remove(index);
        }
    }

    /// Completes a sale, keeping the order type so the next ticket starts the same way.
    pub fn reset(&mut self) {
        let order_type = self.order_type;
        *self = Self {
            order_type,
            ..Self::default()
        };
    }

    /// Loads a held order for editing.
    pub fn resume(&mut self, order: &OrderDto) {
        self.order_type = order.order_type;
        self.table_no = order.table_no.clone().unwrap_or_default();
        self.editing_id = Some(order.id.clone());
        self.editing_no = Some(order.order_no.clone());
        self.lines = order.lines.iter().map(line_from_order).collect();
    }

    pub fn subtotal_kobo(&self) -> i64 {
        self.lines
            .iter()
            .map(|line| line.unit_price_kobo * line.quantity)
            .sum()
    }

    pub fn packaging_fee_kobo(&self, settings: Option<&SettingsDto>) -> i64 {
        match (&self.order_type, settings) {
            (OrderType::Takeaway, Some(settings)) => settings.packaging_fee_kobo,
            _ => 0,
        }
    }

    pub fn total_kobo(&self, settings: Option<&SettingsDto>) -> i64 {
        self.subtotal_kobo() + self.packaging_fee_kobo(settings)
    }

    pub fn to_inputs(&self) -> Vec<OrderLineInput> {
        self.lines
            .iter()
            .map(|line| match line.kind {
                LineKind::Item => OrderLineInput::Item {
                    item_id: line.reference_id.clone(),
                    quantity: line.quantity,
                },
                LineKind::Pack => OrderLineInput::Pack {
                    pack_id: line.reference_id.clone(),
                    quantity: line.quantity,
                },
            })
            .collect()
    }
}

fn line_from_order(line: &pos_types::OrderLineDto) -> CartLine {
    let kind = if line.kind == "pack" {
        LineKind::Pack
    } else {
        LineKind::Item
    };
    let details = if kind == LineKind::Pack {
        line.components
            .iter()
            .map(|component| {
                let per_pack = if line.quantity > 0 {
                    component.quantity / line.quantity
                } else {
                    component.quantity
                };
                format!(
                    "{} {}",
                    quantity_label(per_pack, component.unit_type),
                    component.item_name
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    CartLine {
        kind,
        reference_id: line
            .item_id
            .clone()
            .or_else(|| line.pack_id.clone())
            .unwrap_or_default(),
        name: line.name.clone(),
        unit_price_kobo: line.unit_price_kobo,
        quantity: line.quantity,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::Cart;
    use pos_types::{
        MenuItemDto, OrderDto, OrderLineComponentDto, OrderLineDto, OrderStatus, OrderType,
        PackComponentDto, PackDto, PackPricingMode, SettingsDto, UnitType,
    };

    fn item(id: &str, name: &str, price: i64) -> MenuItemDto {
        MenuItemDto {
            id: id.to_owned(),
            name: name.to_owned(),
            category_id: "cat".to_owned(),
            category_name: "Rice".to_owned(),
            unit_type: UnitType::Spoon,
            price_per_unit_kobo: price,
            low_stock_level: 5,
            stock_balance: 10,
            sold_out: false,
            low_stock: false,
            active: true,
            updated_at: String::new(),
            image_url: None,
        }
    }

    fn pack(id: &str, name: &str, price: i64) -> PackDto {
        PackDto {
            id: id.to_owned(),
            name: name.to_owned(),
            pricing_mode: PackPricingMode::Calculated,
            fixed_price_kobo: None,
            price_kobo: price,
            active: true,
            sold_out: false,
            components: vec![PackComponentDto {
                item_id: "item-1".to_owned(),
                item_name: "Jollof Rice".to_owned(),
                unit_type: UnitType::Spoon,
                quantity: 2,
                unit_price_kobo: 35_000,
            }],
            image_url: None,
        }
    }

    fn settings() -> SettingsDto {
        SettingsDto {
            business_name: "Hiffey".to_owned(),
            business_address: String::new(),
            business_phone: String::new(),
            receipt_footer: String::new(),
            packaging_fee_kobo: 20_000,
            low_stock_default: 5,
            updated_at: String::new(),
        }
    }

    #[test]
    fn lines_merge_and_totals_are_exact() {
        let mut cart = Cart::default();
        cart.add_item(&item("item-1", "Jollof Rice", 35_000), 2);
        cart.add_item(&item("item-1", "Jollof Rice", 35_000), 1);
        cart.add_pack(&pack("pack-1", "Standard Pack", 150_000), 1);
        assert_eq!(cart.lines.len(), 2);
        assert_eq!(cart.lines[0].quantity, 3);
        assert_eq!(cart.lines[1].details, vec!["2 spoons Jollof Rice"]);
        assert_eq!(cart.subtotal_kobo(), 3 * 35_000 + 150_000);
        assert_eq!(cart.packaging_fee_kobo(Some(&settings())), 0);
        cart.order_type = OrderType::Takeaway;
        assert_eq!(cart.packaging_fee_kobo(Some(&settings())), 20_000);
        assert_eq!(cart.total_kobo(Some(&settings())), 275_000);
    }

    #[test]
    fn quantities_clamp_and_zero_is_never_saved() {
        let mut cart = Cart::default();
        cart.add_item(&item("item-1", "Jollof Rice", 35_000), 1);
        cart.adjust(0, -5);
        assert_eq!(cart.lines[0].quantity, 1);
        cart.adjust(0, 2_000);
        assert_eq!(cart.lines[0].quantity, 999);
        cart.remove(0);
        assert!(cart.is_empty());
    }

    #[test]
    fn resuming_a_held_order_restores_its_lines() {
        let order = OrderDto {
            id: "order-1".to_owned(),
            order_no: "HIF-20261111-0001".to_owned(),
            trading_day: "2026-11-11".to_owned(),
            order_type: OrderType::Takeaway,
            table_no: None,
            status: OrderStatus::Held,
            subtotal_kobo: 150_000,
            packaging_fee_kobo: 20_000,
            total_kobo: 170_000,
            user_id: "user-1".to_owned(),
            user_name: "Chidi".to_owned(),
            terminal_id: None,
            created_at: String::new(),
            updated_at: String::new(),
            paid_at: None,
            voided_at: None,
            void_reason: None,
            lines: vec![OrderLineDto {
                id: "line-1".to_owned(),
                kind: "pack".to_owned(),
                item_id: None,
                pack_id: Some("pack-1".to_owned()),
                name: "Standard Pack".to_owned(),
                quantity: 1,
                unit_price_kobo: 150_000,
                line_total_kobo: 150_000,
                components: vec![OrderLineComponentDto {
                    item_id: "item-1".to_owned(),
                    item_name: "Jollof Rice".to_owned(),
                    unit_type: UnitType::Spoon,
                    quantity: 2,
                    unit_price_kobo: 35_000,
                }],
            }],
            payments: Vec::new(),
        };

        let mut cart = Cart::default();
        cart.resume(&order);
        assert_eq!(cart.editing_no.as_deref(), Some("HIF-20261111-0001"));
        assert_eq!(cart.order_type, OrderType::Takeaway);
        assert_eq!(cart.lines[0].details, vec!["2 spoons Jollof Rice"]);
        assert_eq!(cart.total_kobo(Some(&settings())), 170_000);
    }
}
