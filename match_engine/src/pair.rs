use anyhow::Result;
use std::panic;
use tokio::sync::{broadcast, mpsc};

use crate::engine::{MarketEvent, MatchingEngine, Order, OrderEvent};
use crate::errors::EngineError;

/// Trading Pair Implementation
///
/// This struct manages a trading pair with the following responsibilities:
/// - Manages the matching engine in a separate blocking task
/// - Handles order routing to the engine
/// - Processes market data events and broadcasts them
/// - Manages error handling and logging
/// - Provides external interfaces for order management and market data subscription
///
/// ## Architecture
///
/// The TradingPair uses multiple async tasks:
/// 1. **Market Data Handler**: Processes engine events, adds metadata, and broadcasts
/// 2. **Error Handler**: Logs errors and broadcasts them to subscribers
/// 3. **Engine Task**: Runs the core matching engine in a blocking task with panic recovery
///
/// ## Usage Example
///
/// ```rust
/// use match_engine::{TradingPair, Order, Side};
///
/// #[tokio::main]
/// async fn main() {
///     // Create a new trading pair
///     let trading_pair = TradingPair::new("BTC/USD".to_string());
///
///     // Subscribe to market data
///     let mut market_data = trading_pair.subscribe_to_market_data();
///
///     // Subscribe to errors
///     let mut errors = trading_pair.subscribe_to_errors();
///
///     // Place a buy order
///     let order = Order::new(1, Side::Bid, 50000, 100);
///     trading_pair.place_order(order).await.unwrap();
///
///     // Cancel an order
///     trading_pair.cancel_order(1).await.unwrap();
///
///     // Listen for market events
///     tokio::spawn(async move {
///         while let Ok(event) = market_data.recv().await {
///             println!("Market event: {:?}", event);
///         }
///     });
///
///     // Listen for errors
///     tokio::spawn(async move {
///         while let Ok(error) = errors.recv().await {
///             println!("Engine error: {:?}", error);
///         }
///     });
/// }
/// ```
pub struct TradingPair {
    pub symbol: String,
    // External communication channels
    pub order_sender: mpsc::UnboundedSender<OrderEvent>,
    pub market_data_receiver: broadcast::Receiver<MarketEvent>,
    pub error_receiver: broadcast::Receiver<EngineError>,

    // Internal channels for broadcasting
    market_data_sender: broadcast::Sender<MarketEvent>,
    error_sender: broadcast::Sender<EngineError>,
}

impl TradingPair {
    /// Creates a new TradingPair instance for the given trading symbol.
    ///
    /// This function:
    /// 1. Sets up communication channels for orders, market data, and errors
    /// 2. Starts the matching engine in a blocking task with panic recovery
    /// 3. Initializes market data and error handling tasks
    /// 4. Returns a TradingPair instance ready for use
    ///
    /// # Arguments
    ///
    /// * `symbol` - The trading pair symbol (e.g., "BTC/USD", "ETH/BTC")
    ///
    /// # Returns
    ///
    /// A configured TradingPair instance with all tasks running
    pub fn new(symbol: String) -> Self {
        let (order_sender, order_receiver) = mpsc::unbounded_channel::<OrderEvent>();

        // Create broadcast channels for market data and errors
        let (market_data_sender, market_data_receiver) = broadcast::channel::<MarketEvent>(1000);
        let (error_sender, error_receiver) = broadcast::channel::<EngineError>(100);

        let trading_pair = TradingPair {
            symbol: symbol.clone(),
            order_sender,
            market_data_receiver,
            error_receiver,
            market_data_sender: market_data_sender.clone(),
            error_sender: error_sender.clone(),
        };

        // Start the engine
        Self::start_engine(symbol, order_receiver, market_data_sender, error_sender);

        trading_pair
    }

