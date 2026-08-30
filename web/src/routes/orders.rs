//! D04 / P16 / F04 / F01 / T09: Orders view — SSR initial render (page 1 only).
//! T09: SSR calls paginated store fn with no cursor (first 50 orders, no filter).
//!      The browser takes over after first paint — filter bar + infinite scroll.
//! T12/T13: topbar now passes branch name + list for switcher.

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;
use store::{OrderFilter, PaginatedProjections, PAGE_SIZE};

use crate::auth::{auth_error_response, authorize_branch, BranchAuthOutcome};
use crate::templates::{load_topbar_data, page_not_found, shell_close, shell_open, topbar_html};

pub async fn render_orders(
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    let outcome = authorize_branch(&jar, &state.pool, branch_id).await;
    if let Some(err) = auth_error_response(outcome) {
        return err;
    }

    let claims = match authorize_branch(&jar, &state.pool, branch_id).await {
        BranchAuthOutcome::Authorized { claims, .. } => claims,
        _ => return axum::response::Redirect::to("/login").into_response(),
    };

    let (branch_name, all_branches) = load_topbar_data(&state.pool, branch_id, &claims).await;

    // T09: page 1 only — no cursor, no filter.
    let paginator = PaginatedProjections::new(state.pool.clone());
    let (orders, first_next_cursor) = match paginator
        .orders_page(branch_id, None, &OrderFilter::default(), PAGE_SIZE)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            eprintln!("orders page 1 query failed: {e:?}");
            return page_not_found();
        }
    };

    // Encode next_cursor so the JS can pick up infinite scroll from page 2.
    let initial_cursor_json = first_next_cursor
        .map(|c| format!(r#""{}""#, c.encode()))
        .unwrap_or_else(|| "null".to_string());

    let orders_rows_html = if orders.is_empty() {
        r#"<tr><td colspan="5" class="data-table__empty">No orders yet. Create one to get started.</td></tr>"#
            .to_string()
    } else {
        orders.iter().map(order_row_html).collect::<Vec<_>>().join("\n")
    };

    let nudge_html = build_nudge_banner(&orders);

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

  {nudge_html}

  <!-- T09: filter bar -->
  <div class="filter-bar" id="orders-filter-bar" style="
       display:flex;gap:var(--space-3);align-items:flex-end;
       flex-wrap:wrap;margin-bottom:var(--space-4);">
    <div class="form-group" style="margin-bottom:0;min-width:160px;">
      <label class="form-label" for="filter-state">State</label>
      <select class="form-select" id="filter-state">
        <option value="">All states</option>
        <option value="UNASSIGNED">Unassigned</option>
        <option value="ASSIGNED">Assigned</option>
        <option value="ACCEPTED">Accepted</option>
        <option value="PENDING_CLARIFICATION">Pending Clarification</option>
        <option value="UNAVAILABLE">Unavailable</option>
        <option value="READY_FOR_PICKUP">Ready for Pickup</option>
        <option value="DONE">Done</option>
        <option value="CANCELLED">Cancelled</option>
      </select>
    </div>
    <div class="form-group" style="margin-bottom:0;min-width:160px;">
      <label class="form-label" for="filter-worker">Worker</label>
      <select class="form-select" id="filter-worker">
        <option value="">All workers</option>
      </select>
    </div>
    <div class="form-group" style="margin-bottom:0;flex:1;min-width:200px;">
      <label class="form-label" for="filter-q">Search</label>
      <input class="form-input" id="filter-q" type="search"
             placeholder="Description or job name…" autocomplete="off">
    </div>
    <button class="btn btn--ghost" id="filter-clear-btn" style="margin-bottom:0;">Clear</button>
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
    <!-- T09: infinite scroll sentinel -->
    <div id="orders-scroll-sentinel" style="height:1px;"></div>
    <div id="orders-load-status" style="
         text-align:center;padding:var(--space-4);
         font-size:var(--text-sm);color:var(--color-text-muted);
         display:none;">
    </div>
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
        <label class="form-label" for="order-short-name">
          Job name <span class="text-muted" style="font-weight:400;">(optional, max 20 chars)</span>
        </label>
        <input class="form-input font-mono" id="order-short-name" type="text"
               maxlength="20" autocomplete="off"
               placeholder="e.g. AC-B3, ท่อชั้น2">
        <span class="text-xs text-muted" id="short-name-counter">0/20</span>
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
<script src="/static/js/f05_alerts.js"></script>
<script>
  // T09: pass the initial next_cursor from SSR so JS can continue from page 2.
  initOrdersPage('{branch_id}', {initial_cursor});
</script>
{shell_close}
"#,
        shell_open("Orders — Biz-Brain"),
        topbar = topbar_html(branch_id, &branch_name, &all_branches, "orders"),
        orders_rows_html = orders_rows_html,
        nudge_html = nudge_html,
        branch_id = branch_id,
        initial_cursor = initial_cursor_json,
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

    let name_prefix = match &o.short_name {
        Some(sn) => format!(
            r#"<span class="order-tag" title="Job name">{}</span> "#,
            html_escape(sn)
        ),
        None => String::new(),
    };

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

    let ai_badge = if o.ai_routed_low_confidence {
        r#"<span class="ai-badge" title="AI-routed with low confidence — review recommended">🤖?</span>"#
            .to_string()
    } else {
        String::new()
    };

    format!(
        r#"<tr data-order-id="{id}" data-state="{state}" data-short-name="{short_name_escaped}" data-start-date="{start_date}" data-due-date="{due_date}">
  <td><span class="state-pill state-pill--{state_lower}">{state_display}</span></td>
  <td>{name_prefix}<span class="order-desc" id="desc-{id}">{desc}</span></td>
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
        name_prefix = name_prefix,
        short_name_escaped = html_escape(o.short_name.as_deref().unwrap_or("")),
        start_date = o.start_date.map(|d| d.to_rfc3339()).unwrap_or_default(),
        due_date = o.due_date.map(|d| d.to_rfc3339()).unwrap_or_default(),
        thread_btn = thread_btn,
        ai_badge = ai_badge,
    )
}

fn build_nudge_banner(orders: &[store::projection_tables::OrderCurrentState]) -> String {
    use std::collections::HashMap;

    let active_states = ["ASSIGNED", "ACCEPTED", "PENDING_CLARIFICATION", "READY_FOR_PICKUP"];
    let mut worker_unnamed: HashMap<String, u32> = HashMap::new();

    for o in orders {
        let is_active = active_states.contains(&o.state.as_str());
        let is_unnamed = o.short_name.is_none();
        if is_active && is_unnamed {
            if let Some(name) = &o.worker_name {
                *worker_unnamed.entry(name.clone()).or_insert(0) += 1;
            }
        }
    }

    let offenders: Vec<String> = worker_unnamed
        .into_iter()
        .filter(|(_, count)| *count >= 3)
        .map(|(name, count)| format!("{name} has {count} active orders without job names"))
        .collect();

    if offenders.is_empty() {
        return String::new();
    }

    let lines = offenders.join("; ");
    format!(
        r#"<div class="nudge-banner" style="
            background:var(--color-state-warn-bg);
            border:1px solid var(--color-state-warn);
            border-radius:var(--radius-sm);
            padding:var(--space-3) var(--space-4);
            font-size:var(--text-sm);
            color:var(--color-state-warn);
            margin-bottom:var(--space-4);
            display:flex;align-items:center;gap:.5rem;">
          ⚠️ {lines} — workers may struggle to identify them. Set job names via ⚙️ → Set job name.
        </div>"#,
        lines = html_escape(&lines),
    )
}

pub fn validate_short_name(raw: Option<&str>) -> Result<Option<String>, (axum::http::StatusCode, String)> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) if s.len() > 20 => Err((
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            "short_name must be 20 characters or fewer".to_string(),
        )),
        Some(s) => Ok(Some(s.to_string())),
    }
}

pub fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}