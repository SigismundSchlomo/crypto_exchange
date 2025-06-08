use anyhow::Result;
use std::panic;
use tokio::sync::{broadcast, mpsc};

use crate::engine::{MarketEvent, MatchingEngine, Order, OrderEvent};
use crate::errors::EngineError;

/// Trading Pair Implementation
///
/// This struct manages a trading pair with the following responsibilities:
/// - Manages the matching engine in a separate blocking task
/// - Handles order routing and validation
/// - Processes market data events and broadcasts them
/// - Manages error handling and logging
/// - Provides external interfaces for order management and market data subscription
///
/// ## Architecture
///
/// The TradingPair uses multiple async tasks:
/// 1. **Order Forwarding Task**: Routes external orders to the engine
/// 2. **Market Data Handler**: Processes engine events, adds metadata, and broadcasts
/// 3. **Error Handler**: Logs errors and broadcasts them to subscribers
/// 4. **Engine Task**: Runs the core matching engine in a blocking task with panic recovery
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
        mut order_receiver: mpsc::UnboundedReceiver<OrderEvent>,
        market_data_sender: broadcast::Sender<MarketEvent>,
        error_sender: broadcast::Sender<EngineError>,
    ) {
        // Create channels for communication with the engine
        // Run the tokio::blocking task with engine, pass the channels to the engine
        // The blocking task should be wrapped with catch_unwind to handle panics
        // Restart the engine on panic

        // Create a channel that will be used to signal engine restarts
        let (restart_tx, mut restart_rx) =
            mpsc::unbounded_channel::<mpsc::UnboundedSender<OrderEvent>>();

        // Task 1: Persistent order forwarding
        tokio::spawn(async move {
            let mut current_engine_tx: Option<mpsc::UnboundedSender<OrderEvent>> = None;

            loop {
                tokio::select! {
                    // Receive new engine sender on restart
                    engine_tx = restart_rx.recv() => {
                        match engine_tx {
                            Some(tx) => {
                                current_engine_tx = Some(tx);
                                tracing::debug!("Updated engine order sender");
                            }
                            None => {
                                tracing::info!("Restart channel closed");
                                break;
                            }
                        }
                    }
                    // Forward orders to current engine
                    order = order_receiver.recv() => {
                        match order {
                            Some(order_event) => {
                                if let Some(ref tx) = current_engine_tx {
                                    if let Err(_) = tx.send(order_event) {
                                        tracing::error!("Failed to send order to engine - channel closed");
                                        current_engine_tx = None;
                                    }
                                } else {
                                    tracing::warn!("Dropping order - no active engine");
                                }
                            }
                            None => {
                                tracing::info!("Order receiver closed");
                                break;
                            }
                        }
                    }
                }
            }
        });

        // Task 2: Engine management with restart capability
        let symbol_clone = symbol.clone();
        let market_data_sender_clone = market_data_sender.clone();
        let error_sender_clone = error_sender.clone();

        tokio::spawn(async move {
            loop {
                // Create fresh channels for each engine restart
                let (engine_order_tx, engine_order_rx) = mpsc::unbounded_channel::<OrderEvent>();
                let (engine_market_tx, mut engine_market_rx) =
                    mpsc::unbounded_channel::<MarketEvent>();
                let (engine_error_tx, mut engine_error_rx) =
                    mpsc::unbounded_channel::<EngineError>();

                // Notify the order forwarder about the new engine sender
                if let Err(_) = restart_tx.send(engine_order_tx) {
                    tracing::error!("Failed to send engine sender to forwarder");
                    break;
                }

                // Start market data handler for this engine instance
                let market_data_sender_task = market_data_sender_clone.clone();
                let market_data_task = tokio::spawn(async move {
                    while let Some(market_event) = engine_market_rx.recv().await {
                        // Add timestamp and metadata to market data here if needed
                        let enhanced_event = market_event; // TODO: Add metadata processing

                        // Broadcast to external subscribers
                        if let Err(_) = market_data_sender_task.send(enhanced_event) {
                            // No subscribers, which is fine
                        }

                        // TODO: Store market data in batches for database persistence
                    }
                });

                // Start error handler for this engine instance
                let error_sender_task = error_sender_clone.clone();
                let error_task = tokio::spawn(async move {
                    while let Some(error) = engine_error_rx.recv().await {
                        tracing::error!("Engine error: {:?}", error);

                        // Broadcast error to external subscribers
                        if let Err(_) = error_sender_task.send(error) {
                            // No subscribers, which is fine
                        }
                    }
                });

                // Start the engine in a blocking task
                let symbol_engine = symbol_clone.clone();
                let engine_task = tokio::task::spawn_blocking(move || {
                    let result = panic::catch_unwind(|| {
                        let mut engine = MatchingEngine::new();
                        tracing::info!("Starting matching engine for symbol: {}", symbol_engine);

                        // Run the engine directly with tokio channels
                        MatchingEngine::run(
                            &mut engine,
                            engine_order_rx,
                            engine_market_tx,
                            engine_error_tx,
                        );
                    });
                    result
                });

                // Wait for engine to finish or panic
                match engine_task.await {
                    Ok(Ok(_)) => {
                        tracing::info!("Engine exited normally for symbol: {}", symbol_clone);
                        break;
                    }
                    Ok(Err(panic_info)) => {
                        tracing::error!(
                            "Engine panicked for symbol: {}, restarting... Panic: {:?}",
                            symbol_clone,
                            panic_info
                        );
                    }
                    Err(join_error) => {
                        tracing::error!(
                            "Engine task failed for symbol: {}, restarting... Error: {:?}",
                            symbol_clone,
                            join_error
                        );
                    }
                }

                // Cancel helper tasks before restarting
                market_data_task.abort();
                error_task.abort();
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