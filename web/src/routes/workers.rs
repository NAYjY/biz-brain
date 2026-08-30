//! Worker onboarding page — T11: fully translated.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;
use crate::auth::{auth_error_response, authorize_branch, BranchAuthOutcome};
use crate::i18n::Translations;
use crate::templates::{
    html_escape, load_topbar_data, page_not_found, shell_close, shell_open,
    topbar_html, translations_for,
};

pub async fn render_workers(
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
    let current_path = format!("/branches/{branch_id}/workers");

    let rows: Vec<(Uuid, String, Option<String>, Option<String>)> = match sqlx::query_as(
        "SELECT w.id, w.name, ad.channel, ad.external_id \
         FROM workers w \
         LEFT JOIN actor_directory ad \
           ON ad.actor_id = w.id AND ad.actor_type = 'worker' AND ad.owner_confirmed = TRUE \
         WHERE w.branch_id = $1 ORDER BY w.name ASC",
    )
    .bind(branch_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(r) => r,
        Err(e) => { eprintln!("workers query failed: {e}"); return page_not_found(); }
    };

    let rows_html = if rows.is_empty() {
        format!(
            r#"<tr><td colspan="4" class="data-table__empty">{}</td></tr>"#,
            t.get("workers.empty")
        )
    } else {
        rows.iter()
            .map(|(id, name, channel, external_id)| {
                worker_row_html(*id, name, channel.as_deref(), external_id.as_deref(), &t)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let actors_href = format!("/branches/{branch_id}/actors");

    let html = format!(
        r#"{shell_open}
{topbar}
<div class="page">
  <div class="page-header">
    <h1>{title}</h1>
    <button class="btn btn--primary" onclick="BB.openModal('create-worker-modal')">{add_btn}</button>
  </div>

  <div class="card" style="margin-bottom:var(--space-6);">
    <div style="padding:var(--space-4);border-bottom:1px solid var(--color-border);">
      <h3 style="font-size:var(--text-sm);color:var(--color-text-muted);font-weight:500;">
        {onboard_heading}
      </h3>
    </div>
    <ol style="padding:var(--space-4) var(--space-4) var(--space-4) var(--space-8);
               display:flex;flex-direction:column;gap:var(--space-2);
               color:var(--color-text-muted);font-size:var(--text-sm);">
      <li>{step1}</li>
      <li>{step2}</li>
      <li>{step3_pre} <a href="{actors_href}">{step3_link}</a> {step3_post}</li>
      <li>{step4}</li>
    </ol>
  </div>

  <div class="card">
    <table class="data-table" id="workers-table">
      <thead>
        <tr>
          <th>{col_name}</th><th>{col_channel}</th><th>{col_sender}</th><th>{col_actions}</th>
        </tr>
      </thead>
      <tbody id="workers-tbody">{rows_html}</tbody>
    </table>
  </div>
</div>

<!-- Create Worker modal -->
<div class="modal-backdrop hidden" id="create-worker-modal">
  <div class="modal">
    <div class="modal__header">
      <span class="modal__title">{modal_title}</span>
      <button class="btn btn--ghost btn--sm" onclick="BB.closeModal('create-worker-modal')">✕</button>
    </div>
    <div class="modal__body">
      <div class="form-group">
        <label class="form-label" for="worker-name">{name_label}</label>
        <input class="form-input" id="worker-name" type="text"
               placeholder="{name_ph}" autocomplete="off">
      </div>
      <p class="text-xs text-muted" style="margin-top:-.5rem;">{hint}</p>
    </div>
    <div class="modal__footer">
      <button class="btn btn--ghost" onclick="BB.closeModal('create-worker-modal')">{cancel}</button>
      <button class="btn btn--primary" id="create-worker-btn">{create_btn}</button>
    </div>
  </div>
</div>

<script src="/static/js/ui.js"></script>
<script src="/static/js/workers.js"></script>
<script>initWorkersPage('{branch_id}');</script>
{shell_close}"#,
        shell_open     = shell_open(t.get("workers.page_title"), t.lang()),
        topbar         = topbar_html(branch_id, &branch_name, &all_branches, "workers", &t, &current_path),
        title          = t.get("workers.title"),
        add_btn        = t.get("workers.add"),
        onboard_heading= t.get("workers.onboarding.heading"),
        step1          = t.get("workers.onboarding.step1"),
        step2          = t.get("workers.onboarding.step2"),
        step3_pre      = t.get("workers.onboarding.step3_pre"),
        step3_link     = t.get("workers.onboarding.step3_link"),
        step3_post     = t.get("workers.onboarding.step3_post"),
        step4          = t.get("workers.onboarding.step4"),
        actors_href    = actors_href,
        col_name       = t.get("workers.col.name"),
        col_channel    = t.get("workers.col.channel"),
        col_sender     = t.get("workers.col.sender"),
        col_actions    = t.get("workers.col.actions"),
        rows_html      = rows_html,
        modal_title    = t.get("workers.create.title"),
        name_label     = t.get("workers.create.name_label"),
        name_ph        = t.get("workers.create.name_ph"),
        hint           = t.get("workers.create.hint"),
        cancel         = t.get("btn.cancel"),
        create_btn     = t.get("workers.create.btn"),
        branch_id      = branch_id,
        shell_close    = shell_close(),
    );

    Html(html).into_response()
}

fn worker_row_html(
    id: Uuid,
    name: &str,
    channel: Option<&str>,
    external_id: Option<&str>,
    t: &Translations,
) -> String {
    let (binding_cell, channel_cell) = match (channel, external_id) {
        (Some(ch), Some(ext)) => {
            let label = match ch {
                "line"      => "LINE",
                "whats_app" => "WhatsApp",
                "telegram"  => "Telegram",
                other       => other,
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
            format!(r#"<span class="text-muted text-xs">{}</span>"#, t.get("workers.not_bound")),
            r#"<span class="text-muted text-xs">—</span>"#.to_string(),
        ),
    };

    format!(
        r#"<tr data-worker-id="{id}">
  <td>{name}</td>
  <td>{binding_cell}</td>
  <td>{channel_cell}</td>
  <td><button class="btn btn--ghost btn--sm" onclick="workerDelete('{id}')">{remove}</button></td>
</tr>"#,
        id           = id,
        name         = html_escape(name),
        binding_cell = binding_cell,
        channel_cell = channel_cell,
        remove       = t.get("btn.remove"),
    )
}