    fn start_engine(
        symbol: String,
        order_receiver: mpsc::UnboundedReceiver<OrderEvent>,
        market_data_sender: broadcast::Sender<MarketEvent>,
        error_sender: broadcast::Sender<EngineError>,
    ) {
        // Engine management with restart capability
        let symbol_clone = symbol.clone();
        let market_data_sender_clone = market_data_sender.clone();
        let error_sender_clone = error_sender.clone();

        // Create channels for engine communication
        let (engine_market_tx, mut engine_market_rx) = mpsc::unbounded_channel::<MarketEvent>();
        let (engine_error_tx, mut engine_error_rx) = mpsc::unbounded_channel::<EngineError>();

        // Start market data handler
        let market_data_sender_task = market_data_sender_clone.clone();
        tokio::spawn(async move {
            while let Some(market_event) = engine_market_rx.recv().await {
                // Add timestamp and metadata to market data here if needed
                let enhanced_event = market_event; // TODO: Add metadata processing

                // Broadcast to external subscribers
                if let Err(_) = market_data_sender_task.send(enhanced_event) {
                    // No subscribers, which is fine
                }
            }
        });

        // Start error handler
        let error_sender_task = error_sender_clone.clone();
        tokio::spawn(async move {
            while let Some(error) = engine_error_rx.recv().await {
                tracing::error!("Engine error: {:?}", error);

                // Broadcast error to external subscribers
                if let Err(_) = error_sender_task.send(error) {
                    // No subscribers, which is fine
                }
            }
        });

        // Start the engine in a blocking task
        tokio::task::spawn_blocking(move || {
            let result = panic::catch_unwind(|| {
                let mut engine = MatchingEngine::new();
                tracing::info!("Starting matching engine for symbol: {}", symbol_clone);

                // Run the engine directly with external order receiver
                MatchingEngine::run(
                    &mut engine,
                    order_receiver,
                    engine_market_tx,
                    engine_error_tx,
                );
            });

            match result {
                Ok(_) => {
                    tracing::info!("Engine exited normally for symbol: {}", symbol_clone);
                }
                Err(panic_info) => {
                    tracing::error!(
                        "Engine panicked for symbol: {}. Panic: {:?}",
                        symbol_clone,
                        panic_info
                    );
                }
            }
        });
    }

    /// Places a new order in the trading pair.
    ///
    /// # Arguments
    ///
    /// * `order` - The order to place
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the order was successfully queued, or an error if the channel is closed
    pub async fn place_order(&self, order: Order) -> Result<()> {
        self.order_sender
            .send(OrderEvent::New(Box::new(order)))
            .map_err(|_| anyhow::anyhow!("Failed to send order - channel closed"))?;
        Ok(())
    }

    /// Cancels an existing order by its ID.
    ///
    /// # Arguments
    ///
    /// * `order_id` - The ID of the order to cancel
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the cancel request was successfully queued, or an error if the channel is closed
    pub async fn cancel_order(&self, order_id: u64) -> Result<()> {
        self.order_sender
            .send(OrderEvent::Cancel(order_id))
            .map_err(|_| anyhow::anyhow!("Failed to send cancel order - channel closed"))?;
        Ok(())
    }

    /// Subscribe to market data events.
    ///
    /// # Returns
    ///
    /// A broadcast receiver that will receive all market events including trades,
    /// order placements, cancellations, and fills.
    pub fn subscribe_to_market_data(&self) -> broadcast::Receiver<MarketEvent> {
        self.market_data_sender.subscribe()
    }

    /// Subscribe to engine error events.
    ///
    /// # Returns
    ///
    /// A broadcast receiver that will receive all engine errors such as
    /// order not found, duplicate order ID, and processing errors.
    pub fn subscribe_to_errors(&self) -> broadcast::Receiver<EngineError> {
        self.error_sender.subscribe()
    }
}

#[cfg(test)]
mod tests {
    //! Comprehensive tests for the TradingPair implementation
    //!
    //! These tests cover the following scenarios:
    //! 
    //! ## Basic Functionality
    //! - `test_trading_pair_creation`: Verifies basic creation and symbol assignment
    //! - `test_place_single_order`: Tests placing a single order and receiving placement events
    //! 
    //! ## Order Matching and Trading
    //! - `test_place_matching_orders`: Tests full order matching with complete fills
    //! - `test_partial_order_fill`: Tests partial order fills when orders don't fully match
    //! 
    //! ## Order Management
    //! - `test_order_cancellation`: Tests successful order cancellation
    //! - `test_cancel_nonexistent_order`: Tests error handling for invalid cancellations
    //! 
    //! ## Complex Market Scenarios
    //! - `test_multiple_orders_complex_scenario`: Tests complex multi-order scenarios with different price levels
    //! - `test_concurrent_operations`: Tests concurrent order placement using multiple tasks
    //! - `test_engine_stability_with_many_operations`: Tests engine stability under high load
    //! 
    //! ## Event Broadcasting and Subscription
    //! - `test_market_data_subscription`: Tests multiple subscribers receiving market data events
    //! - `test_error_subscription`: Tests multiple subscribers receiving error events
    //! 
    //! ## Test Architecture Notes
    //! - All tests use tokio runtime for async execution
    //! - No startup delays needed due to simplified architecture
    //! - Fast execution with 100ms timeouts for async operations
    //! - Market events are verified to ensure proper order flow simulation
    //! - Error conditions are tested to ensure robust error handling
    //! - TradingPair focuses solely on order matching and event broadcasting

