//! D08-5: Actors (pending Worker/Supplier bindings) page.
//! SSR initial render — same D03 pattern as orders.rs.
//!
//! Owner sees every unconfirmed (channel, external_id) → actor mapping
//! for this Branch and confirms or rejects each one.
//! Until confirmed, messages from that sender are silently dropped (S06 fail-closed).

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;
use domain::BranchId;
use store::PendingBinding;

use crate::auth::{auth_error_response, authorize_branch};
use crate::templates::{shell_close, shell_open, topbar_html, page_not_found};

pub async fn render_actors(
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    let outcome = authorize_branch(&jar, &state.pool, branch_id).await;
    if let Some(err) = auth_error_response(outcome) {
        return err;
    }

    let bindings = match state.actors.list_pending(BranchId::new(branch_id)).await {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("actors list_pending failed: {e:?}");
            return page_not_found();
        }
    };

    let rows_html = if bindings.is_empty() {
        r#"<tr><td colspan="5" class="data-table__empty">No pending bindings. When a new Worker or Supplier messages for the first time, they appear here.</td></tr>"#.to_string()
    } else {
        bindings.iter().map(binding_row_html).collect::<Vec<_>>().join("\n")
    };

    let html = format!(
        r#"{}
{topbar}
<div class="page">
  <div class="page-header">
    <h1>Workers &amp; Suppliers</h1>
    <p class="text-sm text-muted" style="margin-top:.25rem;">
      Confirm a binding to trust that sender. Reject to remove it — their next message creates a new pending row.
    </p>
  </div>

  <div class="card">
    <table class="data-table" id="actors-table">
      <thead>
        <tr>
          <th>Channel</th>
          <th>Sender ID</th>
          <th>Type</th>
          <th>First seen</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody id="actors-tbody">
        {rows_html}
      </tbody>
    </table>
  </div>
</div>

<script src="/static/js/ui.js"></script>
<script src="/static/js/actors.js"></script>
<script>initActorsPage('{branch_id}');</script>
{shell_close}"#,
        shell_open("Workers — Biz-Brain"),
        topbar = topbar_html(branch_id, "actors"),
        rows_html = rows_html,
        branch_id = branch_id,
        shell_close = shell_close(),
    );

    Html(html).into_response()
}

fn binding_row_html(b: &PendingBinding) -> String {
    let channel_label = match b.channel.as_str() {
        "line"      => "LINE",
        "whats_app" => "WhatsApp",
        other       => other,
    };
    let actor_type_label = match b.actor_type.as_str() {
        "worker"   => "Worker",
        "supplier" => "Supplier",
        other      => other,
    };
    let created = b.created_at.format("%Y-%m-%d %H:%M UTC").to_string();

    format!(
        r#"<tr data-binding-id="{id}">
  <td><span class="channel-badge channel-badge--{channel_slug}">{channel}</span></td>
  <td class="font-mono text-xs">{external_id}</td>
  <td>{actor_type}</td>
  <td class="text-muted text-xs">{created}</td>
  <td>
    <div style="display:flex;gap:.5rem;">
      <button class="btn btn--primary btn--sm" onclick="actorConfirm('{id}')">Confirm</button>
      <button class="btn btn--ghost btn--sm"   onclick="actorReject('{id}')">Reject</button>
    </div>
  </td>
</tr>"#,
        id           = b.id,
        channel_slug = b.channel.replace('_', "-"),
        channel      = channel_label,
        external_id  = html_escape(&b.external_id),
        actor_type   = actor_type_label,
        created      = created,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}