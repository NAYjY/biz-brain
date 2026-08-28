//! T12 / T13: /branches — branch list + create form.
//! Handles zero-branch state (replaces the old /setup redirect from T13).
//! Also serves as the branch switcher destination for multi-branch Owners.

use axum::{
    extract::State,
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;

use crate::auth::decode_claims_from_jar;
use crate::templates::{html_escape, shell_close, shell_open};

pub async fn render_branches(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Response {
    let Some(claims) = decode_claims_from_jar(&jar) else {
        return axum::response::Redirect::to("/login").into_response();
    };

    // Load branch names. IDs are in JWT but names are not.
    let rows: Vec<(Uuid, String)> = if claims.is_owner() {
        sqlx::query_as("SELECT id, name FROM branches ORDER BY created_at ASC")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default()
    } else {
        sqlx::query_as(
            "SELECT b.id, b.name \
             FROM branches b \
             JOIN user_branch_access uba ON uba.branch_id = b.id \
             WHERE uba.user_id = $1 \
             ORDER BY uba.granted_at ASC",
        )
        .bind(claims.sub)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default()
    };

    let is_owner = claims.is_owner();

    let branch_list_html = if rows.is_empty() {
        r#"<p class="text-muted text-sm" style="padding:var(--space-2) 0;">
             No branches yet. Create your first one below.
           </p>"#
            .to_string()
    } else {
        rows.iter()
            .map(|(id, name)| {
                format!(
                    r#"<a href="/branches/{id}/orders" class="branch-card">
  <span class="branch-card__name">{name}</span>
  <span class="branch-card__arrow">→</span>
</a>"#,
                    id = id,
                    name = html_escape(name),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Create branch form — Owner only.
    let create_section = if is_owner {
        r#"<div class="card" style="padding:var(--space-6);margin-top:var(--space-6);">
  <h2 style="font-size:var(--text-base);margin-bottom:var(--space-4);">Create a branch</h2>
  <div id="create-error" class="error-banner hidden" style="margin-bottom:var(--space-4);"></div>
  <div style="display:flex;gap:var(--space-3);align-items:flex-end;">
    <div class="form-group" style="flex:1;margin-bottom:0;">
      <label class="form-label" for="branch-name">Branch name</label>
      <input class="form-input" id="branch-name" type="text"
             placeholder="e.g. Bangkok North, Warehouse 2"
             autocomplete="off">
    </div>
    <button class="btn btn--primary" id="create-branch-btn">Create</button>
  </div>
</div>

<script>
(function () {
  const btn     = document.getElementById('create-branch-btn');
  const nameEl  = document.getElementById('branch-name');
  const errorEl = document.getElementById('create-error');

  function showError(msg) {
    errorEl.textContent = msg;
    errorEl.classList.remove('hidden');
  }

  btn.addEventListener('click', async () => {
    const name = nameEl.value.trim();
    if (!name) { showError('Branch name is required.'); return; }
    errorEl.classList.add('hidden');
    btn.disabled = true;
    btn.textContent = 'Creating…';

    try {
      const res = await fetch('/api/v1/branches', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name }),
      });

      if (res.ok) {
        const data = await res.json();
        window.location.href = `/branches/${data.id}/orders`;
      } else {
        const text = await res.text().catch(() => 'Request failed.');
        showError(text || 'Request failed.');
        btn.disabled = false;
        btn.textContent = 'Create';
      }
    } catch (e) {
      showError('Network error — please try again.');
      btn.disabled = false;
      btn.textContent = 'Create';
    }
  });

  nameEl.addEventListener('keydown', (e) => { if (e.key === 'Enter') btn.click(); });
})();
</script>"#
            .to_string()
    } else {
        String::new()
    };

    let html = format!(
        r#"{shell_open}
<header class="topbar">
  <a href="/branches" class="topbar__wordmark" style="text-decoration:none;">Biz<span>·</span>Brain</a>
  <div class="topbar__actions">
    <a href="/account/settings" class="btn btn--ghost btn--sm">Account</a>
    <form method="POST" action="/logout" style="margin:0;">
      <button class="btn btn--ghost btn--sm" type="submit">Sign out</button>
    </form>
  </div>
</header>

<div class="page" style="max-width:600px;">
  <div class="page-header">
    <h1>Branches</h1>
  </div>

  <div class="card" style="padding:var(--space-6);">
    <div style="display:flex;flex-direction:column;gap:var(--space-3);">
      {branch_list_html}
    </div>
  </div>

  {create_section}
</div>

<style>
.branch-card {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-4);
  background: var(--color-surface-2);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  text-decoration: none;
  color: var(--color-text);
  transition: border-color .15s, background .15s;
}}
.branch-card:hover {{
  border-color: var(--color-accent);
  background: var(--color-surface);
  text-decoration: none;
}}
.branch-card__name {{
  font-weight: 500;
  font-size: var(--text-base);
}}
.branch-card__arrow {{
  color: var(--color-text-muted);
  font-size: var(--text-lg);
}}
</style>
{shell_close}"#,
        shell_open = shell_open("Branches — Biz-Brain"),
        shell_close = shell_close(),
        branch_list_html = branch_list_html,
        create_section = create_section,
    );

    Html(html).into_response()
}
