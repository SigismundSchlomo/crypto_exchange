//! Basic usage example for the match_engine library
//! 
//! This example demonstrates the core functionality of the matching engine:
//! - Creating orders
//! - Processing matches
//! - Canceling orders
//! - Inspecting order book state

use match_engine::{MatchingEngine, Event, Side, OrderBookStats};

fn main() {
    println!("=== Match Engine Library Example ===\n");

    // Create a new matching engine
    let mut engine = MatchingEngine::new();
    println!("Created new matching engine");

    // Add some buy orders (bids)
    println!("\n--- Adding Buy Orders ---");
    let trades = engine.process_event(Event::NewOrder {
        id: 1,
        side: Side::Bid,
        price: 100,
        amount: 50,
    });
    println!("Added buy order: ID=1, Price=100, Amount=50. Trades: {}", trades.len());

    let trades = engine.process_event(Event::NewOrder {
        id: 2,
        side: Side::Bid,
        price: 99,
        amount: 30,
    });
    println!("Added buy order: ID=2, Price=99, Amount=30. Trades: {}", trades.len());

    let trades = engine.process_event(Event::NewOrder {
        id: 3,
        side: Side::Bid,
        price: 101,
        amount: 20,
    });
    println!("Added buy order: ID=3, Price=101, Amount=20. Trades: {}", trades.len());

    // Add some sell orders (asks)
    println!("\n--- Adding Sell Orders ---");
    let trades = engine.process_event(Event::NewOrder {
        id: 4,
        side: Side::Ask,
        price: 105,
        amount: 25,
    });
    println!("Added sell order: ID=4, Price=105, Amount=25. Trades: {}", trades.len());

    let trades = engine.process_event(Event::NewOrder {
        id: 5,
        side: Side::Ask,
        price: 103,
        amount: 40,
    });
    println!("Added sell order: ID=5, Price=103, Amount=40. Trades: {}", trades.len());

    // Display order book state
    println!("\n--- Order Book State ---");
    display_order_book_state(&engine);

    // Add a sell order that will match with existing bids
    println!("\n--- Adding Matching Sell Order ---");
    let trades = engine.process_event(Event::NewOrder {
        id: 6,
        side: Side::Ask,
        price: 100,
        amount: 30,
    });
    
    println!("Added sell order: ID=6, Price=100, Amount=30. Generated {} trades:", trades.len());
    for trade in &trades {
        println!("  Trade: Bid={}, Ask={}, Price={}, Amount={}", 
                trade.bid_id, trade.ask_id, trade.price, trade.amount);
    }

    // Display updated order book state
    println!("\n--- Updated Order Book State ---");
    display_order_book_state(&engine);

    // Add a buy order that will match with existing asks
    println!("\n--- Adding Matching Buy Order ---");
    let trades = engine.process_event(Event::NewOrder {
        id: 7,
        side: Side::Bid,
        price: 105,
        amount: 15,
    });
    
    println!("Added buy order: ID=7, Price=105, Amount=15. Generated {} trades:", trades.len());
    for trade in &trades {
        println!("  Trade: Bid={}, Ask={}, Price={}, Amount={}", 
                trade.bid_id, trade.ask_id, trade.price, trade.amount);
    }

    // Cancel an order
    println!("\n--- Canceling Order ---");
    let trades = engine.process_event(Event::CancelOrder { id: 2 });
    println!("Canceled order ID=2. Trades: {}", trades.len());

    // Display final order book state
    println!("\n--- Final Order Book State ---");
    display_order_book_state(&engine);

    // Demonstrate additional functionality
    println!("\n--- Additional Functionality ---");
    
    // Get specific order
    if let Some(order) = engine.get_order_by_id(1) {
        println!("Order 1: Price={}, Amount={}, Side={:?}", order.price, order.amount, order.side);
    } else {
        println!("Order 1 not found");
    }

    // Get price levels
    println!("\nBid levels:");
    for (price, amount) in engine.get_price_levels(Side::Bid) {
        println!("  Price: {}, Total Amount: {}", price, amount);
    }

    println!("\nAsk levels:");
    for (price, amount) in engine.get_price_levels(Side::Ask) {
        println!("  Price: {}, Total Amount: {}", price, amount);
    }

    // Cancel all orders on one side
    println!("\n--- Canceling All Bid Orders ---");
    let cancelled_orders = engine.cancel_all_orders(Some(Side::Bid));
    println!("Cancelled {} bid orders: {:?}", cancelled_orders.len(), cancelled_orders);

    // Clean up cancelled orders
    engine.cleanup_cancelled_orders();
    println!("Cleaned up cancelled orders");

    // Final state
    println!("\n--- Final State After Cleanup ---");
    display_order_book_state(&engine);

    println!("\n=== Example Complete ===");
}

fn display_order_book_state(engine: &MatchingEngine) {
    let stats = engine.get_stats();
    
    println!("Order Book Statistics:");
    println!("  Total Orders: {}", stats.total_orders);
    println!("  Bid Levels: {}", stats.bid_levels);
    println!("  Ask Levels: {}", stats.ask_levels);
    println!("  Bid Volume: {}", stats.bid_volume);
    println!("  Ask Volume: {}", stats.ask_volume);
    
    if let Some(spread) = stats.spread {
        println!("  Spread: {}", spread);
    } else {
        println!("  Spread: N/A (no matching orders)");
    }
    
    if let Some(best_bid) = stats.best_bid {
        println!("  Best Bid: {}", best_bid);
    } else {
        println!("  Best Bid: None");
    }
    
    if let Some(best_ask) = stats.best_ask {
        println!("  Best Ask: {}", best_ask);
    } else {
        println!("  Best Ask: None");
    }
}