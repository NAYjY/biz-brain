//! T03: "which Order is this message about" — session state per LINE/WhatsApp
//! thread. Set-valued, not a single "current Order": a Worker can hold
//! multiple concurrent `Assigned` Orders (freelance work is not single-order).

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
