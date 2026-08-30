//! T13 / T11: /account/settings — fully translated.

use axum::{
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;

use api::AppState;
use crate::auth::decode_claims_from_jar;
use crate::templates::{shell_close, shell_open, translations_for};

pub async fn render_account_settings(
    State(_state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Response {
    let Some(claims) = decode_claims_from_jar(&jar) else {
        return Redirect::to("/login").into_response();
    };
    let t = translations_for(&jar, &headers);

    let back_href = claims
        .branch_ids
        .first()
        .map(|id| format!("/branches/{id}/orders"))
        .unwrap_or_else(|| "/branches".to_string());

    let role_key = if claims.is_owner() { "account.role.owner" } else { "account.role.manager" };

    let html = format!(
        r#"{shell_open}
<header class="topbar">
  <a href="/branches" class="topbar__wordmark" style="text-decoration:none;">Biz<span>·</span>Brain</a>
  <nav>
    <ul class="topbar__nav">
      <li><a href="{back_href}">{back}</a></li>
    </ul>
  </nav>
  <div class="topbar__actions">
    <form method="POST" action="/logout" style="margin:0;">
      <button class="btn btn--ghost btn--sm" type="submit">{sign_out}</button>
    </form>
  </div>
</header>

<div class="page" style="max-width:480px;">
  <div class="page-header"><h1>{title}</h1></div>

  <div class="card" style="padding:var(--space-6);">
    <p class="text-sm text-muted" style="margin-bottom:var(--space-6);">
      {role_label}: <strong>{role_value}</strong>
    </p>

    <h2 style="font-size:var(--text-base);margin-bottom:var(--space-4);">{change_pw_heading}</h2>

    <div id="pw-error" class="error-banner hidden" style="margin-bottom:var(--space-4);"></div>
    <div id="pw-success" class="hidden" style="
         background:var(--color-state-done-bg);border:1px solid var(--color-state-done);
         border-radius:var(--radius-sm);color:var(--color-state-done);
         font-size:var(--text-sm);padding:var(--space-3);margin-bottom:var(--space-4);">
      {pw_success}
    </div>

    <div style="display:flex;flex-direction:column;gap:var(--space-4);">
      <div class="form-group">
        <label class="form-label" for="current-pw">{current_pw}</label>
        <input class="form-input" id="current-pw" type="password" autocomplete="current-password">
      </div>
      <div class="form-group">
        <label class="form-label" for="new-pw">{new_pw}</label>
        <input class="form-input" id="new-pw" type="password" autocomplete="new-password">
        <span class="text-xs text-muted">{new_pw_hint}</span>
      </div>
      <div class="form-group">
        <label class="form-label" for="confirm-pw">{confirm_pw}</label>
        <input class="form-input" id="confirm-pw" type="password" autocomplete="new-password">
      </div>
      <div style="display:flex;justify-content:flex-end;">
        <button class="btn btn--primary" id="save-pw-btn">{save_pw}</button>
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

  const msg_fields   = {msg_fields_json};
  const msg_short    = {msg_short_json};
  const msg_match    = {msg_match_json};
  const msg_network  = {msg_network_json};
  const label_saving = {label_saving_json};
  const label_save   = {label_save_json};

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
    if (!current || !next || !confirm) {{ showError(msg_fields); return; }}
    if (next.length < 8) {{ showError(msg_short); return; }}
    if (next !== confirm) {{ showError(msg_match); return; }}

    saveBtn.disabled = true;
    saveBtn.textContent = label_saving;
    try {{
      const res = await fetch('/api/v1/account/change-password', {{
        method: 'POST',
        headers: {{ 'Content-Type': 'application/json' }},
        body: JSON.stringify({{ current_password: current, new_password: next }}),
      }});
      if (res.ok) {{
        currentEl.value = ''; newEl.value = ''; confirmEl.value = '';
        successEl.classList.remove('hidden');
      }} else {{
        const text = await res.text().catch(() => res.statusText);
        showError(text || 'Request failed.');
      }}
    }} catch (e) {{
      showError(msg_network);
    }} finally {{
      saveBtn.disabled = false;
      saveBtn.textContent = label_save;
    }}
  }});
}})();
</script>
{shell_close}"#,
        shell_open          = shell_open(t.get("account.page_title"), t.lang()),
        shell_close         = shell_close(),
        title               = t.get("account.title"),
        back                = t.get("account.back"),
        back_href           = back_href,
        sign_out            = t.get("nav.sign_out"),
        role_label          = t.get("account.role"),
        role_value          = t.get(role_key),
        change_pw_heading   = t.get("account.change_password"),
        current_pw          = t.get("account.current_pw"),
        new_pw              = t.get("account.new_pw"),
        new_pw_hint         = t.get("account.new_pw_hint"),
        confirm_pw          = t.get("account.confirm_pw"),
        save_pw             = t.get("account.save_pw"),
        pw_success          = t.get("account.pw_success"),
        // Inline JS strings — JSON-encoded so quotes inside don't break the script
        msg_fields_json     = serde_json::to_string(t.get("account.error.fields_required")).unwrap_or_default(),
        msg_short_json      = serde_json::to_string(t.get("account.error.too_short")).unwrap_or_default(),
        msg_match_json      = serde_json::to_string(t.get("account.error.no_match")).unwrap_or_default(),
        msg_network_json    = serde_json::to_string(t.get("account.error.network")).unwrap_or_default(),
        label_saving_json   = serde_json::to_string(t.get("account.saving_pw")).unwrap_or_default(),
        label_save_json     = serde_json::to_string(t.get("account.save_pw")).unwrap_or_default(),
    );

    Html(html).into_response()
}
