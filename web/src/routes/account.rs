//! T13: /account/settings — change-password form for Owner and Manager.
//! SSR shell; form submits via JS fetch to /api/v1/account/change-password.
//! Changing password bumps token_version, signing out all other sessions.

use axum::{
    extract::State,
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;

use api::AppState;

use crate::auth::decode_claims_from_jar;
use crate::templates::{shell_close, shell_open};

pub async fn render_account_settings(
    State(_state): State<AppState>,
    jar: CookieJar,
) -> Response {
    let Some(claims) = decode_claims_from_jar(&jar) else {
        return Redirect::to("/login").into_response();
    };

    // Back link: first accessible branch or /branches.
    let back_href = claims
        .branch_ids
        .first()
        .map(|id| format!("/branches/{id}/orders"))
        .unwrap_or_else(|| "/branches".to_string());

    let role_label = if claims.is_owner() { "Owner" } else { "Manager" };

    let html = format!(
        r#"{shell_open}
<header class="topbar">
  <a href="/branches" class="topbar__wordmark" style="text-decoration:none;">Biz<span>·</span>Brain</a>
  <nav>
    <ul class="topbar__nav">
      <li><a href="{back_href}">← Dashboard</a></li>
    </ul>
  </nav>
  <div class="topbar__actions">
    <form method="POST" action="/logout" style="margin:0;">
      <button class="btn btn--ghost btn--sm" type="submit">Sign out</button>
    </form>
  </div>
</header>

<div class="page" style="max-width:480px;">
  <div class="page-header">
    <h1>Account settings</h1>
  </div>

  <div class="card" style="padding:var(--space-6);">
    <p class="text-sm text-muted" style="margin-bottom:var(--space-6);">
      Role: <strong>{role_label}</strong>
    </p>

    <h2 style="font-size:var(--text-base);margin-bottom:var(--space-4);">Change password</h2>

    <div id="pw-error" class="error-banner hidden" style="margin-bottom:var(--space-4);"></div>
    <div id="pw-success" class="hidden" style="
         background:var(--color-state-done-bg);
         border:1px solid var(--color-state-done);
         border-radius:var(--radius-sm);
         color:var(--color-state-done);
         font-size:var(--text-sm);
         padding:var(--space-3);
         margin-bottom:var(--space-4);">
      Password changed. Other sessions have been signed out.
    </div>

    <div style="display:flex;flex-direction:column;gap:var(--space-4);">
      <div class="form-group">
        <label class="form-label" for="current-pw">Current password</label>
        <input class="form-input" id="current-pw" type="password"
               autocomplete="current-password">
      </div>
      <div class="form-group">
        <label class="form-label" for="new-pw">New password</label>
        <input class="form-input" id="new-pw" type="password"
               autocomplete="new-password">
        <span class="text-xs text-muted">Minimum 8 characters.</span>
      </div>
      <div class="form-group">
        <label class="form-label" for="confirm-pw">Confirm new password</label>
        <input class="form-input" id="confirm-pw" type="password"
               autocomplete="new-password">
      </div>
      <div style="display:flex;justify-content:flex-end;">
        <button class="btn btn--primary" id="save-pw-btn">Save new password</button>
      </div>
    </div>
  </div>
</div>

<script>
(function () {{
  const saveBtn   = document.getElementById('save-pw-btn');
  const currentEl = document.getElementById('current-pw');
  const newEl     = document.getElementById('new-pw');
  const confirmEl = document.getElementById('confirm-pw');
  const errorEl   = document.getElementById('pw-error');
  const successEl = document.getElementById('pw-success');

  function showError(msg) {{
    errorEl.textContent = msg;
    errorEl.classList.remove('hidden');
    successEl.classList.add('hidden');
  }}

  function clearFeedback() {{
    errorEl.classList.add('hidden');
    successEl.classList.add('hidden');
  }}

  saveBtn.addEventListener('click', async () => {{
    clearFeedback();
    const current = currentEl.value;
    const next    = newEl.value;
    const confirm = confirmEl.value;

    if (!current || !next || !confirm) {{
      showError('All fields are required.');
      return;
    }}
    if (next.length < 8) {{
      showError('New password must be at least 8 characters.');
      return;
    }}
    if (next !== confirm) {{
      showError('New passwords do not match.');
      return;
    }}

    saveBtn.disabled = true;
    saveBtn.textContent = 'Saving…';

    try {{
      const res = await fetch('/api/v1/account/change-password', {{
        method: 'POST',
        headers: {{ 'Content-Type': 'application/json' }},
        body: JSON.stringify({{
          current_password: current,
          new_password: next,
        }}),
      }});

      if (res.ok) {{
        currentEl.value = '';
        newEl.value     = '';
        confirmEl.value = '';
        successEl.classList.remove('hidden');
      }} else {{
        const text = await res.text().catch(() => 'Request failed.');
        showError(text || 'Request failed.');
      }}
    }} catch (e) {{
      showError('Network error — please try again.');
    }} finally {{
      saveBtn.disabled = false;
      saveBtn.textContent = 'Save new password';
    }}
  }});
}})();
</script>
{shell_close}"#,
        shell_open = shell_open("Account Settings — Biz-Brain"),
        shell_close = shell_close(),
        back_href = back_href,
        role_label = role_label,
    );

    Html(html).into_response()
}
