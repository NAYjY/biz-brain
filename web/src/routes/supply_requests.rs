//! D05 / T09 / T11: Supply Requests view — SSR shell, fully translated.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;
use store::{PaginatedProjections, SrFilter, PAGE_SIZE};

use crate::auth::{auth_error_response, authorize_branch, BranchAuthOutcome};
use crate::templates::{
    html_escape, load_topbar_data, page_not_found, shell_close, shell_open,
    topbar_html, translations_for,
};

pub async fn render_supply_requests(
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Response {
    let outcome = authorize_branch(&jar, &state.pool, branch_id).await;
    if let Some(err) = auth_error_response(outcome) { return err; }

    let claims = match authorize_branch(&jar, &state.pool, branch_id).await {
        BranchAuthOutcome::Authorized { claims, .. } => claims,
        _ => return axum::response::Redirect::to("/login").into_response(),
    };

    let t = translations_for(&jar, &headers);
    let (branch_name, all_branches) = load_topbar_data(&state.pool, branch_id, &claims).await;
    let current_path = format!("/branches/{branch_id}/supply-requests");

    let paginator = PaginatedProjections::new(state.pool.clone());
    let (supply_requests, first_next_cursor) = match paginator
        .supply_requests_page(branch_id, None, &SrFilter::default(), PAGE_SIZE)
        .await
    {
        Ok(result) => result,
        Err(_) => return page_not_found(),
    };

    let initial_cursor_json = first_next_cursor
        .map(|c| format!(r#""{}""#, c.encode()))
        .unwrap_or_else(|| "null".to_string());

    let rows_html = if supply_requests.is_empty() {
        format!(
            r#"<tr><td colspan="4" class="data-table__empty">{}</td></tr>"#,
            t.get("supply.empty")
        )
    } else {
        supply_requests
            .iter()
            .map(|sr| supply_request_row_html(sr, &t))
            .collect::<Vec<_>>()
            .join("\n")
    };

    // i18n strings for JS layer
    let i18n_json = serde_json::json!({
        "supply.empty":          t.get("supply.empty"),
        "supply.all_loaded":     t.get("supply.all_loaded"),
        "supply.filter.all":     t.get("supply.filter.all"),
        "supply.approve.warning":t.get("supply.approve.warning"),
        "btn.cancel":            t.get("btn.cancel"),
        "btn.confirm":           t.get("btn.confirm"),
        "btn.send":              t.get("btn.send"),
        "live.connecting":       t.get("live.connecting"),
        "live.live":             t.get("live.live"),
        "live.reconnecting":     t.get("live.reconnecting"),
        "state.draft":                  t.get("state.draft"),
        "state.sent":                   t.get("state.sent"),
        "state.invoice_received":       t.get("state.invoice_received"),
        "state.owner_approved_invoice": t.get("state.owner_approved_invoice"),
        "state.supplier_confirmed":     t.get("state.supplier_confirmed"),
    });

    let html = format!(
        r#"{shell_open}
{topbar}
<div class="page">
  <div class="page-header">
    <h1>{title}</h1>
    <div style="display:flex;align-items:center;gap:1rem;">
      <div class="live-badge live-badge--disconnected" id="live-badge">
        <div class="live-badge__dot"></div>
        <span class="live-badge__label">{connecting}</span>
      </div>
      <button class="btn btn--primary" onclick="BB.openModal('create-sr-modal')">{new_btn}</button>
    </div>
  </div>

  <div style="display:flex;gap:var(--space-3);align-items:flex-end;margin-bottom:var(--space-4);">
    <div class="form-group" style="margin-bottom:0;min-width:180px;">
      <label class="form-label" for="sr-filter-state">{filter_state_label}</label>
      <select class="form-select" id="sr-filter-state">
        <option value="">{filter_all}</option>
        <option value="DRAFT">{s_draft}</option>
        <option value="SENT">{s_sent}</option>
        <option value="INVOICE_RECEIVED">{s_invoice}</option>
        <option value="OWNER_APPROVED_INVOICE">{s_approved}</option>
        <option value="SUPPLIER_CONFIRMED">{s_confirmed}</option>
      </select>
    </div>
  </div>

  <div class="card">
    <table class="data-table" id="sr-table">
      <thead>
        <tr>
          <th>{col_state}</th>
          <th>{col_desc}</th>
          <th>{col_orders}</th>
          <th>{col_actions}</th>
        </tr>
      </thead>
      <tbody id="sr-tbody">{rows_html}</tbody>
    </table>
    <div id="sr-scroll-sentinel" style="height:1px;"></div>
    <div id="sr-load-status" style="
         text-align:center;padding:var(--space-4);
         font-size:var(--text-sm);color:var(--color-text-muted);display:none;"></div>
  </div>
</div>

<!-- Create Supply Request modal -->
<div class="modal-backdrop hidden" id="create-sr-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title">{create_title}</span>
      <button class="btn btn--ghost btn--sm" onclick="BB.closeModal('create-sr-modal')">✕</button>
    </div>
    <div class="modal__body">
      <div class="form-group">
        <label class="form-label">{create_desc_label}</label>
        <textarea class="form-textarea" id="sr-description"
                  placeholder="{create_desc_ph}"></textarea>
      </div>
      <div class="form-group">
        <label class="form-label">{create_orders_label}</label>
        <select class="form-select" id="sr-order-ids" multiple size="5" style="height:auto;"></select>
        <span class="text-xs text-muted">{create_orders_hint}</span>
      </div>
    </div>
    <div class="modal__footer">
      <button class="btn btn--ghost" onclick="BB.closeModal('create-sr-modal')">{cancel}</button>
      <button class="btn btn--primary" id="create-sr-btn">{create_btn}</button>
    </div>
  </div>
</div>

<!-- Approve Invoice modal -->
<div class="modal-backdrop hidden" id="approve-invoice-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title">{approve_title}</span>
      <button class="btn btn--ghost btn--sm" onclick="BB.closeModal('approve-invoice-modal')">✕</button>
    </div>
    <div class="modal__body">
      <p class="text-sm text-muted" style="padding-bottom:.5rem;">{approve_warning}</p>
      <div class="form-group">
        <label class="form-label">{approve_label}</label>
        <select class="form-select" id="approve-invoice-select"></select>
      </div>
      <div id="invoice-media-preview" style="margin-top:var(--space-3);"></div>
    </div>
    <div class="modal__footer">
      <button class="btn btn--ghost" onclick="BB.closeModal('approve-invoice-modal')">{cancel}</button>
      <button class="btn btn--danger" id="approve-invoice-btn">{approve_btn}</button>
    </div>
  </div>
</div>

<script>window.BB_I18N = {i18n_json};</script>
<script src="/static/js/ui.js"></script>
<script src="/static/js/live.js"></script>
<script src="/static/js/supply_requests.js"></script>
<script>initSupplyRequestsPage('{branch_id}', {initial_cursor});</script>
{shell_close}"#,
        shell_open         = shell_open(t.get("supply.page_title"), t.lang()),
        topbar             = topbar_html(branch_id, &branch_name, &all_branches, "supply-requests", &t, &current_path),
        title              = t.get("supply.title"),
        connecting         = t.get("live.connecting"),
        new_btn            = t.get("supply.new"),
        filter_state_label = t.get("supply.filter.state"),
        filter_all         = t.get("supply.filter.all"),
        s_draft            = t.get("state.draft"),
        s_sent             = t.get("state.sent"),
        s_invoice          = t.get("state.invoice_received"),
        s_approved         = t.get("state.owner_approved_invoice"),
        s_confirmed        = t.get("state.supplier_confirmed"),
        col_state          = t.get("supply.col.state"),
        col_desc           = t.get("supply.col.description"),
        col_orders         = t.get("supply.col.orders"),
        col_actions        = t.get("supply.col.actions"),
        rows_html          = rows_html,
        create_title       = t.get("supply.create.title"),
        create_desc_label  = t.get("supply.create.description"),
        create_desc_ph     = t.get("supply.create.description_ph"),
        create_orders_label= t.get("supply.create.orders"),
        create_orders_hint = t.get("supply.create.orders_hint"),
        cancel             = t.get("btn.cancel"),
        create_btn         = t.get("supply.create.btn"),
        approve_title      = t.get("supply.approve.title"),
        approve_warning    = t.get("supply.approve.warning"),
        approve_label      = t.get("supply.approve.label"),
        approve_btn        = t.get("supply.approve.btn"),
        i18n_json          = i18n_json,
        branch_id          = branch_id,
        initial_cursor     = initial_cursor_json,
        shell_close        = shell_close(),
    );

    Html(html).into_response()
}

fn supply_request_row_html(
    sr: &store::projection_tables_paginated::SrPageRow,
    t: &crate::i18n::Translations,
) -> String {
    let state_lower = sr.state.to_lowercase();
    let state_display = match sr.state.as_str() {
        "DRAFT"                  => t.get("state.draft"),
        "SENT"                   => t.get("state.sent"),
        "INVOICE_RECEIVED"       => t.get("state.invoice_received"),
        "OWNER_APPROVED_INVOICE" => t.get("state.owner_approved_invoice"),
        "SUPPLIER_CONFIRMED"     => t.get("state.supplier_confirmed"),
        other                    => other,
    };
    format!(
        r#"<tr data-sr-id="{id}">
  <td><span class="state-pill state-pill--{state_lower}">{state_display}</span></td>
  <td>{desc}</td>
  <td><div class="chip-list" id="orders-{id}"></div></td>
  <td><div class="sr-actions" data-sr-id="{id}" style="display:flex;gap:.5rem;"></div></td>
</tr>"#,
        id            = sr.id,
        state_lower   = state_lower,
        state_display = state_display,
        desc          = html_escape(&sr.description),
    )
}
