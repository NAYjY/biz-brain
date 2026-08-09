//! D04: Orders view — SSR initial render (D03 pattern).
//!
//! First paint: queries projection table directly (not via api loopback).
//! Subsequent updates: client JS fetches /api/v1/branches/:id/orders.
//! SSE: OrderChanged signal triggers whole-list re-fetch (D06/D03).

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;
use store::ProjectionTables;

use crate::auth::{auth_error_response, authorize_branch, BranchAuthOutcome};
use crate::templates::{shell_open, shell_close, topbar_html, page_not_found};

pub async fn render_orders(
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    let outcome = authorize_branch(&jar, &state.pool, branch_id).await;
    if let Some(err) = auth_error_response(outcome) {
        return err;
    }

    // D03: first paint queries projection table directly (server-side).
    let orders = match state.projections.orders_by_branch(branch_id).await {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("orders query failed: {e:?}");
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "DB error").into_response();
        },
    };

    let orders_rows_html = if orders.is_empty() {
        r#"<tr><td colspan="5" class="data-table__empty">No orders yet. Create one to get started.</td></tr>"#.to_string()
    } else {
        orders
            .iter()
            .map(|o| order_row_html(o))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let html = format!(r#"{}
{topbar}
<div class="page">
  <div class="page-header">
    <h1>Orders</h1>
    <div style="display:flex;align-items:center;gap:1rem;">
      <div class="live-badge live-badge--disconnected" id="live-badge">
        <div class="live-badge__dot"></div>
        <span class="live-badge__label">Connecting…</span>
      </div>
      <button class="btn btn--primary" onclick="BB.openModal('create-order-modal')">+ New Order</button>
    </div>
  </div>

  <div class="card">
    <table class="data-table" id="orders-table">
      <thead>
        <tr>
          <th>State</th>
          <th>Description</th>
          <th>Customer</th>
          <th>Worker</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody id="orders-tbody">
        {orders_rows_html}
      </tbody>
    </table>
  </div>
</div>

<!-- Create Order modal -->
<div class="modal-backdrop hidden" id="create-order-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title">New Order</span>
      <button class="btn btn--ghost btn--sm" onclick="BB.closeModal('create-order-modal')">✕</button>
    </div>
    <div class="modal__body">
      <div class="form-group">
        <label class="form-label" for="order-customer">Customer</label>
        <div style="display:flex;gap:.5rem;">
          <select class="form-select" id="order-customer" style="flex:1;"></select>
          <button class="btn btn--ghost btn--sm" id="new-customer-btn">+ New</button>
        </div>
      </div>
      <div class="form-group" id="new-customer-row" style="display:none;">
        <label class="form-label" for="new-customer-name">New customer name</label>
        <input class="form-input" id="new-customer-name" type="text" placeholder="Customer name">
      </div>
      <div class="form-group">
        <label class="form-label" for="order-description">Description</label>
        <textarea class="form-textarea" id="order-description" placeholder="What needs doing?"></textarea>
      </div>
    </div>
    <div class="modal__footer">
      <button class="btn btn--ghost" onclick="BB.closeModal('create-order-modal')">Cancel</button>
      <button class="btn btn--primary" id="create-order-btn">Create</button>
    </div>
  </div>
</div>

<!-- Assign Worker modal -->
<div class="modal-backdrop hidden" id="assign-worker-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title">Assign Worker</span>
      <button class="btn btn--ghost btn--sm" onclick="BB.closeModal('assign-worker-modal')">✕</button>
    </div>
    <div class="modal__body">
      <div class="form-group">
        <label class="form-label" for="assign-worker-select">Worker</label>
        <select class="form-select" id="assign-worker-select"></select>
      </div>
    </div>
    <div class="modal__footer">
      <button class="btn btn--ghost" onclick="BB.closeModal('assign-worker-modal')">Cancel</button>
      <button class="btn btn--primary" id="assign-worker-btn">Assign</button>
    </div>
  </div>
</div>

<script src="/static/js/ui.js"></script>
<script src="/static/js/live.js"></script>
<script src="/static/js/orders.js"></script>
<script>initOrdersPage('{branch_id}');</script>
{shell_close}
"#,
        shell_open("Orders — Biz-Brain"),
        topbar = topbar_html(branch_id, "orders"),
        orders_rows_html = orders_rows_html,
        branch_id = branch_id,
        shell_close = shell_close(),
    );

    Html(html).into_response()
}

fn order_row_html(o: &store::projection_tables::OrderCurrentState) -> String {
    let state_lower = o.state.to_lowercase();
    let short_id = &o.id.to_string()[..8];
    format!(
        r#"<tr data-order-id="{id}">
  <td><span class="state-pill state-pill--{state_lower}">{state}</span></td>
  <td>{desc}</td>
  <td class="text-muted font-mono text-xs">{customer}</td>
  <td class="text-muted text-xs" id="worker-{id}">—</td>
  <td><div style="display:flex;gap:.5rem;" class="order-actions" data-order-id="{id}"></div></td>
</tr>"#,
        id = o.id,
        state_lower = state_lower,
        state = o.state.replace('_', " "),
        desc = html_escape(&o.description),
        customer = &o.customer_id.to_string()[..8],
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
