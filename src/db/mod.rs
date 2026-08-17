//! The data layer. This module owns the model and the shop's money rules;
//! the queries live in one backend per host -- sqlite behind tokio for the
//! native binary, d1 behind the Worker bindings at the edge. Both expose
//! the same functions with the same signatures, so the pages never know
//! which one they are talking to.

#[cfg(feature = "native")]
mod sqlite;
#[cfg(feature = "native")]
pub use sqlite::*;

#[cfg(feature = "edge")]
mod d1;
#[cfg(feature = "edge")]
pub use d1::*;

/// anyhow, so `?` converts straight into topcoat's error type at the
/// call sites without a per-module helper.
pub type Error = anyhow::Error;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct Product {
    pub sku: String,
    pub name: String,
    pub summary: String,
    pub detail: String,
    pub price_cents: i64,
    pub stock: i64,
    pub category: String,
    pub is_new: i64,
    pub material: String,
    pub hidden: i64,
}

impl Product {
    pub fn sold_out(&self) -> bool {
        self.stock == 0
    }
    /// Below this, the product page says so rather than staying silent.
    pub fn low_stock(&self) -> bool {
        self.stock > 0 && self.stock <= 5
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct Variant {
    pub size: String,
    pub stock: i64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct User {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub admin: i64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct CartLine {
    pub sku: String,
    pub name: String,
    pub size: String,
    pub price_cents: i64,
    pub quantity: i64,
    pub stock: i64,
}

impl CartLine {
    pub fn subtotal(&self) -> i64 {
        self.price_cents * self.quantity
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct Order {
    pub id: i64,
    pub reference: String,
    pub total_cents: i64,
    pub shipping_cents: i64,
    pub shipping: String,
    pub status: String,
    pub address: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct OrderLine {
    pub sku: String,
    pub name: String,
    pub size: String,
    pub price_cents: i64,
    pub quantity: i64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct TrackingStep {
    pub step: String,
    pub note: String,
    pub at: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct Review {
    pub author: String,
    pub rating: i64,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct Address {
    pub id: i64,
    pub label: String,
    pub text: String,
    pub is_default: i64,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct AdminOrder {
    pub reference: String,
    pub status: String,
    pub total_cents: i64,
    pub created_at: String,
    pub customer: String,
    pub email: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "native", derive(sqlx::FromRow))]
#[cfg_attr(feature = "edge", derive(serde::Deserialize))]
pub struct AdminCustomer {
    pub name: String,
    pub email: String,
    pub admin: i64,
    pub orders: i64,
    pub total_cents: i64,
}

#[derive(Debug, Clone, Default)]
pub struct AdminStats {
    pub products: i64,
    pub orders: i64,
    pub customers: i64,
    pub subscribers: i64,
    pub alerts: i64,
    pub revenue_cents: i64,
}

// --- shipping and money

/// Free shipping above this; the threshold is quoted in the header banner.
pub const FREE_SHIPPING_CENTS: i64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShippingMode {
    pub key: &'static str,
    pub name: &'static str,
    pub delay: &'static str,
    pub cents: i64,
}

pub const SHIPPING_MODES: [ShippingMode; 2] = [
    ShippingMode { key: "standard", name: "Standard", delay: "3 à 5 jours ouvrés", cents: 490 },
    ShippingMode { key: "express", name: "Express", delay: "24 à 48 heures", cents: 1_190 },
];

pub fn shipping_mode(key: &str) -> ShippingMode {
    SHIPPING_MODES.iter().copied().find(|m| m.key == key).unwrap_or(SHIPPING_MODES[0])
}

/// Shipping is free once the basket passes the threshold.
pub fn shipping_cents(subtotal: i64, key: &str) -> i64 {
    if subtotal >= FREE_SHIPPING_CENTS { 0 } else { shipping_mode(key).cents }
}

/// Prices are integers everywhere; this is the only place they become text.
pub fn format_price(cents: i64) -> String {
    format!("{},{:02}\u{a0}€", cents / 100, (cents % 100).abs())
}

// Shared by both backends.
const PRODUCT_FIELDS: &str =
    "sku, name, summary, detail, price_cents, stock, category, is_new, material, hidden";
const ORDER_FIELDS: &str =
    "id, reference, total_cents, shipping_cents, shipping, status, address, created_at";

fn order_reference() -> String {
    const ALPHABET: &[u8] = b"ACDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut raw = [0u8; 6];
    rand::fill(&mut raw);
    let suffix: String =
        raw.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect();
    format!("BER-{suffix}")
}
