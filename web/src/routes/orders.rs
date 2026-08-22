//! D04 / P16 / F04: Orders view — SSR initial render.
//! F04: inline worker-message bubble removed; thread button with unread badge added.

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;

use crate::auth::{auth_error_response, authorize_branch};
use crate::templates::{page_not_found, shell_close, shell_open, topbar_html};

pub async fn render_orders(
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    let outcome = authorize_branch(&jar, &state.pool, branch_id).await;
    if let Some(err) = auth_error_response(outcome) {
        return err;
    }

    let orders = match state.projections.orders_by_branch(branch_id).await {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("orders query failed: {e:?}");
            return page_not_found();
        }
    };

    let orders_rows_html = if orders.is_empty() {
        r#"<tr><td colspan="5" class="data-table__empty">No orders yet. Create one to get started.</td></tr>"#
            .to_string()
    } else {
        orders.iter().map(order_row_html).collect::<Vec<_>>().join("\n")
    };

    let html = format!(
        r#"{}
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

<!-- Assign / Reassign Worker modal -->
<div class="modal-backdrop hidden" id="assign-worker-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title" id="assign-worker-modal-title">Assign Worker</span>
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
      <button class="btn btn--primary" id="assign-worker-btn">Confirm</button>
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

    let worker_cell = match &o.worker_name {
        Some(name) => html_escape(name),
        None => "—".to_string(),
    };

    // F04: thread button — only when a worker is assigned (has something to chat about).
    let thread_btn = if o.worker_id.is_some() {
        let unread = o.unread_message_count;
        let badge = if unread > 0 {
            format!(" <span class=\"thread-unread-badge\">{unread}</span>")
        } else {
            String::new()
        };
        format!(
            r#"<button class="thread-btn" data-order-id="{id}" data-unread="{unread}"
                      title="View conversation thread">💬{badge}</button>"#,
            id = o.id,
            unread = unread,
        )
    } else {
        String::new()
    };

    // F04: AI low-confidence badge.
    let ai_badge = if o.ai_routed_low_confidence {
        r#"<span class="ai-badge" title="AI-routed with low confidence — review recommended">🤖?</span>"#
            .to_string()
    } else {
        String::new()
    };

    format!(
        r#"<tr data-order-id="{id}" data-state="{state}">
  <td><span class="state-pill state-pill--{state_lower}">{state_display}</span></td>
  <td><span class="order-desc" id="desc-{id}">{desc}</span></td>
  <td class="text-muted text-xs">{customer}</td>
  <td class="text-muted text-sm" id="worker-{id}">{worker}</td>
  <td>
    <div style="display:flex;gap:.5rem;align-items:center;flex-wrap:wrap;">
      {thread_btn}{ai_badge}
      <div class="order-gear-wrap" data-order-id="{id}" style="position:relative;display:inline-block;"></div>
    </div>
  </td>
</tr>"#,
        id = o.id,
        state = o.state,
        state_lower = state_lower,
        state_display = o.state.replace('_', " "),
        desc = html_escape(&o.description),
        customer = &o.customer_id.to_string()[..8],
        worker = worker_cell,
        thread_btn = thread_btn,
        ai_badge = ai_badge,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
