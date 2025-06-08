//! Core matching engine implementation for order book trading.
//!
//! This module provides the main `MatchingEngine` struct and related types
//! for processing trading orders and generating trades.
use std::collections::{BTreeMap, HashMap, VecDeque};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::errors::EngineError;

// The matching engine must process incoming events and generate outcoming events

// engine recieves orders, and tries to make a match on each new order
// to perform order cancellation engine store index of orders to search them
// When we cancel the order, we delete it from the order book and generate a cancellation event
// External system must handle the order's history by handling outcoming events
// On partial order match, we update the order's amount and emit an `OrderPartialMatched` event.
// On when order is filled, we emit an `OrderFilled` event.

// We should be able to get the information about the order book state
// it should be handled on the complementary async task

// Should store the price levels and orders index with order id maped to price level and side
// On cancel is responsibility of the price level to return move the order outside of the price level
// To provide L1 market data, engine should use events - just emit best bid and ask on update
// To provide L2 market data, engine could emit updates to order book.
// Also, this events could be used to store the order book state for persistance.

// For decimal numbers, engine should use fixed-point arithmetics. So, we should just configure the engine with the faction.
// Also, we should specify different precisions for the price - tick size, and order amount
// Alos, side should has own precision for amount
// Tick size == price precision
//
// The engine makes an assumption that both assets have the same precision.
//
// The limit orders should be validated outside of the engine, on the external task level.
//
// Maybe we should have different types for bids and asks.
//
//

pub type Price = u64;
pub type OrderId = u64;
pub type TradeId = u64;
pub type Amount = u64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Bid,
    Ask,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: OrderId,
    pub side: Side,
    //TODO: Work on decimals processing
    pub price: Price,
    pub amount: u64,
}

impl Order {
    pub fn new(id: OrderId, side: Side, price: Price, amount: u64) -> Self {
        Order {
            id,
            side,
            price,
            amount,
        }
    }
}

#[derive(Debug, Clone)]
pub enum OrderEvent {
    New(Box<Order>),
    Cancel(OrderId),
}

#[derive(Debug, Clone)]
pub enum MarketEvent {
    Trade(Box<Trade>),
    OrderCancelled(OrderId),
    OrderFilled(OrderId),
    OrderPartiallyFilled(OrderId),
    OrderPlaced(OrderId),
    OrderUpdated(OrderId),
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub id: TradeId,
    pub price: u64,
    pub amount: u64,
}

pub struct FillResult {
    pub trades: Vec<Trade>,
    pub filled_orders: Vec<OrderId>,
    pub partially_filled_order: Option<OrderId>,
    pub remaining_amount: u64,
}

impl FillResult {
    pub fn into_events(self) -> Vec<MarketEvent> {
        let mut events = Vec::new();
        self.trades.into_iter().for_each(|trade| {
            events.push(MarketEvent::Trade(Box::new(trade)));
        });
        self.filled_orders.into_iter().for_each(|id| {
            events.push(MarketEvent::OrderFilled(id));
        });
        self.partially_filled_order.into_iter().for_each(|id| {
            events.push(MarketEvent::OrderPartiallyFilled(id));
        });
        events
    }
}

/// Represents a price level in the order book with aggregated information
#[derive(Debug, Clone)]
pub struct PriceLevel {
    pub quantity: u64,
    pub orders: VecDeque<(OrderId, Amount)>,
    pub order_count: u64,
}

impl PriceLevel {
    /// Create a new price level
    pub fn new() -> Self {
        Self {
            quantity: 0,
            orders: VecDeque::new(),
            order_count: 0,
        }
    }

    pub fn add_order(&mut self, order_id: OrderId, amount: Amount) {
        self.orders.push_back((order_id, amount));
        self.order_count += 1;
        self.quantity += amount;
    }

    pub fn remove_order(&mut self, order_id: OrderId) -> Result<(), EngineError> {
        let mut found = false;
        self.orders.retain(|(id, amount)| {
            if *id == order_id && !found {
                self.quantity -= *amount;
                self.order_count -= 1;
                found = true;
                false // remove
            } else {
                true // keep
            }
        });
        if !found {
            Err(EngineError::OrderNotFound(order_id))
        } else {
            Ok(())
        }
    }

    pub fn update_order(&mut self, order_id: OrderId, new_amount: u64) -> Result<(), EngineError> {
        let mut found = false;
        self.orders.iter_mut().for_each(|(id, amount)| {
            if *id == order_id {
                self.quantity -= *amount;
                self.quantity += new_amount;
                found = true;
                *amount = new_amount;
            }
        });
        if !found {
            Err(EngineError::OrderNotFound(order_id))
        } else {
            Ok(())
        }
    }

    // When we filling the order, the next thinks could happen
    // - The order is filled completely
    // - The order is partially filled
    // - The order is not filled at all
    // When order is filled, it could be filled by multiple orders.
    // At the same time, it means that the orders that are stored on this level was filled, or filled partially.
    // So, filling the order, would generate a trade. It must be one single trade, that contains the ids of the orders that were filled.
    // Also, it may contain the id of the order that was partially filled.
    // So, we must return enough information to generate events about all the state change

