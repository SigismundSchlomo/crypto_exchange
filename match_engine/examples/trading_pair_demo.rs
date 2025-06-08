//! Trading Pair Demo
//! 
//! This example demonstrates how to use the TradingPair struct to:
//! - Create a new trading pair with simplified channel architecture
//! - Subscribe to market data and error events
//! - Place and cancel orders with immediate engine restart capability
//! - Handle engine events and errors without restart delays

use match_engine::{TradingPair, Order, Side, MarketEvent, EngineError};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    println!("🚀 Starting Trading Pair Demo for BTC/USD");

    // Create a new trading pair
    let trading_pair = TradingPair::new("BTC/USD".to_string());
    
    // Subscribe to market data events
    let mut market_data_rx = trading_pair.subscribe_to_market_data();
    
    // Subscribe to error events
    let mut error_rx = trading_pair.subscribe_to_errors();
    
    // Spawn task to handle market data events
    let market_data_task = tokio::spawn(async move {
        println!("📊 Market data handler started");
        while let Ok(event) = market_data_rx.recv().await {
            match event {
                MarketEvent::Trade(trade) => {
                    println!("💰 Trade executed: ID={}, Price={}, Amount={}", 
                            trade.id, trade.price, trade.amount);
                }
                MarketEvent::OrderPlaced(order_id) => {
                    println!("📝 Order placed: ID={}", order_id);
                }
                MarketEvent::OrderCancelled(order_id) => {
                    println!("❌ Order cancelled: ID={}", order_id);
                }
                MarketEvent::OrderFilled(order_id) => {
                    println!("✅ Order filled: ID={}", order_id);
                }
                MarketEvent::OrderPartiallyFilled(order_id) => {
                    println!("🔄 Order partially filled: ID={}", order_id);
                }
                MarketEvent::OrderUpdated(order_id) => {
                    println!("🔄 Order updated: ID={}", order_id);
                }
            }
        }
        println!("📊 Market data handler stopped");
    });
    
    // Spawn task to handle error events
    let error_task = tokio::spawn(async move {
        println!("🚨 Error handler started");
        while let Ok(error) = error_rx.recv().await {
            match error {
                EngineError::OrderNotFound(order_id) => {
                    println!("❌ Error: Order not found: {}", order_id);
                }
                EngineError::DuplicateOrderId(order_id) => {
                    println!("❌ Error: Duplicate order ID: {}", order_id);
                }
                EngineError::Processing(msg) => {
                    println!("❌ Error: Processing error: {}", msg);
                }
            }
        }
        println!("🚨 Error handler stopped");
    });

    // Give the engine a moment to start up
    sleep(Duration::from_millis(100)).await;

    println!("\n📈 Demo: Placing orders and executing trades");

    // Place a buy order (bid)
    println!("\n1. Placing buy order: ID=1, Price=50000, Amount=100");
    let buy_order = Order::new(1, Side::Bid, 50000, 100);
    trading_pair.place_order(buy_order).await?;
    
    sleep(Duration::from_millis(100)).await;

    // Place a sell order at a higher price (no match)
    println!("\n2. Placing sell order: ID=2, Price=51000, Amount=50 (no match expected)");
    let sell_order_high = Order::new(2, Side::Ask, 51000, 50);
    trading_pair.place_order(sell_order_high).await?;
    
    sleep(Duration::from_millis(100)).await;

    // Place another buy order at a higher price (no match with existing sell)
    println!("\n3. Placing buy order: ID=3, Price=50500, Amount=75 (no match expected)");
    let buy_order_2 = Order::new(3, Side::Bid, 50500, 75);
    trading_pair.place_order(buy_order_2).await?;
    
    sleep(Duration::from_millis(100)).await;

    // Place a sell order that should match with the highest buy order
    println!("\n4. Placing sell order: ID=4, Price=50500, Amount=50 (should match!)");
    let sell_order_match = Order::new(4, Side::Ask, 50500, 50);
    trading_pair.place_order(sell_order_match).await?;
    
    sleep(Duration::from_millis(100)).await;

    // Place a large sell order that should match with multiple buy orders
    println!("\n5. Placing large sell order: ID=5, Price=50000, Amount=200 (should match multiple!)");
    let large_sell_order = Order::new(5, Side::Ask, 50000, 200);
    trading_pair.place_order(large_sell_order).await?;
    
    sleep(Duration::from_millis(100)).await;

    // Try to cancel an order
    println!("\n6. Attempting to cancel order ID=2");
    trading_pair.cancel_order(2).await?;
    
    sleep(Duration::from_millis(100)).await;

    // Try to cancel a non-existent order (should generate an error)
    println!("\n7. Attempting to cancel non-existent order ID=999 (should error)");
    trading_pair.cancel_order(999).await?;
    
    sleep(Duration::from_millis(100)).await;

    // Place orders with the same ID (should generate an error)
    println!("\n8. Placing order with duplicate ID=1 (should error)");
    let duplicate_order = Order::new(1, Side::Bid, 49000, 50);
    trading_pair.place_order(duplicate_order).await?;

    // Wait for all events to be processed
    println!("\n⏳ Waiting for all events to be processed...");
    sleep(Duration::from_secs(2)).await;

    println!("\n🎯 Demo completed! Check the output above for market events and errors.");
    println!("💡 The trading pair will continue running in the background.");
    println!("🔄 The engine has automatic immediate restart on panic functionality.");
    println!("⚡ No restart delays - maximum responsiveness and throughput.");

    // Keep the demo running for a bit longer to see any remaining events
    sleep(Duration::from_secs(1)).await;

    // Gracefully abort the tasks
    market_data_task.abort();
    error_task.abort();

    println!("\n👋 Demo finished!");
    
    Ok(())
}