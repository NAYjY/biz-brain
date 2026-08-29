//! T10: Supplier onboarding page — mirrors workers.rs.

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;

use crate::auth::{auth_error_response, authorize_branch, BranchAuthOutcome};
use crate::templates::{load_topbar_data, page_not_found, shell_close, shell_open, topbar_html};

pub async fn render_suppliers(
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

    let rows: Vec<(Uuid, String, Option<String>, Option<String>)> = match sqlx::query_as(
        "SELECT s.id, s.name, ad.channel, ad.external_id \
         FROM suppliers s \
         LEFT JOIN actor_directory ad \
           ON ad.actor_id = s.id \
           AND ad.actor_type = 'supplier' \
           AND ad.owner_confirmed = TRUE \
         WHERE s.branch_id = $1 \
         ORDER BY s.name ASC",
    )
    .bind(branch_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("suppliers query failed: {e}");
            return page_not_found();
        }
    };

    let rows_html = if rows.is_empty() {
        r#"<tr><td colspan="4" class="data-table__empty">No suppliers yet. Create one to get started.</td></tr>"#
            .to_string()
    } else {
        rows.iter()
            .map(|(id, name, channel, external_id)| {
                supplier_row_html(*id, name, channel.as_deref(), external_id.as_deref())
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let html = format!(
        r#"{shell_open}
{topbar}
<div class="page">
  <div class="page-header">
    <h1>Suppliers</h1>
    <button class="btn btn--primary" onclick="BB.openModal('create-supplier-modal')">+ Add Supplier</button>
  </div>

  <div class="card" style="margin-bottom:var(--space-6);">
    <div style="padding:var(--space-4);border-bottom:1px solid var(--color-border);">
      <h3 style="font-size:var(--text-sm);color:var(--color-text-muted);font-weight:500;">
        How Supplier onboarding works
      </h3>
    </div>
    <ol style="padding:var(--space-4) var(--space-4) var(--space-4) var(--space-8);
               display:flex;flex-direction:column;gap:var(--space-2);
               color:var(--color-text-muted);font-size:var(--text-sm);">
      <li>Add a Supplier here (name only — creates their profile)</li>
      <li>Tell the Supplier to send any message to your WhatsApp bot</li>
      <li>Go to the <a href="/branches/{branch_id}/actors">Workers &amp; Suppliers</a> page — their message appears as a pending binding</li>
      <li>Confirm the binding to link their WhatsApp account to this Supplier profile</li>
    </ol>
  </div>

  <div class="card">
    <table class="data-table" id="suppliers-table">
      <thead>
        <tr>
          <th>Name</th>
          <th>Channel</th>
          <th>Sender ID</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody id="suppliers-tbody">
        {rows_html}
      </tbody>
    </table>
  </div>
</div>

<!-- Create Supplier modal -->
<div class="modal-backdrop hidden" id="create-supplier-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title">Add Supplier</span>
      <button class="btn btn--ghost btn--sm" onclick="BB.closeModal('create-supplier-modal')">✕</button>
    </div>
    <div class="modal__body">
      <div class="form-group">
        <label class="form-label" for="supplier-name">Name</label>
        <input class="form-input" id="supplier-name" type="text"
               placeholder="e.g. ABC Hardware Co." autocomplete="off">
      </div>
      <p class="text-xs text-muted" style="margin-top:-.5rem;">
        After adding, tell this supplier to message your WhatsApp bot.
        Their binding will appear on the Workers &amp; Suppliers page for you to confirm.
      </p>
    </div>
    <div class="modal__footer">
      <button class="btn btn--ghost" onclick="BB.closeModal('create-supplier-modal')">Cancel</button>
      <button class="btn btn--primary" id="create-supplier-btn">Add Supplier</button>
    </div>
  </div>
</div>

<script src="/static/js/ui.js"></script>
<script src="/static/js/suppliers.js"></script>
<script>initSuppliersPage('{branch_id}');</script>
{shell_close}"#,
        shell_open = shell_open("Suppliers — Biz-Brain"),
        topbar = topbar_html(branch_id, &branch_name, &all_branches, "suppliers"),
        branch_id = branch_id,
        rows_html = rows_html,
        shell_close = shell_close(),
    );

    Html(html).into_response()
}

fn supplier_row_html(
    id: Uuid,
    name: &str,
    channel: Option<&str>,
    external_id: Option<&str>,
) -> String {
    let (binding_cell, channel_cell) = match (channel, external_id) {
        (Some(ch), Some(ext)) => {
            let label = match ch {
                "line" => "LINE",
                "whats_app" => "WhatsApp",
                "telegram" => "Telegram",
                other => other,
            };
            (
                format!(
                    r#"<span class="channel-badge channel-badge--{}">{}</span>"#,
                    ch.replace('_', "-"),
                    label
                ),
                format!(r#"<span class="font-mono text-xs">{}</span>"#, html_escape(ext)),
            )
        }
        _ => (
            r#"<span class="text-muted text-xs">Not bound</span>"#.to_string(),
            r#"<span class="text-muted text-xs">—</span>"#.to_string(),
        ),
    };

    format!(
        r#"<tr data-supplier-id="{id}">
  <td>{name}</td>
  <td>{binding_cell}</td>
  <td>{channel_cell}</td>
  <td>
    <button class="btn btn--ghost btn--sm" onclick="supplierDelete('{id}')">Remove</button>
  </td>
</tr>"#,
        id = id,
        name = html_escape(name),
        binding_cell = binding_cell,
        channel_cell = channel_cell,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}