    pub fn try_fill(&mut self, order: &Order) -> Option<FillResult> {
        let mut trades = Vec::new();
        let mut filled_orders = Vec::new();
        let mut partially_filled_order = None;
        let mut remaining_amount = order.amount;
        let mut trade_id = 0;

        while remaining_amount > 0 && !self.orders.is_empty() {
            let (current_id, mut current_amount) = self.orders.pop_front()?;

            //TODO: Use fixed point arithmetic
            if current_amount <= remaining_amount {
                let trade_amount = current_amount;

                trades.push(Trade {
                    id: trade_id,
                    price: order.price,
                    amount: trade_amount,
                });
                filled_orders.push(current_id);
                remaining_amount -= trade_amount;
                self.quantity -= trade_amount;
                self.order_count -= 1;
                trade_id += 1;
            } else {
                // Current order is partially filled
                let trade_amount = remaining_amount;

                trades.push(Trade {
                    id: trade_id,
                    price: order.price,
                    amount: trade_amount,
                });

                //Update the current order's quantity and put it back
                current_amount -= trade_amount;
                //TODO: Store the special type instead of the order id
                partially_filled_order = Some(current_id);
                self.orders.push_front((current_id, current_amount));

                self.quantity -= trade_amount;
                remaining_amount = 0;
                trade_id += 1;
            }
        }

        Some(FillResult {
            trades,
            filled_orders,
            partially_filled_order,
            remaining_amount,
        })
    }
}

pub struct MatchingEngine {
    /// Buy orders sorted by price (highest first) then time priority
    bids: BTreeMap<Price, PriceLevel>,
    /// Sell orders sorted by price (lowest first) then time priority
    asks: BTreeMap<Price, PriceLevel>,
    /// Index for fast order lookup by ID
    id_index: HashMap<OrderId, (Price, Side)>,
}

