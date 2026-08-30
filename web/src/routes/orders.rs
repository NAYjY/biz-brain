//! D04 / P16 / F04 / F01 / T09 / T11: Orders view — SSR shell, fully translated.
//! T11: all chrome strings go through Translations. JS i18n strings passed
//!      as a data-i18n JSON attribute on the root div so orders.js can use them.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;
use store::{OrderFilter, PaginatedProjections, PAGE_SIZE};

use crate::auth::{auth_error_response, authorize_branch, BranchAuthOutcome};
use crate::templates::{
    html_escape, load_topbar_data, page_not_found, shell_close, shell_open,
    topbar_html, translations_for,
};

pub async fn render_orders(
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
    let current_path = format!("/branches/{branch_id}/orders");

    let paginator = PaginatedProjections::new(state.pool.clone());
    let (orders, first_next_cursor) = match paginator
        .orders_page(branch_id, None, &OrderFilter::default(), PAGE_SIZE)
        .await
    {
        Ok(result) => result,
        Err(e) => { eprintln!("orders page 1 query failed: {e:?}"); return page_not_found(); }
    };

    let initial_cursor_json = first_next_cursor
        .map(|c| format!(r#""{}""#, c.encode()))
        .unwrap_or_else(|| "null".to_string());

    let orders_rows_html = if orders.is_empty() {
        format!(
            r#"<tr><td colspan="5" class="data-table__empty">{}</td></tr>"#,
            t.get("orders.empty")
        )
    } else {
        orders.iter().map(|o| order_row_html(o, &t)).collect::<Vec<_>>().join("\n")
    };

    let nudge_html = build_nudge_banner(&orders, &t);

    // Build a JSON object of all strings the JS layer needs so we don't need
    // a separate JS i18n bundle. Passed as window.BB_I18N.
    let i18n_json = serde_json::json!({
        "orders.empty":                  t.get("orders.empty"),
        "orders.all_loaded":             t.get("orders.all_loaded"),
        "orders.loading":                t.get("orders.loading"),
        "orders.assign.title":           t.get("orders.assign.title"),
        "orders.reassign.title":         t.get("orders.reassign.title"),
        "orders.assign.placeholder":     t.get("orders.assign.placeholder"),
        "orders.create.customer_placeholder": t.get("orders.create.customer_placeholder"),
        "orders.filter.all_states":      t.get("orders.filter.all_states"),
        "orders.filter.all_workers":     t.get("orders.filter.all_workers"),
        "btn.cancel":                    t.get("btn.cancel"),
        "btn.confirm":                   t.get("btn.confirm"),
        "btn.save":                      t.get("btn.save"),
        "btn.remove":                    t.get("btn.remove"),
        "btn.send":                      t.get("btn.send"),
        "btn.saving":                    t.get("btn.saving"),
        "btn.sending":                   t.get("btn.sending"),
        "thread.reply_placeholder":      t.get("thread.reply_placeholder"),
        "thread.empty":                  t.get("thread.empty"),
        "thread.label.worker":           t.get("thread.label.worker"),
        "thread.label.owner":            t.get("thread.label.owner"),
        "live.connecting":               t.get("live.connecting"),
        "live.live":                     t.get("live.live"),
        "live.reconnecting":             t.get("live.reconnecting"),
        "state.unassigned":              t.get("state.unassigned"),
        "state.assigned":                t.get("state.assigned"),
        "state.accepted":                t.get("state.accepted"),
        "state.pending_clarification":   t.get("state.pending_clarification"),
        "state.unavailable":             t.get("state.unavailable"),
        "state.ready_for_pickup":        t.get("state.ready_for_pickup"),
        "state.done":                    t.get("state.done"),
        "state.cancelled":               t.get("state.cancelled"),
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
      <button class="btn btn--primary" onclick="BB.openModal('create-order-modal')">{new_order}</button>
    </div>
  </div>

  {nudge_html}

  <div class="filter-bar" id="orders-filter-bar" style="
       display:flex;gap:var(--space-3);align-items:flex-end;
       flex-wrap:wrap;margin-bottom:var(--space-4);">
    <div class="form-group" style="margin-bottom:0;min-width:160px;">
      <label class="form-label" for="filter-state">{filter_state_label}</label>
      <select class="form-select" id="filter-state">
        <option value="">{filter_all_states}</option>
        <option value="UNASSIGNED">{s_unassigned}</option>
        <option value="ASSIGNED">{s_assigned}</option>
        <option value="ACCEPTED">{s_accepted}</option>
        <option value="PENDING_CLARIFICATION">{s_clarif}</option>
        <option value="UNAVAILABLE">{s_unavail}</option>
        <option value="READY_FOR_PICKUP">{s_ready}</option>
        <option value="DONE">{s_done}</option>
        <option value="CANCELLED">{s_cancelled}</option>
      </select>
    </div>
    <div class="form-group" style="margin-bottom:0;min-width:160px;">
      <label class="form-label" for="filter-worker">{filter_worker_label}</label>
      <select class="form-select" id="filter-worker">
        <option value="">{filter_all_workers}</option>
      </select>
    </div>
    <div class="form-group" style="margin-bottom:0;flex:1;min-width:200px;">
      <label class="form-label" for="filter-q">{filter_search_label}</label>
      <input class="form-input" id="filter-q" type="search"
             placeholder="{filter_placeholder}" autocomplete="off">
    </div>
    <button class="btn btn--ghost" id="filter-clear-btn" style="margin-bottom:0;">{filter_clear}</button>
  </div>

  <div class="card">
    <table class="data-table" id="orders-table">
      <thead>
        <tr>
          <th>{col_state}</th>
          <th>{col_desc}</th>
          <th>{col_customer}</th>
          <th>{col_worker}</th>
          <th>{col_actions}</th>
        </tr>
      </thead>
      <tbody id="orders-tbody">{orders_rows_html}</tbody>
    </table>
    <div id="orders-scroll-sentinel" style="height:1px;"></div>
    <div id="orders-load-status" style="
         text-align:center;padding:var(--space-4);
         font-size:var(--text-sm);color:var(--color-text-muted);display:none;"></div>
  </div>
</div>

<!-- Create Order modal -->
<div class="modal-backdrop hidden" id="create-order-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title">{create_title}</span>
      <button class="btn btn--ghost btn--sm" onclick="BB.closeModal('create-order-modal')">✕</button>
    </div>
    <div class="modal__body">
      <div class="form-group">
        <label class="form-label" for="order-customer">{create_customer}</label>
        <div style="display:flex;gap:.5rem;">
          <select class="form-select" id="order-customer" style="flex:1;"></select>
          <button class="btn btn--ghost btn--sm" id="new-customer-btn">{create_new_customer}</button>
        </div>
      </div>
      <div class="form-group" id="new-customer-row" style="display:none;">
        <label class="form-label" for="new-customer-name">{create_new_customer_name}</label>
        <input class="form-input" id="new-customer-name" type="text"
               placeholder="{create_customer_ph}">
      </div>
      <div class="form-group">
        <label class="form-label" for="order-short-name">
          {create_short_name} <span class="text-muted" style="font-weight:400;">{create_short_name_hint}</span>
        </label>
        <input class="form-input font-mono" id="order-short-name" type="text"
               maxlength="20" autocomplete="off" placeholder="{create_short_name_ph}">
        <span class="text-xs text-muted" id="short-name-counter">0/20</span>
      </div>
      <div class="form-group">
        <label class="form-label" for="order-description">{create_desc}</label>
        <textarea class="form-textarea" id="order-description"
                  placeholder="{create_desc_ph}"></textarea>
      </div>
    </div>
    <div class="modal__footer">
      <button class="btn btn--ghost" onclick="BB.closeModal('create-order-modal')">{cancel}</button>
      <button class="btn btn--primary" id="create-order-btn">{create_btn}</button>
    </div>
  </div>
</div>

<!-- Assign / Reassign Worker modal -->
<div class="modal-backdrop hidden" id="assign-worker-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title" id="assign-worker-modal-title">{assign_title}</span>
      <button class="btn btn--ghost btn--sm" onclick="BB.closeModal('assign-worker-modal')">✕</button>
    </div>
    <div class="modal__body">
      <div class="form-group">
        <label class="form-label" for="assign-worker-select">{assign_label}</label>
        <select class="form-select" id="assign-worker-select"></select>
      </div>
    </div>
    <div class="modal__footer">
      <button class="btn btn--ghost" onclick="BB.closeModal('assign-worker-modal')">{cancel}</button>
      <button class="btn btn--primary" id="assign-worker-btn">{assign_btn}</button>
    </div>
  </div>
</div>

<script>window.BB_I18N = {i18n_json};</script>
<script src="/static/js/ui.js"></script>
<script src="/static/js/live.js"></script>
<script src="/static/js/orders.js"></script>
<script src="/static/js/f05_alerts.js"></script>
<script>initOrdersPage('{branch_id}', {initial_cursor});</script>
{shell_close}
"#,
        shell_open           = shell_open(t.get("orders.page_title"), t.lang()),
        topbar               = topbar_html(branch_id, &branch_name, &all_branches, "orders", &t, &current_path),
        title                = t.get("orders.title"),
        connecting           = t.get("live.connecting"),
        new_order            = t.get("orders.new"),
        nudge_html           = nudge_html,
        filter_state_label   = t.get("orders.filter.state"),
        filter_all_states    = t.get("orders.filter.all_states"),
        filter_worker_label  = t.get("orders.filter.worker"),
        filter_all_workers   = t.get("orders.filter.all_workers"),
        filter_search_label  = t.get("orders.filter.search"),
        filter_placeholder   = t.get("orders.filter.placeholder"),
        filter_clear         = t.get("orders.filter.clear"),
        col_state            = t.get("orders.col.state"),
        col_desc             = t.get("orders.col.description"),
        col_customer         = t.get("orders.col.customer"),
        col_worker           = t.get("orders.col.worker"),
        col_actions          = t.get("orders.col.actions"),
        s_unassigned         = t.get("state.unassigned"),
        s_assigned           = t.get("state.assigned"),
        s_accepted           = t.get("state.accepted"),
        s_clarif             = t.get("state.pending_clarification"),
        s_unavail            = t.get("state.unavailable"),
        s_ready              = t.get("state.ready_for_pickup"),
        s_done               = t.get("state.done"),
        s_cancelled          = t.get("state.cancelled"),
        orders_rows_html     = orders_rows_html,
        create_title         = t.get("orders.create.title"),
        create_customer      = t.get("orders.create.customer"),
        create_new_customer  = t.get("orders.create.new_customer"),
        create_new_customer_name = t.get("orders.create.new_customer_name"),
        create_customer_ph   = t.get("orders.create.customer_placeholder"),
        create_short_name    = t.get("orders.create.short_name"),
        create_short_name_hint = t.get("orders.create.short_name_hint"),
        create_short_name_ph = t.get("orders.create.short_name_placeholder"),
        create_desc          = t.get("orders.create.description"),
        create_desc_ph       = t.get("orders.create.description_placeholder"),
        cancel               = t.get("btn.cancel"),
        create_btn           = t.get("orders.create.btn"),
        assign_title         = t.get("orders.assign.title"),
        assign_label         = t.get("orders.assign.label"),
        assign_btn           = t.get("orders.assign.btn"),
        i18n_json            = i18n_json,
        branch_id            = branch_id,
        initial_cursor       = initial_cursor_json,
        shell_close          = shell_close(),
    );

    Html(html).into_response()
}

fn order_row_html(o: &store::projection_tables::OrderCurrentState, t: &crate::i18n::Translations) -> String {
    let state_lower = o.state.to_lowercase();
    let state_display = state_pill_label(&o.state, t);

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
            format!(r#" <span class="thread-unread-badge">{unread}</span>"#)
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
        r#"<tr data-order-id="{id}" data-state="{state}"
            data-short-name="{short_name_escaped}"
            data-start-date="{start_date}" data-due-date="{due_date}">
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
        id                 = o.id,
        state              = o.state,
        state_lower        = state_lower,
        state_display      = state_display,
        desc               = html_escape(&o.description),
        customer           = &o.customer_id.to_string()[..8],
        worker             = worker_cell,
        name_prefix        = name_prefix,
        short_name_escaped = html_escape(o.short_name.as_deref().unwrap_or("")),
        start_date         = o.start_date.map(|d| d.to_rfc3339()).unwrap_or_default(),
        due_date           = o.due_date.map(|d| d.to_rfc3339()).unwrap_or_default(),
        thread_btn         = thread_btn,
        ai_badge           = ai_badge,
    )
}

/// Map the raw DB state string to the translated pill label.
fn state_pill_label(state: &str, t: &crate::i18n::Translations) -> String {
    let key = match state {
        "UNASSIGNED"            => "state.unassigned",
        "ASSIGNED"              => "state.assigned",
        "ACCEPTED"              => "state.accepted",
        "PENDING_CLARIFICATION" => "state.pending_clarification",
        "UNAVAILABLE"           => "state.unavailable",
        "READY_FOR_PICKUP"      => "state.ready_for_pickup",
        "DONE"                  => "state.done",
        "CANCELLED"             => "state.cancelled",
        "DRAFT"                 => "state.draft",
        "SENT"                  => "state.sent",
        "INVOICE_RECEIVED"      => "state.invoice_received",
        "OWNER_APPROVED_INVOICE"=> "state.owner_approved_invoice",
        "SUPPLIER_CONFIRMED"    => "state.supplier_confirmed",
        _                       => return state.replace('_', " "),
    };
    t.get(key).to_string()
}

fn build_nudge_banner(
    orders: &[store::projection_tables::OrderCurrentState],
    t: &crate::i18n::Translations,
) -> String {
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

    let suffix = t.get("nudge.no_job_names");
    let offenders: Vec<String> = worker_unnamed
        .into_iter()
        .filter(|(_, count)| *count >= 3)
        .map(|(name, count)| format!("{name} ({count}) — {suffix}"))
        .collect();

    if offenders.is_empty() {
        return String::new();
    }

    let lines = offenders.join("; ");
    format!(
        r#"<div class="nudge-banner" style="
            background:var(--color-state-warn-bg);border:1px solid var(--color-state-warn);
            border-radius:var(--radius-sm);padding:var(--space-3) var(--space-4);
            font-size:var(--text-sm);color:var(--color-state-warn);
            margin-bottom:var(--space-4);display:flex;align-items:center;gap:.5rem;">
          ⚠️ {lines}
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
