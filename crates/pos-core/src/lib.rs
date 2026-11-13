//! Pure business rules for the Hiffey POS.
//!
//! This crate contains no I/O: no database, no network, no clock, no filesystem.
//! Everything here is deterministic and unit tested.

mod money;
mod order_no;
mod pricing;

pub use money::Money;
pub use order_no::{format_order_no, ORDER_PREFIX};
pub use pricing::{calculated_pack_price, line_total, order_total, pack_price};
