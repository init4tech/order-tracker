use ratatui::widgets::TableState;
use signet_tracker::OrderStatus;

pub struct App {
    orders: Vec<OrderStatus>,
    pub table_state: TableState,
    pub running: bool,
    pub connected: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            orders: Vec::new(),
            table_state: TableState::default(),
            running: true,
            connected: true,
        }
    }

    /// Insert or update an order by hash.
    pub fn update_order(&mut self, status: OrderStatus) {
        let hash = status.order_hash();
        if let Some(pos) = self.orders.iter().position(|order| order.order_hash() == hash) {
            self.orders[pos] = status;
        } else {
            self.orders.push(status);
            if self.table_state.selected().is_none() {
                self.table_state.select(Some(0));
            }
        }
    }

    pub fn orders(&self) -> &[OrderStatus] {
        &self.orders
    }

    pub fn selected_order(&self) -> Option<&OrderStatus> {
        self.table_state.selected().and_then(|idx| self.orders.get(idx))
    }

    pub fn select_next(&mut self) {
        if self.orders.is_empty() {
            return;
        }
        let next = match self.table_state.selected() {
            Some(current) => (current + 1).min(self.orders.len() - 1),
            None => 0,
        };
        self.table_state.select(Some(next));
    }

    pub fn select_prev(&mut self) {
        if self.orders.is_empty() {
            return;
        }
        let prev = match self.table_state.selected() {
            Some(current) => current.saturating_sub(1),
            None => 0,
        };
        self.table_state.select(Some(prev));
    }

    /// Returns (pending, filled, expired) counts.
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut pending = 0;
        let mut filled = 0;
        let mut expired = 0;
        for order in &self.orders {
            match order {
                OrderStatus::Pending { .. } => pending += 1,
                OrderStatus::Filled { .. } => filled += 1,
                OrderStatus::Expired { .. } => expired += 1,
            }
        }
        (pending, filled, expired)
    }
}
