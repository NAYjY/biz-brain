//! D08-5 / T11 / T16-04: Actors (pending bindings) page — fully translated, mobile card layout.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;
use domain::BranchId;
use store::PendingBinding;

use crate::auth::{auth_error_response, authorize_branch, BranchAuthOutcome};
use crate::i18n::Translations;
use crate::templates::{
    html_escape, load_topbar_data, page_not_found, shell_close, shell_open,
    topbar_html, translations_for,
};

pub async fn render_actors(
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
    let current_path = format!("/branches/{branch_id}/actors");

    let bindings = match state.actors.list_pending(BranchId::new(branch_id)).await {
        Ok(rows) => rows,
        Err(e) => { eprintln!("actors list_pending failed: {e:?}"); return page_not_found(); }
    };

    let rows_html = if bindings.is_empty() {
        format!(
            r#"<tr><td colspan="5" class="data-table__empty">{}</td></tr>"#,
            t.get("actors.empty")
        )
    } else {
        bindings.iter().map(|b| binding_row_html(b, &t)).collect::<Vec<_>>().join("\n")
    };

    // T16-04: mobile card list
    let cards_html = if bindings.is_empty() {
        format!(
            r#"<p class="actors-cards-empty">{}</p>"#,
            t.get("actors.empty")
        )
    } else {
        bindings.iter().map(|b| binding_card_html(b, &t)).collect::<Vec<_>>().join("\n")
    };

    let html = format!(
        r#"{shell_open}
{topbar}
<div class="page">
  <div class="page-header">
    <h1>{title}</h1>
    <p class="text-sm text-muted" style="margin-top:.25rem;">{subtitle}</p>
  </div>

  <!-- Desktop table (hidden on mobile) -->
  <div class="card desktop-only">
    <table class="data-table" id="actors-table">
      <thead>
        <tr>
          <th>{col_channel}</th>
          <th>{col_sender}</th>
          <th>{col_type}</th>
          <th>{col_seen}</th>
          <th>{col_actions}</th>
        </tr>
      </thead>
      <tbody id="actors-tbody">{rows_html}</tbody>
    </table>
  </div>

  <!-- Mobile card list (T16-04, hidden on desktop) -->
  <div id="actors-cards-list">{cards_html}</div>
</div>
<script src="/static/js/ui.js"></script>
<script src="/static/js/actors.js"></script>
<script>initActorsPage('{branch_id}');</script>
{shell_close}"#,
        shell_open  = shell_open(t.get("actors.page_title"), t.lang()),
        topbar      = topbar_html(branch_id, &branch_name, &all_branches, "actors", &t, &current_path),
        title       = t.get("actors.title"),
        subtitle    = t.get("actors.subtitle"),
        col_channel = t.get("actors.col.channel"),
        col_sender  = t.get("actors.col.sender"),
        col_type    = t.get("actors.col.type"),
        col_seen    = t.get("actors.col.seen"),
        col_actions = t.get("actors.col.actions"),
        rows_html   = rows_html,
        cards_html  = cards_html,
        branch_id   = branch_id,
        shell_close = shell_close(),
    );

    Html(html).into_response()
}

fn binding_row_html(b: &PendingBinding, t: &Translations) -> String {
    let channel_label = match b.channel.as_str() {
        "line"      => "LINE",
        "whats_app" => "WhatsApp",
        "telegram"  => "Telegram",
        other       => other,
    };
    let actor_type_label = match b.actor_type.as_str() {
        "worker"   => t.get("actors.type.worker"),
        "supplier" => t.get("actors.type.supplier"),
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
      <button class="btn btn--primary btn--sm" onclick="actorConfirm('{id}')">{confirm}</button>
      <button class="btn btn--ghost btn--sm"   onclick="actorReject('{id}')">{reject}</button>
    </div>
  </td>
</tr>"#,
        id           = b.id,
        channel_slug = b.channel.replace('_', "-"),
        channel      = channel_label,
        external_id  = html_escape(&b.external_id),
        actor_type   = actor_type_label,
        created      = created,
        confirm      = t.get("actors.btn.confirm"),
        reject       = t.get("actors.btn.reject"),
    )
}

/// T16-04: Mobile card for a single pending binding.
fn binding_card_html(b: &PendingBinding, t: &Translations) -> String {
    let channel_label = match b.channel.as_str() {
        "line"      => "LINE",
        "whats_app" => "WhatsApp",
        "telegram"  => "Telegram",
        other       => other,
    };
    let actor_type_label = match b.actor_type.as_str() {
        "worker"   => t.get("actors.type.worker"),
        "supplier" => t.get("actors.type.supplier"),
        other      => other,
    };
    let created = b.created_at.format("%Y-%m-%d %H:%M UTC").to_string();

    format!(
        r#"<div class="actor-card" data-binding-id="{id}">
  <div class="actor-card__header">
    <span class="channel-badge channel-badge--{channel_slug}">{channel}</span>
    <span class="actor-card__type">{actor_type}</span>
  </div>
  <span class="actor-card__sender">{external_id}</span>
  <span class="actor-card__seen">{seen_label}: {created}</span>
  <div class="actor-card__actions">
    <button class="btn btn--primary btn--sm" onclick="actorConfirm('{id}')">{confirm}</button>
    <button class="btn btn--ghost btn--sm"   onclick="actorReject('{id}')">{reject}</button>
  </div>
</div>"#,
        id           = b.id,
        channel_slug = b.channel.replace('_', "-"),
        channel      = channel_label,
        external_id  = html_escape(&b.external_id),
        actor_type   = actor_type_label,
        created      = created,
        seen_label   = t.get("actors.col.seen"),
        confirm      = t.get("actors.btn.confirm"),
        reject       = t.get("actors.btn.reject"),
    )
}