    use super::*;
    use crate::engine::{MarketEvent, Order, Side};
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_trading_pair_creation() {
        let trading_pair = TradingPair::new("BTC/USD".to_string());
        assert_eq!(trading_pair.symbol, "BTC/USD");
    }

    #[tokio::test]
    async fn test_place_single_order() {
        let trading_pair = TradingPair::new("BTC/USD".to_string());
        let mut market_data = trading_pair.subscribe_to_market_data();



        // Place a buy order
        let order = Order::new(1, Side::Bid, 50000, 100);
        trading_pair.place_order(order).await.unwrap();

        // Wait for the order to be processed
        match tokio::time::timeout(Duration::from_millis(100), market_data.recv()).await {
            Ok(Ok(MarketEvent::OrderPlaced(order_id))) => {
                assert_eq!(order_id, 1);
            }
            Ok(Ok(event)) => panic!("Unexpected event: {:?}", event),
            Ok(Err(e)) => panic!("Market data receive error: {:?}", e),
            Err(_) => panic!("Timeout waiting for market data"),
        }
    }

    #[tokio::test]
    async fn test_place_matching_orders() {
        let trading_pair = TradingPair::new("BTC/USD".to_string());
        let mut market_data = trading_pair.subscribe_to_market_data();



        // Place a sell order first
        let sell_order = Order::new(1, Side::Ask, 50000, 100);
        trading_pair.place_order(sell_order).await.unwrap();

        // Wait for sell order to be placed
        match tokio::time::timeout(Duration::from_millis(100), market_data.recv()).await {
            Ok(Ok(MarketEvent::OrderPlaced(order_id))) => {
                assert_eq!(order_id, 1);
            }
            _ => panic!("Expected OrderPlaced event for sell order"),
        }

        // Place a matching buy order
        let buy_order = Order::new(2, Side::Bid, 50000, 100);
        trading_pair.place_order(buy_order).await.unwrap();

        // Collect events from the trade
        let mut events = Vec::new();
        for _ in 0..3 {
            match tokio::time::timeout(Duration::from_millis(100), market_data.recv()).await {
                Ok(Ok(event)) => events.push(event),
                Ok(Err(e)) => panic!("Market data receive error: {:?}", e),
                Err(_) => break, // Timeout is expected when no more events
            }
        }

        // Verify we got the expected events: Trade and OrderFilled
        assert!(events.len() >= 2);
        assert!(events.iter().any(|e| matches!(e, MarketEvent::Trade(_))));
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderFilled(1))));
    }

    #[tokio::test]
    async fn test_partial_order_fill() {
        let trading_pair = TradingPair::new("BTC/USD".to_string());
        let mut market_data = trading_pair.subscribe_to_market_data();



        // Place a large sell order
        let sell_order = Order::new(1, Side::Ask, 50000, 200);
        trading_pair.place_order(sell_order).await.unwrap();

        // Wait for sell order to be placed
        match tokio::time::timeout(Duration::from_millis(100), market_data.recv()).await {
            Ok(Ok(MarketEvent::OrderPlaced(order_id))) => {
                assert_eq!(order_id, 1);
            }
            _ => panic!("Expected OrderPlaced event for sell order"),
        }

        // Place a smaller buy order that will partially fill the sell order
        let buy_order = Order::new(2, Side::Bid, 50000, 100);
        trading_pair.place_order(buy_order).await.unwrap();

        // Collect events from the partial trade
        let mut events = Vec::new();
        for _ in 0..3 {
            match tokio::time::timeout(Duration::from_millis(100), market_data.recv()).await {
                Ok(Ok(event)) => events.push(event),
                Ok(Err(e)) => panic!("Market data receive error: {:?}", e),
                Err(_) => break,
            }
        }

        // Verify we got a trade and a partially filled order
        assert!(events.iter().any(|e| matches!(e, MarketEvent::Trade(_))));
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderPartiallyFilled(1))));
    }

    #[tokio::test]
    async fn test_order_cancellation() {
        let trading_pair = TradingPair::new("BTC/USD".to_string());
        let mut market_data = trading_pair.subscribe_to_market_data();



        // Place an order
        let order = Order::new(1, Side::Bid, 50000, 100);
        trading_pair.place_order(order).await.unwrap();

        // Wait for order to be placed
        match tokio::time::timeout(Duration::from_millis(100), market_data.recv()).await {
            Ok(Ok(MarketEvent::OrderPlaced(order_id))) => {
                assert_eq!(order_id, 1);
            }
            _ => panic!("Expected OrderPlaced event"),
        }

        // Cancel the order
        trading_pair.cancel_order(1).await.unwrap();

        // Wait for cancellation event
        match tokio::time::timeout(Duration::from_millis(100), market_data.recv()).await {
            Ok(Ok(MarketEvent::OrderCancelled(order_id))) => {
                assert_eq!(order_id, 1);
            }
            Ok(Ok(event)) => panic!("Unexpected event: {:?}", event),
            Ok(Err(e)) => panic!("Market data receive error: {:?}", e),
            Err(_) => panic!("Timeout waiting for cancellation event"),
        }
    }

    #[tokio::test]
    async fn test_cancel_nonexistent_order() {
        let trading_pair = TradingPair::new("BTC/USD".to_string());
        let mut error_receiver = trading_pair.subscribe_to_errors();



        // Try to cancel a non-existent order
        trading_pair.cancel_order(999).await.unwrap();

        // Wait for error event
        match tokio::time::timeout(Duration::from_millis(100), error_receiver.recv()).await {
            Ok(Ok(EngineError::OrderNotFound(order_id))) => {
                assert_eq!(order_id, 999);
            }
            Ok(Ok(error)) => panic!("Unexpected error: {:?}", error),
            Ok(Err(e)) => panic!("Error receiver error: {:?}", e),
            Err(_) => panic!("Timeout waiting for error event"),
        }
    }

    #[tokio::test]
    async fn test_multiple_orders_complex_scenario() {
        let trading_pair = TradingPair::new("ETH/USD".to_string());
        let mut market_data = trading_pair.subscribe_to_market_data();



        // Place multiple orders at different price levels
        let orders = vec![
            Order::new(1, Side::Ask, 3000, 50),  // Sell at 3000
            Order::new(2, Side::Ask, 3100, 75),  // Sell at 3100
            Order::new(3, Side::Bid, 2900, 100), // Buy at 2900
            Order::new(4, Side::Bid, 2800, 150), // Buy at 2800
        ];

        // Place all orders
        for order in orders {
            trading_pair.place_order(order).await.unwrap();
        }

        // Collect placement events
        let mut placement_events = 0;
        for _ in 0..4 {
            match tokio::time::timeout(Duration::from_millis(100), market_data.recv()).await {
                Ok(Ok(MarketEvent::OrderPlaced(_))) => placement_events += 1,
                Ok(Ok(event)) => panic!("Unexpected event during placement: {:?}", event),
                Ok(Err(e)) => panic!("Market data receive error: {:?}", e),
                Err(_) => break,
            }
        }
        assert_eq!(placement_events, 4);

        // Place a buy order that matches the lowest ask
        let matching_order = Order::new(5, Side::Bid, 3000, 30);
        trading_pair.place_order(matching_order).await.unwrap();

        // Wait for trade events
        let mut events = Vec::new();
        for _ in 0..5 {
            match tokio::time::timeout(Duration::from_millis(100), market_data.recv()).await {
                Ok(Ok(event)) => events.push(event),
                Ok(Err(e)) => panic!("Market data receive error: {:?}", e),
                Err(_) => break,
            }
        }

        // Should have a trade and partial fill
        assert!(events.iter().any(|e| matches!(e, MarketEvent::Trade(_))));
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderPartiallyFilled(1))));
    }

    #[tokio::test]
    async fn test_concurrent_operations() {
        let trading_pair = TradingPair::new("BTC/USD".to_string());
        let mut market_data = trading_pair.subscribe_to_market_data();



        // Place orders concurrently using Arc to share the trading pair
        let mut handles = Vec::new();
        
        for i in 1..=10 {
            let order_sender = trading_pair.order_sender.clone();
            let handle = tokio::spawn(async move {
                let order = Order::new(i, Side::Bid, 50000 + i * 10, 100);
                order_sender.send(OrderEvent::New(Box::new(order))).unwrap();
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Count the placement events
        let mut placement_count = 0;
        while placement_count < 10 {
            match tokio::time::timeout(Duration::from_millis(100), market_data.recv()).await {
                Ok(Ok(MarketEvent::OrderPlaced(_))) => placement_count += 1,
                Ok(Ok(event)) => panic!("Unexpected event: {:?}", event),
                Ok(Err(e)) => panic!("Market data receive error: {:?}", e),
                Err(_) => break,
            }
        }

        assert_eq!(placement_count, 10);
    }

    #[tokio::test]
    async fn test_market_data_subscription() {
        let trading_pair = TradingPair::new("BTC/USD".to_string());
        
        // Create multiple subscribers
        let mut subscriber1 = trading_pair.subscribe_to_market_data();
        let mut subscriber2 = trading_pair.subscribe_to_market_data();



        // Place an order
        let order = Order::new(1, Side::Bid, 50000, 100);
        trading_pair.place_order(order).await.unwrap();

        // Both subscribers should receive the event
        let event1 = tokio::time::timeout(Duration::from_millis(100), subscriber1.recv())
            .await
            .expect("Timeout")
            .expect("Receive error");

        let event2 = tokio::time::timeout(Duration::from_millis(100), subscriber2.recv())
            .await
            .expect("Timeout")
            .expect("Receive error");

        // Both should be OrderPlaced events
        match (event1, event2) {
            (MarketEvent::OrderPlaced(id1), MarketEvent::OrderPlaced(id2)) => {
                assert_eq!(id1, 1);
                assert_eq!(id2, 1);
            }
            _ => panic!("Expected OrderPlaced events"),
        }
    }

    #[tokio::test]
    async fn test_error_subscription() {
        let trading_pair = TradingPair::new("BTC/USD".to_string());
        let mut error_subscriber1 = trading_pair.subscribe_to_errors();
        let mut error_subscriber2 = trading_pair.subscribe_to_errors();



        // Trigger an error by canceling non-existent order
        trading_pair.cancel_order(999).await.unwrap();

        // Both subscribers should receive the error
        let error1 = tokio::time::timeout(Duration::from_millis(100), error_subscriber1.recv())
            .await
            .expect("Timeout")
            .expect("Receive error");

        let error2 = tokio::time::timeout(Duration::from_millis(100), error_subscriber2.recv())
            .await
            .expect("Timeout")
            .expect("Receive error");

        // Both should be OrderNotFound errors
        match (error1, error2) {
            (EngineError::OrderNotFound(id1), EngineError::OrderNotFound(id2)) => {
                assert_eq!(id1, 999);
                assert_eq!(id2, 999);
            }
            _ => panic!("Expected OrderNotFound errors"),
        }
    }

    #[tokio::test]
    async fn test_engine_stability_with_many_operations() {
        let trading_pair = TradingPair::new("BTC/USD".to_string());
        let mut market_data = trading_pair.subscribe_to_market_data();



        // Perform many operations rapidly
        for i in 1..=50 {
            let order = Order::new(i, Side::Bid, 50000 + (i % 10) * 100, 100);
            trading_pair.place_order(order).await.unwrap();

            // Cancel every 5th order
            if i % 5 == 0 && i > 5 {
                trading_pair.cancel_order(i - 4).await.unwrap();
            }
        }

        // Let the engine process everything
        sleep(Duration::from_millis(100)).await;

        // Count events received (should be many)
        let mut event_count = 0;
        while event_count < 100 {
            match tokio::time::timeout(Duration::from_millis(10), market_data.recv()).await {
                Ok(Ok(_)) => event_count += 1,
                Ok(Err(_)) => break,
                Err(_) => break, // Timeout
            }
        }

        // Should have received a significant number of events
        assert!(event_count > 40, "Expected many events, got {}", event_count);
    }
}