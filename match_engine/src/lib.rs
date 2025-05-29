mod engine;
mod errors;
mod types;

// Core engine and matching functionality
pub use engine::{MatchingEngine, Order, Side, Trade};

// Error handling
pub use errors::{EngineError, IntoAnyhow};

// Additional types for advanced usage
pub use types::InternalEvents;

/// Type alias for order identifiers
pub type OrderId = u64;

/// Result type for engine operations
pub type EngineResult<T> = Result<T, EngineError>;

//Here the Trading Pair  lives for now
// This object must have task state, which contains
// - The trading pair symbol
// - The external communication channels
// - The in memory storage for the order's and trades history.
//
// The trading pair is running a blocking task of the matching engine and communicate with it by crossbeam channels.
// Also, this structure is running a regular task, that are works with the engine passing the events.
// The trading pair is storing a L1 and L2 market data.
//
// The trading pair should provide an interface to communicate with engine - add order, cancel order, get market data.
//
// L1 market data
// - highest bid price.
// - bid size on that price
// - lowest ask price
// - ask size on that price
// - last trade
//
// L2 market data
// - highest bid prices - somehow get info about the closest price levels
// - bid sizes
// - lowest ask prices
// - ask sizes
// - last trades
//
// For market data we use matching engine events. Engine emits events on each update of the order book
// So we can store the market data in the trading pair using appropriate data structures, optimised for fast access and updates.
// For example, just vectors, to easily update the market data.
//
// The Pair struct must validate the orders
// The Bid and Ask orders must be validated differently, because they have different precision.
