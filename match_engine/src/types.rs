use std::cmp::Ordering;

/// Internal events used by the matching engine for state management
#[derive(Debug, Clone)]
pub enum InternalEvents {
    /// Order book state changed
    BookStateChanged,
    /// Price level was created
    PriceLevelCreated { price: u64 },
    /// Price level was removed
    PriceLevelRemoved { price: u64 },
    /// Order was added to book
    OrderAddedToBook { order_id: u64, price: u64 },
    /// Order was removed from book
    OrderRemovedFromBook { order_id: u64, price: u64 },
}

/// Configuration for order book behavior
#[derive(Debug, Clone)]
pub struct OrderBookConfig {
    /// Maximum number of orders per price level
    pub max_orders_per_level: Option<usize>,
    /// Whether to automatically clean up cancelled orders
    pub auto_cleanup_cancelled: bool,
    /// Minimum order amount allowed
    pub min_order_amount: u64,
    /// Maximum order amount allowed
    pub max_order_amount: Option<u64>,
}

impl Default for OrderBookConfig {
    fn default() -> Self {
        Self {
            max_orders_per_level: None,
            auto_cleanup_cancelled: true,
            min_order_amount: 1,
            max_order_amount: None,
        }
    }
}
