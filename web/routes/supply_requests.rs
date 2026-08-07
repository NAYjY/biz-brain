//! D05: SupplyRequests view — SSR initial render (D03 pattern).

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;

use crate::auth::{auth_error_response, authorize_branch, BranchAuthOutcome};
use crate::templates::{shell_close, shell_open, topbar_html, page_not_found};

pub async fn render_supply_requests(
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    let outcome = authorize_branch(&jar, &state.pool, branch_id).await;
    if let Some(err) = auth_error_response(outcome) {
        return err;
    }

    let supply_requests = match state.projections.supply_requests_by_branch(branch_id).await {
        Ok(rows) => rows,
        Err(_) => return page_not_found(),
    };

    let rows_html = if supply_requests.is_empty() {
        r#"<tr><td colspan="4" class="data-table__empty">No supply requests yet.</td></tr>"#.to_string()
    } else {
        supply_requests
            .iter()
            .map(supply_request_row_html)
            .collect::<Vec<_>>()
            .join("\n")
    };

    let html = format!(
        r#"{}
{topbar}
<div class="page">
  <div class="page-header">
    <h1>Supply Requests</h1>
    <div style="display:flex;align-items:center;gap:1rem;">
      <div class="live-badge live-badge--disconnected" id="live-badge">
        <div class="live-badge__dot"></div>
        <span class="live-badge__label">Connecting…</span>
      </div>
      <button class="btn btn--primary" onclick="BB.openModal('create-sr-modal')">+ New Request</button>
    </div>
  </div>

  <div class="card">
    <table class="data-table" id="sr-table">
      <thead>
        <tr>
          <th>State</th>
          <th>Description</th>
          <th>Orders</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody id="sr-tbody">
        {rows_html}
      </tbody>
    </table>
  </div>
</div>

<!-- Create Supply Request modal -->
<div class="modal-backdrop hidden" id="create-sr-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title">New Supply Request</span>
      <button class="btn btn--ghost btn--sm" onclick="BB.closeModal('create-sr-modal')">✕</button>
    </div>
    <div class="modal__body">
      <div class="form-group">
        <label class="form-label">Description</label>
        <textarea class="form-textarea" id="sr-description" placeholder="What supplies are needed?"></textarea>
      </div>
      <div class="form-group">
        <label class="form-label">In-flight Orders to cover</label>
        <select class="form-select" id="sr-order-ids" multiple size="5" style="height:auto;"></select>
        <span class="text-xs text-muted">Hold Ctrl/Cmd to select multiple</span>
      </div>
    </div>
    <div class="modal__footer">
      <button class="btn btn--ghost" onclick="BB.closeModal('create-sr-modal')">Cancel</button>
      <button class="btn btn--primary" id="create-sr-btn">Create</button>
    </div>
  </div>
</div>

<!-- Approve Invoice modal -->
<div class="modal-backdrop hidden" id="approve-invoice-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title">Approve Invoice</span>
      <button class="btn btn--ghost btn--sm" onclick="BB.closeModal('approve-invoice-modal')">✕</button>
    </div>
    <div class="modal__body">
      <p class="text-sm text-muted" style="padding-bottom:.5rem;">
        Approving an invoice is a financial commitment. Review carefully.
      </p>
      <div class="form-group">
        <label class="form-label">Invoice</label>
        <select class="form-select" id="approve-invoice-select"></select>
      </div>
    </div>
    <div class="modal__footer">
      <button class="btn btn--ghost" onclick="BB.closeModal('approve-invoice-modal')">Cancel</button>
      <button class="btn btn--danger" id="approve-invoice-btn">Approve Invoice</button>
    </div>
  </div>
</div>

<script src="/static/js/ui.js"></script>
<script src="/static/js/live.js"></script>
<script src="/static/js/supply_requests.js"></script>
<script>initSupplyRequestsPage('{branch_id}');</script>
{}"#,
        shell_open("Supply Requests — Biz-Brain"),
        topbar = topbar_html(branch_id, "supply-requests"),
        rows_html = rows_html,
        branch_id = branch_id,
        shell_close()
    );

    Html(html).into_response()
}

fn supply_request_row_html(sr: &store::projection_tables::SupplyRequestCurrentState) -> String {
    let state_lower = sr.state.to_lowercase();
    format!(
        r#"<tr data-sr-id="{id}">
  <td><span class="state-pill state-pill--{state_lower}">{state}</span></td>
  <td>{desc}</td>
  <td><div class="chip-list" id="orders-{id}"></div></td>
  <td><div class="sr-actions" data-sr-id="{id}" style="display:flex;gap:.5rem;"></div></td>
</tr>"#,
        id = sr.id,
        state_lower = state_lower,
        state = sr.state.replace('_', " "),
        desc = html_escape(&sr.description),
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
