//! Core matching engine implementation for order book trading.
//!
//! This module provides the main `MatchingEngine` struct and related types
//! for processing trading orders and generating trades.
use crossbeam_channel::{Receiver, Sender};
use std::collections::{BTreeMap, HashMap, VecDeque};

// The matching engine must process incoming events and generate outcoming events
//
// OrderEvent
// - NewOrder
// - CancelOrder
//
// MarketEvent
// - Trade
// - OrderCancelled
// - OrderPartialMatched
// - OrderFilled
//

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
    pub side: Side,
    //TODO: Work on decimals processing - use rust decimal crate
    pub price: Price,
    pub amount: u64,
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
    OrderPlaced(Box<(OrderId, Order)>),
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
    price_decimals: u64,
    base_decimals: u64,
    quote_decimals: u64,
}

// Engine should have
// - A run function that processes the incoming event using a crossbeam channel (engine loop)
// - A new function that creates a new Engine, with a channels
// - Other internal functions to work with orders and price levels

impl MatchingEngine {
    /// Create a new matching engine with empty order books.
    pub fn new(price_decimals: u64, base_decimals: u64, quote_decimals: u64) -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            id_index: HashMap::new(),
            price_decimals,
            base_decimals,
            quote_decimals,
        }
    }

    pub fn run(engine: &mut Self, order_rx: Receiver<OrderEvent>, market_tx: Sender<MarketEvent>) {
        loop {
            match order_rx.recv() {
                Ok(event) => match event {
                    OrderEvent::New(new_order) => {
                        engine.process_new_order(&new_order);
                    }
                    OrderEvent::Cancel(order_id) => {
                        engine.process_cancel_order(&order_id);
                    }
                },
                Err(_) => break,
            }
        }
    }

    // This function must return a bunch of MarketEvents, that are represents the state changes.

    fn process_new_order(&mut self, new_order: &Order) {
        match new_order.side {
            Side::Bid => {
                self.process_buy_order(new_order);
            }
            Side::Ask => {
                unimplemented!()
                //self.process_sell_order(new_order);
            }
        }
    }

    // When we got a new buy order, we could have such events
    // - The best ask price is higher than the order price, so we need to store order
    // - The best ask price is lower of equal to the order price, so we can try to fill the order
    // As a result, we can get a FillResult, if not - the order is stored

    fn process_buy_order(&mut self, order: &Order) -> Vec<MarketEvent> {
        // The None basically means that there are no asks in the order book
        let mut events = Vec::new();
        match self.asks.iter_mut().next() {
            Some((price, level)) if price <= &order.price => {
                let fill_result = level.try_fill(order);
                //TODO: Store the reminder of the order
                //TODO: Convert fill result to events
            }
            _ => {
                let order_id = self.insert_bid(&order.price, order.amount);
                events.push(MarketEvent::OrderPlaced(Box::new((
                    order_id,
                    order.clone(),
                ))))
            }
        }
        events
    }

    fn process_cancel_order(&mut self, order_id: &OrderId) {}

    fn insert_bid(&mut self, price: &Price, amount: u64) -> OrderId {
        //TODO: Generate order id with uuid;
        let order_id = 1;
        match self.bids.get_mut(price) {
            Some(level) => {
                level.add_order(order_id, amount);
            }
            None => {
                let mut level = PriceLevel::new();
                level.add_order(order_id, amount);
                self.bids.insert(*price, level);
            }
        }
        order_id
    }
}
