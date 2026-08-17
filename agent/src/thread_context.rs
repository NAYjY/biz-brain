//! T03 / P07: "which Order is this message about" — session state per sender.
//! Set-valued: a Worker can hold multiple concurrent `Assigned` Orders.
//!
//! P07: `remove_active_order` is called by inbox_worker when a terminal event
//! (OrderDone, WorkerUnavailable, WorkerCancelled, OwnerCancelled) fires.
//! OrderReset does NOT remove from ThreadContextStore — a stale hit on an
//! Unassigned order causes P13 classifier to return null order_id, which
//! falls through to disambiguation correctly (P04 + P07 resolution).

use std::collections::HashMap;

use domain::{ChannelIdentity, OrderId};

#[derive(Debug, Default)]
pub struct ThreadContextStore {
    active_orders: HashMap<ChannelIdentity, Vec<OrderId>>,
}

impl ThreadContextStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_orders(&self, sender: &ChannelIdentity) -> &[OrderId] {
        self.active_orders.get(sender).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn add_active_order(&mut self, sender: ChannelIdentity, order_id: OrderId) {
        let orders = self.active_orders.entry(sender).or_default();
        if !orders.contains(&order_id) {
            orders.push(order_id);
        }
    }

    pub fn remove_active_order(&mut self, sender: &ChannelIdentity, order_id: OrderId) {
        if let Some(orders) = self.active_orders.get_mut(sender) {
            orders.retain(|id| *id != order_id);
        }
    }
}
