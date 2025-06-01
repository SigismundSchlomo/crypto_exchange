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

#[derive(Debug, Clone, Copy)]
pub enum Side {
    Bid,
    Ask,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: OrderId,
    pub side: Side,
    //TODO: Work on decimals processing - use rust decimal crate
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
                        self.update_bid(order.id, fill_result.remaining_amount)?;
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
        match self.bids.iter_mut().next() {
            Some((price, level)) if price >= &order.price => {
                if let Some(fill_result) = level.try_fill(order) {
                    //Updating a order with the unfilled part
                    if fill_result.remaining_amount > 0 {
                        self.update_ask(order.id, fill_result.remaining_amount)?;
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