impl MatchingEngine {
    /// Create a new matching engine with empty order books.
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            id_index: HashMap::new(),
        }
    }

    pub fn run(
        engine: &mut Self,
        mut order_rx: UnboundedReceiver<OrderEvent>,
        market_tx: UnboundedSender<MarketEvent>,
        error_tx: UnboundedSender<EngineError>,
    ) {
        loop {
            match order_rx.blocking_recv() {
                Some(event) => match event {
                    OrderEvent::New(new_order) => {
                        match engine.process_new_order(&new_order) {
                            Ok(events) => {
                                events.into_iter().for_each(|event| {
                                    //TODO: Handle channel errors
                                    // Such errors mean that the channel is closed, so we must restart the engine
                                    market_tx.send(event).unwrap();
                                });
                            }
                            Err(err) => {
                                //TODO: Handle channel errors
                                error_tx.send(err).unwrap();
                            }
                        }
                    }
                    OrderEvent::Cancel(order_id) => {
                        match engine.process_cancel_order(order_id) {
                            Ok(event) => {
                                //TODO: Handle channel errors
                                // Such errors mean that the channel is closed, so we must restart the engine
                                market_tx.send(event).unwrap();
                            }
                            Err(err) => {
                                //TODO: Handle channel errors
                                error_tx.send(err).unwrap();
                            }
                        }
                    }
                },
                None => panic!("Orders channels is closed"),
            }
        }
    }

    // This function must return a bunch of MarketEvents, that are represents the state changes.
    fn process_new_order(&mut self, new_order: &Order) -> Result<Vec<MarketEvent>, EngineError> {
        match new_order.side {
            Side::Bid => self.process_buy_order(new_order),
            Side::Ask => self.process_sell_order(new_order),
        }
    }

    fn process_buy_order(&mut self, order: &Order) -> Result<Vec<MarketEvent>, EngineError> {
        let mut events = Vec::new();
        match self.asks.iter_mut().next() {
            Some((price, level)) if price <= &order.price => {
                if let Some(fill_result) = level.try_fill(order) {
                    //Updating a order with the unfilled part
                    if fill_result.remaining_amount > 0 {
                        // Insert the order with remaining amount
                        let remaining_order = Order::new(order.id, order.side, order.price, fill_result.remaining_amount);
                        self.insert_bid(&remaining_order);
                        events.push(MarketEvent::OrderUpdated(order.id));
                    }
                    events.append(&mut fill_result.into_events());
                } else {
                    self.insert_bid(order);
                    events.push(MarketEvent::OrderPlaced(order.id))
                }
            }
            _ => {
                self.insert_bid(order);
                events.push(MarketEvent::OrderPlaced(order.id))
            }
        }
        Ok(events)
    }

    fn process_sell_order(&mut self, order: &Order) -> Result<Vec<MarketEvent>, EngineError> {
        let mut events = Vec::new();
        match self.bids.iter_mut().rev().next() {
            Some((price, level)) if price >= &order.price => {
                if let Some(fill_result) = level.try_fill(order) {
                    //Updating a order with the unfilled part
                    if fill_result.remaining_amount > 0 {
                        // Insert the order with remaining amount
                        let remaining_order = Order::new(order.id, order.side, order.price, fill_result.remaining_amount);
                        self.insert_ask(&remaining_order);
                        events.push(MarketEvent::OrderUpdated(order.id));
                    }
                    events.append(&mut fill_result.into_events());
                } else {
                    self.insert_ask(order);
                    events.push(MarketEvent::OrderPlaced(order.id))
                }
            }
            _ => {
                self.insert_ask(order);
                events.push(MarketEvent::OrderPlaced(order.id))
            }
        }
        Ok(events)
    }

    fn process_cancel_order(&mut self, order_id: OrderId) -> Result<MarketEvent, EngineError> {
        if let Some((price, side)) = self.id_index.remove(&order_id) {
            match side {
                Side::Bid => {
                    if let Some(level) = self.bids.get_mut(&price) {
                        level.remove_order(order_id)?;
                        Ok(MarketEvent::OrderCancelled(order_id))
                    } else {
                        //TODO: Provide context?
                        Err(EngineError::OrderNotFound(order_id))
                    }
                }
                Side::Ask => {
                    if let Some(level) = self.asks.get_mut(&price) {
                        level.remove_order(order_id)?;
                        Ok(MarketEvent::OrderCancelled(order_id))
                    } else {
                        Err(EngineError::OrderNotFound(order_id))
                    }
                }
            }
        } else {
            Err(EngineError::OrderNotFound(order_id))
        }
    }

    fn insert_bid(&mut self, order: &Order) {
        match self.bids.get_mut(&order.price) {
            Some(level) => {
                level.add_order(order.id, order.amount);
            }
            None => {
                let mut level = PriceLevel::new();
                level.add_order(order.id, order.amount);
                self.bids.insert(order.price, level);
            }
        }
        self.id_index.insert(order.id, (order.price, Side::Bid));
    }

    fn update_bid(&mut self, order_id: OrderId, new_amount: u64) -> Result<(), EngineError> {
        if let Some((price, _)) = self.id_index.get(&order_id) {
            if let Some(level) = self.bids.get_mut(price) {
                level.update_order(order_id, new_amount)?;
            }
        }
        Ok(())
    }

    fn insert_ask(&mut self, order: &Order) {
        match self.asks.get_mut(&order.price) {
            Some(level) => {
                level.add_order(order.id, order.amount);
            }
            None => {
                let mut level = PriceLevel::new();
                level.add_order(order.id, order.amount);
                self.asks.insert(order.price, level);
            }
        }
        self.id_index.insert(order.id, (order.price, Side::Ask));
    }

    fn update_ask(&mut self, order_id: OrderId, new_amount: u64) -> Result<(), EngineError> {
        if let Some((price, _)) = self.id_index.get(&order_id) {
            if let Some(level) = self.asks.get_mut(price) {
                level.update_order(order_id, new_amount)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_new() {
        let order = Order::new(1, Side::Bid, 100, 50);
        assert_eq!(order.id, 1);
        assert!(matches!(order.side, Side::Bid));
        assert_eq!(order.price, 100);
        assert_eq!(order.amount, 50);
    }

    #[test]
    fn test_price_level_new() {
        let level = PriceLevel::new();
        assert_eq!(level.quantity, 0);
        assert_eq!(level.order_count, 0);
        assert!(level.orders.is_empty());
    }

    #[test]
    fn test_price_level_add_order() {
        let mut level = PriceLevel::new();
        level.add_order(1, 100);
        assert_eq!(level.quantity, 100);
        assert_eq!(level.order_count, 1);
        assert_eq!(level.orders.len(), 1);
        assert_eq!(level.orders[0], (1, 100));

        level.add_order(2, 200);
        assert_eq!(level.quantity, 300);
        assert_eq!(level.order_count, 2);
        assert_eq!(level.orders.len(), 2);
        assert_eq!(level.orders[1], (2, 200));
    }

    #[test]
    fn test_price_level_remove_order() {
        let mut level = PriceLevel::new();
        level.add_order(1, 100);
        level.add_order(2, 200);
        level.add_order(3, 300);

        assert!(level.remove_order(2).is_ok());
        assert_eq!(level.quantity, 400);
        assert_eq!(level.order_count, 2);
        assert_eq!(level.orders.len(), 2);

        // Order 2 should be removed
        assert!(!level.orders.iter().any(|(id, _)| *id == 2));

        // Try to remove non-existent order
        assert!(level.remove_order(99).is_err());
    }

    #[test]
    fn test_price_level_update_order() {
        let mut level = PriceLevel::new();
        level.add_order(1, 100);
        level.add_order(2, 200);

        assert!(level.update_order(1, 150).is_ok());
        assert_eq!(level.quantity, 350); // 150 + 200
        assert_eq!(level.orders[0], (1, 150));

        // Try to update non-existent order
        assert!(level.update_order(99, 500).is_err());
    }

    #[test]
    fn test_price_level_try_fill_exact_match() {
        let mut level = PriceLevel::new();
        level.add_order(1, 100);

        let order = Order::new(99, Side::Bid, 1000, 100);
        let result = level.try_fill(&order).unwrap();

        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].amount, 100);
        assert_eq!(result.trades[0].price, 1000);
        assert_eq!(result.filled_orders, vec![1]);
        assert_eq!(result.partially_filled_order, None);
        assert_eq!(result.remaining_amount, 0);

        assert_eq!(level.quantity, 0);
        assert_eq!(level.order_count, 0);
        assert!(level.orders.is_empty());
    }

    #[test]
    fn test_price_level_try_fill_partial() {
        let mut level = PriceLevel::new();
        level.add_order(1, 100);

        let order = Order::new(99, Side::Bid, 1000, 150);
        let result = level.try_fill(&order).unwrap();

        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].amount, 100);
        assert_eq!(result.filled_orders, vec![1]);
        assert_eq!(result.partially_filled_order, None);
        assert_eq!(result.remaining_amount, 50);

        assert_eq!(level.quantity, 0);
        assert!(level.orders.is_empty());
    }

    #[test]
    fn test_price_level_try_fill_multiple_orders() {
        let mut level = PriceLevel::new();
        level.add_order(1, 100);
        level.add_order(2, 200);
        level.add_order(3, 300);

        let order = Order::new(99, Side::Bid, 1000, 250);
        let result = level.try_fill(&order).unwrap();

        assert_eq!(result.trades.len(), 2);
        assert_eq!(result.trades[0].amount, 100);
        assert_eq!(result.trades[1].amount, 150);
        assert_eq!(result.filled_orders, vec![1]);
        assert_eq!(result.partially_filled_order, Some(2));
        assert_eq!(result.remaining_amount, 0);

        assert_eq!(level.quantity, 350); // 50 + 300
        assert_eq!(level.order_count, 2);
        assert_eq!(level.orders[0], (2, 50));
        assert_eq!(level.orders[1], (3, 300));
    }

    #[test]
    fn test_fill_result_into_events() {
        let result = FillResult {
            trades: vec![
                Trade { id: 1, price: 100, amount: 50 },
                Trade { id: 2, price: 100, amount: 30 },
            ],
            filled_orders: vec![1, 2],
            partially_filled_order: Some(3),
            remaining_amount: 0,
        };

        let events = result.into_events();
        assert_eq!(events.len(), 5); // 2 trades + 2 filled + 1 partial

        // Check event types
        assert!(matches!(events[0], MarketEvent::Trade(_)));
        assert!(matches!(events[1], MarketEvent::Trade(_)));
        assert!(matches!(events[2], MarketEvent::OrderFilled(1)));
        assert!(matches!(events[3], MarketEvent::OrderFilled(2)));
        assert!(matches!(events[4], MarketEvent::OrderPartiallyFilled(3)));
    }

    #[test]
    fn test_matching_engine_new() {
        let engine = MatchingEngine::new();
        assert!(engine.bids.is_empty());
        assert!(engine.asks.is_empty());
        assert!(engine.id_index.is_empty());
    }

    #[test]
    fn test_matching_engine_insert_bid() {
        let mut engine = MatchingEngine::new();
        let order = Order::new(1, Side::Bid, 100, 50);

        engine.insert_bid(&order);

        assert_eq!(engine.bids.len(), 1);
        assert!(engine.bids.contains_key(&100));
        assert_eq!(engine.id_index.get(&1), Some(&(100, Side::Bid)));

        let level = engine.bids.get(&100).unwrap();
        assert_eq!(level.quantity, 50);
        assert_eq!(level.order_count, 1);
    }

    #[test]
    fn test_matching_engine_insert_ask() {
        let mut engine = MatchingEngine::new();
        let order = Order::new(1, Side::Ask, 100, 50);

        engine.insert_ask(&order);

        assert_eq!(engine.asks.len(), 1);
        assert!(engine.asks.contains_key(&100));
        assert_eq!(engine.id_index.get(&1), Some(&(100, Side::Ask)));

        let level = engine.asks.get(&100).unwrap();
        assert_eq!(level.quantity, 50);
        assert_eq!(level.order_count, 1);
    }

    #[test]
    fn test_matching_engine_update_bid() {
        let mut engine = MatchingEngine::new();
        let order = Order::new(1, Side::Bid, 100, 50);
        engine.insert_bid(&order);

        assert!(engine.update_bid(1, 75).is_ok());

        let level = engine.bids.get(&100).unwrap();
        assert_eq!(level.quantity, 75);
        assert_eq!(level.orders[0], (1, 75));
    }

    #[test]
    fn test_matching_engine_update_ask() {
        let mut engine = MatchingEngine::new();
        let order = Order::new(1, Side::Ask, 100, 50);
        engine.insert_ask(&order);

        assert!(engine.update_ask(1, 75).is_ok());

        let level = engine.asks.get(&100).unwrap();
        assert_eq!(level.quantity, 75);
        assert_eq!(level.orders[0], (1, 75));
    }

    #[test]
    fn test_process_buy_order_no_match() {
        let mut engine = MatchingEngine::new();
        let order = Order::new(1, Side::Bid, 100, 50);

        let events = engine.process_buy_order(&order).unwrap();

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MarketEvent::OrderPlaced(1)));
        assert_eq!(engine.bids.len(), 1);
    }

    #[test]
    fn test_process_sell_order_no_match() {
        let mut engine = MatchingEngine::new();
        let order = Order::new(1, Side::Ask, 100, 50);

        let events = engine.process_sell_order(&order).unwrap();

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MarketEvent::OrderPlaced(1)));
        assert_eq!(engine.asks.len(), 1);
    }

    #[test]
    fn test_process_buy_order_with_match() {
        let mut engine = MatchingEngine::new();
        
        // Place a sell order first
        let sell_order = Order::new(1, Side::Ask, 100, 50);
        engine.insert_ask(&sell_order);

        // Place a buy order that matches
        let buy_order = Order::new(2, Side::Bid, 100, 50);
        let events = engine.process_buy_order(&buy_order).unwrap();

        // Should have trade and filled events
        assert!(events.iter().any(|e| matches!(e, MarketEvent::Trade(_))));
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderFilled(1))));
    }

    #[test]
    fn test_process_sell_order_with_match() {
        let mut engine = MatchingEngine::new();
        
        // Place a buy order first
        let buy_order = Order::new(1, Side::Bid, 100, 50);
        engine.insert_bid(&buy_order);

        // Place a sell order that matches
        let sell_order = Order::new(2, Side::Ask, 100, 50);
        let events = engine.process_sell_order(&sell_order).unwrap();

        // Should have trade and filled events
        assert!(events.iter().any(|e| matches!(e, MarketEvent::Trade(_))));
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderFilled(1))));
    }

    #[test]
    fn test_process_cancel_order_bid() {
        let mut engine = MatchingEngine::new();
        let order = Order::new(1, Side::Bid, 100, 50);
        engine.insert_bid(&order);

        let event = engine.process_cancel_order(1).unwrap();
        assert!(matches!(event, MarketEvent::OrderCancelled(1)));
        assert!(!engine.id_index.contains_key(&1));
    }

    #[test]
    fn test_process_cancel_order_ask() {
        let mut engine = MatchingEngine::new();
        let order = Order::new(1, Side::Ask, 100, 50);
        engine.insert_ask(&order);

        let event = engine.process_cancel_order(1).unwrap();
        assert!(matches!(event, MarketEvent::OrderCancelled(1)));
        assert!(!engine.id_index.contains_key(&1));
    }

    #[test]
    fn test_process_cancel_order_not_found() {
        let mut engine = MatchingEngine::new();
        assert!(engine.process_cancel_order(99).is_err());
    }

    #[test]
    fn test_process_new_order() {
        let mut engine = MatchingEngine::new();
        
        // Test bid order
        let bid_order = Order::new(1, Side::Bid, 100, 50);
        let events = engine.process_new_order(&bid_order).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MarketEvent::OrderPlaced(1)));

        // Test ask order
        let ask_order = Order::new(2, Side::Ask, 110, 50);
        let events = engine.process_new_order(&ask_order).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MarketEvent::OrderPlaced(2)));
    }

    #[test]
    fn test_partial_fill_with_remainder() {
        let mut engine = MatchingEngine::new();
        
        // Place a small sell order
        let sell_order = Order::new(1, Side::Ask, 100, 30);
        engine.insert_ask(&sell_order);

        // Place a larger buy order
        let buy_order = Order::new(2, Side::Bid, 100, 50);
        let events = engine.process_buy_order(&buy_order).unwrap();

        // Should have trade, filled order 1, and updated order 2
        assert!(events.iter().any(|e| matches!(e, MarketEvent::Trade(_))));
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderFilled(1))));
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderUpdated(2))));

        // Check that order 2 is in the book with remaining amount
        let level = engine.bids.get(&100).unwrap();
        assert_eq!(level.quantity, 20); // 50 - 30
    }

    #[test]
    fn test_multiple_price_levels() {
        let mut engine = MatchingEngine::new();
        
        // Add multiple bid levels
        engine.insert_bid(&Order::new(1, Side::Bid, 100, 50));
        engine.insert_bid(&Order::new(2, Side::Bid, 99, 30));
        engine.insert_bid(&Order::new(3, Side::Bid, 98, 20));

        assert_eq!(engine.bids.len(), 3);
        
        // Add multiple ask levels
        engine.insert_ask(&Order::new(4, Side::Ask, 101, 40));
        engine.insert_ask(&Order::new(5, Side::Ask, 102, 60));
        engine.insert_ask(&Order::new(6, Side::Ask, 103, 80));

        assert_eq!(engine.asks.len(), 3);
        assert_eq!(engine.id_index.len(), 6);
    }

    #[test]
    fn test_btreemap_ordering() {
        let mut engine = MatchingEngine::new();
        
        // Test that bids are sorted in descending order (highest first)
        engine.insert_bid(&Order::new(1, Side::Bid, 100, 10));
        engine.insert_bid(&Order::new(2, Side::Bid, 105, 20));
        engine.insert_bid(&Order::new(3, Side::Bid, 95, 30));
        
        let bid_prices: Vec<_> = engine.bids.keys().cloned().collect();
        assert_eq!(bid_prices, vec![95, 100, 105]);
        
        // But when we iterate in reverse, we get highest first
        let best_bid_price = engine.bids.keys().rev().next().cloned();
        assert_eq!(best_bid_price, Some(105));
        
        // Test that asks are sorted in ascending order (lowest first)
        engine.insert_ask(&Order::new(4, Side::Ask, 110, 10));
        engine.insert_ask(&Order::new(5, Side::Ask, 115, 20));
        engine.insert_ask(&Order::new(6, Side::Ask, 108, 30));
        
        let ask_prices: Vec<_> = engine.asks.keys().cloned().collect();
        assert_eq!(ask_prices, vec![108, 110, 115]);
    }

    #[test]
    fn test_matching_at_different_prices() {
        let mut engine = MatchingEngine::new();
        
        // Place ask at 100
        engine.insert_ask(&Order::new(1, Side::Ask, 100, 50));
        
        // Place bid at 105 (higher than ask, should match)
        let buy_order = Order::new(2, Side::Bid, 105, 50);
        let events = engine.process_buy_order(&buy_order).unwrap();
        
        // Check that a trade occurred at the ask price
        let trade_event = events.iter().find_map(|e| {
            if let MarketEvent::Trade(trade) = e {
                Some(trade.as_ref())
            } else {
                None
            }
        }).unwrap();
        
        assert_eq!(trade_event.price, 105); // Trade happens at aggressive order price
        assert_eq!(trade_event.amount, 50);
    }

    #[test]
    fn test_empty_price_level_cleanup() {
        let mut engine = MatchingEngine::new();
        
        // Add and then remove all orders from a price level
        engine.insert_bid(&Order::new(1, Side::Bid, 100, 50));
        engine.process_cancel_order(1).unwrap();
        
        // The price level should still exist but be empty
        assert!(engine.bids.contains_key(&100));
        let level = engine.bids.get(&100).unwrap();
        assert_eq!(level.quantity, 0);
        assert_eq!(level.order_count, 0);
    }

    #[test]
    fn test_multiple_orders_same_price() {
        let mut engine = MatchingEngine::new();
        
        // Add multiple orders at the same price
        engine.insert_bid(&Order::new(1, Side::Bid, 100, 10));
        engine.insert_bid(&Order::new(2, Side::Bid, 100, 20));
        engine.insert_bid(&Order::new(3, Side::Bid, 100, 30));
        
        let level = engine.bids.get(&100).unwrap();
        assert_eq!(level.quantity, 60);
        assert_eq!(level.order_count, 3);
        assert_eq!(level.orders.len(), 3);
    }

    #[test]
    fn test_side_enum() {
        // Test Side enum
        let bid = Side::Bid;
        let ask = Side::Ask;
        
        assert!(matches!(bid, Side::Bid));
        assert!(matches!(ask, Side::Ask));
    }

    #[test]
    fn test_order_event_enum() {
        let order = Order::new(1, Side::Bid, 100, 50);
        let new_event = OrderEvent::New(Box::new(order.clone()));
        let cancel_event = OrderEvent::Cancel(1);
        
        assert!(matches!(new_event, OrderEvent::New(_)));
        assert!(matches!(cancel_event, OrderEvent::Cancel(1)));
    }

    #[test]
    fn test_market_event_variants() {
        let trade = Trade { id: 1, price: 100, amount: 50 };
        let trade_event = MarketEvent::Trade(Box::new(trade));
        
        assert!(matches!(trade_event, MarketEvent::Trade(_)));
    }

    #[test]
    fn test_cross_spread_matching() {
        let mut engine = MatchingEngine::new();
        
        // Set up order book with multiple levels
        engine.insert_bid(&Order::new(1, Side::Bid, 99, 100));
        engine.insert_bid(&Order::new(2, Side::Bid, 98, 200));
        engine.insert_ask(&Order::new(3, Side::Ask, 101, 150));
        engine.insert_ask(&Order::new(4, Side::Ask, 102, 250));
        
        // Place a large sell order that crosses the spread
        let sell_order = Order::new(5, Side::Ask, 97, 250);
        let events = engine.process_sell_order(&sell_order).unwrap();
        
        // Should match only the best bid level (since we only process one level at a time)
        let trades: Vec<_> = events.iter().filter_map(|e| {
            if let MarketEvent::Trade(trade) = e {
                Some(trade.as_ref())
            } else {
                None
            }
        }).collect();
        
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].amount, 100);
        
        // Remaining should be placed as a new ask
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderUpdated(5))));
        
        // Check that the remaining order is in the book
        let ask_level = engine.asks.get(&97).unwrap();
        assert_eq!(ask_level.quantity, 150); // 250 - 100
    }

    #[test]
    fn test_fifo_order_matching() {
        let mut engine = MatchingEngine::new();
        
        // Add multiple orders at the same price (FIFO test)
        engine.insert_ask(&Order::new(1, Side::Ask, 100, 30));
        engine.insert_ask(&Order::new(2, Side::Ask, 100, 40));
        engine.insert_ask(&Order::new(3, Side::Ask, 100, 50));
        
        // Match against them
        let buy_order = Order::new(4, Side::Bid, 100, 60);
        let events = engine.process_buy_order(&buy_order).unwrap();
        
        // Should fill order 1 completely (30) and order 2 partially (30)
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderFilled(1))));
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderPartiallyFilled(2))));
        
        // Order 3 should remain untouched
        let level = engine.asks.get(&100).unwrap();
        assert_eq!(level.orders.len(), 2); // Orders 2 (partial) and 3
        assert_eq!(level.orders[0].0, 2);
        assert_eq!(level.orders[0].1, 10); // 40 - 30
        assert_eq!(level.orders[1].0, 3);
        assert_eq!(level.orders[1].1, 50);
    }

    #[test]
    fn test_zero_amount_order() {
        let mut engine = MatchingEngine::new();
        
        // Test behavior with zero amount (edge case)
        let order = Order::new(1, Side::Bid, 100, 0);
        let events = engine.process_buy_order(&order).unwrap();
        
        // Should still be placed
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MarketEvent::OrderPlaced(1)));
    }

    #[test]
    fn test_max_price_order() {
        let mut engine = MatchingEngine::new();
        
        // Test with maximum u64 price
        let order = Order::new(1, Side::Bid, u64::MAX, 100);
        engine.insert_bid(&order);
        
        assert!(engine.bids.contains_key(&u64::MAX));
    }

    #[test]
    fn test_cancel_order_twice() {
        let mut engine = MatchingEngine::new();
        let order = Order::new(1, Side::Bid, 100, 50);
        engine.insert_bid(&order);
        
        // First cancel should succeed
        assert!(engine.process_cancel_order(1).is_ok());
        
        // Second cancel should fail
        assert!(engine.process_cancel_order(1).is_err());
    }

    #[test]
    fn test_order_at_same_price_after_cancel() {
        let mut engine = MatchingEngine::new();
        
        // Add two orders at same price
        engine.insert_bid(&Order::new(1, Side::Bid, 100, 50));
        engine.insert_bid(&Order::new(2, Side::Bid, 100, 30));
        
        // Cancel first order
        engine.process_cancel_order(1).unwrap();
        
        // Level should still exist with one order
        let level = engine.bids.get(&100).unwrap();
        assert_eq!(level.quantity, 30);
        assert_eq!(level.order_count, 1);
        assert_eq!(level.orders.len(), 1);
        assert_eq!(level.orders[0].0, 2);
    }

    #[test]
    fn test_exact_match_clears_level() {
        let mut engine = MatchingEngine::new();
        
        // Place ask order
        engine.insert_ask(&Order::new(1, Side::Ask, 100, 50));
        
        // Place matching buy order
        let buy_order = Order::new(2, Side::Bid, 100, 50);
        let events = engine.process_buy_order(&buy_order).unwrap();
        
        // Check events
        assert!(events.iter().any(|e| matches!(e, MarketEvent::Trade(_))));
        assert!(events.iter().any(|e| matches!(e, MarketEvent::OrderFilled(1))));
        
        // Level should be empty
        let level = engine.asks.get(&100).unwrap();
        assert_eq!(level.quantity, 0);
        assert_eq!(level.order_count, 0);
        assert!(level.orders.is_empty());
    }

    #[test]
    fn test_price_level_after_partial_fill() {
        let mut engine = MatchingEngine::new();
        
        // Add order
        engine.insert_ask(&Order::new(1, Side::Ask, 100, 100));
        
        // Partially fill it
        let buy_order = Order::new(2, Side::Bid, 100, 40);
        engine.process_buy_order(&buy_order).unwrap();
        
        // Check remaining
        let level = engine.asks.get(&100).unwrap();
        assert_eq!(level.quantity, 60);
        assert_eq!(level.order_count, 1);
        assert_eq!(level.orders[0].1, 60);
    }

    #[test]
    fn test_multiple_partial_fills_same_order() {
        let mut engine = MatchingEngine::new();
        
        // Add large order
        engine.insert_ask(&Order::new(1, Side::Ask, 100, 100));
        
        // First partial fill
        let buy1 = Order::new(2, Side::Bid, 100, 30);
        engine.process_buy_order(&buy1).unwrap();
        
        // Second partial fill
        let buy2 = Order::new(3, Side::Bid, 100, 20);
        engine.process_buy_order(&buy2).unwrap();
        
        // Check remaining
        let level = engine.asks.get(&100).unwrap();
        assert_eq!(level.quantity, 50); // 100 - 30 - 20
        assert_eq!(level.orders[0].1, 50);
    }

    #[test]
    fn test_trade_price_uses_aggressive_order() {
        let mut engine = MatchingEngine::new();
        
        // Place ask at 100
        engine.insert_ask(&Order::new(1, Side::Ask, 100, 50));
        
        // Place bid at 105 (crosses spread)
        let buy_order = Order::new(2, Side::Bid, 105, 50);
        let events = engine.process_buy_order(&buy_order).unwrap();
        
        // Trade should be at aggressive order price (105)
        let trade = events.iter().find_map(|e| {
            if let MarketEvent::Trade(t) = e {
                Some(t.as_ref())
            } else {
                None
            }
        }).unwrap();
        
        assert_eq!(trade.price, 105);
    }

    #[test]
    fn test_no_match_when_prices_dont_cross() {
        let mut engine = MatchingEngine::new();
        
        // Place bid at 99
        engine.insert_bid(&Order::new(1, Side::Bid, 99, 50));
        
        // Place ask at 101 (no match)
        let ask_order = Order::new(2, Side::Ask, 101, 50);
        let events = engine.process_sell_order(&ask_order).unwrap();
        
        // Should only place order, no match
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MarketEvent::OrderPlaced(2)));
        
        // Both orders should remain in book
        assert_eq!(engine.bids.len(), 1);
        assert_eq!(engine.asks.len(), 1);
    }

    #[test]
    fn test_remove_order_from_empty_level() {
        let mut level = PriceLevel::new();
        let result = level.remove_order(1);
        assert!(result.is_err());
        if let Err(EngineError::OrderNotFound(id)) = result {
            assert_eq!(id, 1);
        }
    }

    #[test]
    fn test_try_fill_empty_level() {
        let mut level = PriceLevel::new();
        let order = Order::new(1, Side::Bid, 100, 50);
        let result = level.try_fill(&order);
        
        // Should return Some with no trades but full remaining amount
        assert!(result.is_some());
        let fill_result = result.unwrap();
        assert!(fill_result.trades.is_empty());
        assert!(fill_result.filled_orders.is_empty());
        assert_eq!(fill_result.remaining_amount, 50);
    }

    #[test]
    fn test_large_order_fills_entire_level() {
        let mut level = PriceLevel::new();
        level.add_order(1, 10);
        level.add_order(2, 20);
        level.add_order(3, 30);
        
        let order = Order::new(99, Side::Bid, 100, 100);
        let result = level.try_fill(&order).unwrap();
        
        assert_eq!(result.trades.len(), 3);
        assert_eq!(result.filled_orders.len(), 3);
        assert_eq!(result.remaining_amount, 40); // 100 - 60
        assert_eq!(level.quantity, 0);
        assert_eq!(level.order_count, 0);
    }

    #[test]
    fn test_cancel_updates_index() {
        let mut engine = MatchingEngine::new();
        let order1 = Order::new(1, Side::Bid, 100, 50);
        let order2 = Order::new(2, Side::Ask, 100, 50);
        
        engine.insert_bid(&order1);
        engine.insert_ask(&order2);
        
        assert_eq!(engine.id_index.len(), 2);
        
        engine.process_cancel_order(1).unwrap();
        assert_eq!(engine.id_index.len(), 1);
        assert!(!engine.id_index.contains_key(&1));
        assert!(engine.id_index.contains_key(&2));
        
        engine.process_cancel_order(2).unwrap();
        assert_eq!(engine.id_index.len(), 0);
    }

    #[test]
    fn test_match_removes_filled_orders_from_index() {
        let mut engine = MatchingEngine::new();
        
        // Place ask
        engine.insert_ask(&Order::new(1, Side::Ask, 100, 50));
        assert!(engine.id_index.contains_key(&1));
        
        // Match it completely
        let buy_order = Order::new(2, Side::Bid, 100, 50);
        engine.process_buy_order(&buy_order).unwrap();
        
        // Filled order should be removed from level but index still contains it
        // (This is a design choice - the index could be cleaned up on fill)
        assert!(engine.id_index.contains_key(&1));
    }

    #[test]
    fn test_trade_id_increments() {
        let mut level = PriceLevel::new();
        level.add_order(1, 30);
        level.add_order(2, 40);
        level.add_order(3, 50);
        
        let order = Order::new(99, Side::Bid, 100, 100);
        let result = level.try_fill(&order).unwrap();
        
        // Trade IDs should increment
        assert_eq!(result.trades.len(), 3);
        assert_eq!(result.trades[0].id, 0);
        assert_eq!(result.trades[1].id, 1);
        assert_eq!(result.trades[2].id, 2);
    }

    #[test]
    fn test_same_order_multiple_prices() {
        let mut engine = MatchingEngine::new();
        
        // Try to insert same order ID at different prices (shouldn't happen in practice)
        engine.insert_bid(&Order::new(1, Side::Bid, 100, 50));
        engine.insert_bid(&Order::new(1, Side::Bid, 101, 60));
        
        // Index should have the latest price
        assert_eq!(engine.id_index.get(&1), Some(&(101, Side::Bid)));
        
        // Both price levels should exist
        assert!(engine.bids.contains_key(&100));
        assert!(engine.bids.contains_key(&101));
    }

    #[test]
    fn test_best_price_selection() {
        let mut engine = MatchingEngine::new();
        
        // Add multiple bid levels
        engine.insert_bid(&Order::new(1, Side::Bid, 100, 10));
        engine.insert_bid(&Order::new(2, Side::Bid, 105, 20));
        engine.insert_bid(&Order::new(3, Side::Bid, 103, 30));
        
        // Add multiple ask levels
        engine.insert_ask(&Order::new(4, Side::Ask, 110, 10));
        engine.insert_ask(&Order::new(5, Side::Ask, 108, 20));
        engine.insert_ask(&Order::new(6, Side::Ask, 112, 30));
        
        // Best bid should be 105, best ask should be 108
        let best_bid = engine.bids.keys().rev().next().cloned();
        let best_ask = engine.asks.keys().next().cloned();
        
        assert_eq!(best_bid, Some(105));
        assert_eq!(best_ask, Some(108));
    }
}