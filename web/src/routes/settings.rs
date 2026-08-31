//! T21: Branch Settings page — Owner-only SSR page.
//! Sections: branch rename, AI provider, Manager table (grant/revoke).
//! T11: rendered in detected locale; chrome strings go through Translations.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use api::AppState;
use crate::auth::{authorize_branch, BranchAuthOutcome};
use crate::templates::{
    html_escape, load_topbar_data, shell_close, shell_open,
    topbar_html, translations_for,
};

pub async fn render_settings(
    Path(branch_id): Path<Uuid>,
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Response {
    let outcome = authorize_branch(&jar, &state.pool, branch_id).await;
    let claims = match outcome {
        BranchAuthOutcome::Authorized { claims, .. } => claims,
        BranchAuthOutcome::Unauthenticated => {
            return Redirect::to("/login").into_response()
        }
        BranchAuthOutcome::Forbidden => {
            return StatusCode::FORBIDDEN.into_response()
        }
    };

    // Settings page is Owner-only — Managers get a clean 403.
    if !claims.is_owner() {
        return (
            StatusCode::FORBIDDEN,
            Html("<h1>Owner access required for branch settings.</h1>".to_string()),
        )
            .into_response();
    }

    let t = translations_for(&jar, &headers);
    let (branch_name, all_branches) = load_topbar_data(&state.pool, branch_id, &claims).await;
    let current_path = format!("/branches/{branch_id}/settings");

    // AI provider for this branch.
    let ai_provider: String = sqlx::query_scalar(
        "SELECT ai_provider FROM branches WHERE id = $1",
    )
    .bind(branch_id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| "claude".to_string());

    // Managers with access to this branch.
    let managers: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT u.id, u.email \
         FROM users u \
         JOIN user_branch_access uba ON uba.user_id = u.id \
         WHERE uba.branch_id = $1 AND u.role = 'manager' \
         ORDER BY u.email ASC",
    )
    .bind(branch_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let managers_html = if managers.is_empty() {
        r#"<tr><td colspan="2" class="data-table__empty">No managers have access to this branch yet.</td></tr>"#
            .to_string()
    } else {
        managers
            .iter()
            .map(|(id, email)| manager_row_html(*id, email))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let claude_sel = if ai_provider == "claude" { " selected" } else { "" };
    let gemini_sel = if ai_provider == "gemini" { " selected" } else { "" };

    let html = format!(
        r#"{shell_open}
{topbar}
<div class="page" style="max-width:700px;">
  <div class="page-header"><h1>⚙️ Branch Settings</h1></div>

  <!-- ── Branch name ──────────────────────────────────────────────── -->
  <div class="card" style="padding:var(--space-6);margin-bottom:var(--space-6);">
    <h2 style="font-size:var(--text-base);font-weight:600;margin-bottom:var(--space-4);">Branch name</h2>
    <div style="display:flex;gap:var(--space-3);align-items:flex-end;">
      <div class="form-group" style="flex:1;margin-bottom:0;">
        <label class="form-label" for="branch-name-input">Name</label>
        <input class="form-input" id="branch-name-input" type="text"
               value="{branch_name_escaped}" maxlength="255" autocomplete="off">
      </div>
      <button class="btn btn--primary" id="rename-branch-btn">Save</button>
    </div>
    <div id="rename-feedback"
         style="margin-top:var(--space-3);font-size:var(--text-sm);display:none;"></div>
  </div>

  <!-- ── AI Provider ──────────────────────────────────────────────── -->
  <div class="card" style="padding:var(--space-6);margin-bottom:var(--space-6);">
    <h2 style="font-size:var(--text-base);font-weight:600;margin-bottom:var(--space-2);">AI provider</h2>
    <p class="text-sm text-muted" style="margin-bottom:var(--space-4);">
      Controls which model classifies inbound Worker and Supplier messages for this branch.
      Claude is the default and recommended option.
    </p>
    <div style="display:flex;gap:var(--space-3);align-items:flex-end;">
      <div class="form-group" style="margin-bottom:0;">
        <label class="form-label" for="ai-provider-select">Provider</label>
        <select class="form-select" id="ai-provider-select">
          <option value="claude"{claude_sel}>Claude (Anthropic) — default</option>
          <option value="gemini"{gemini_sel}>Gemini (Google)</option>
        </select>
      </div>
      <button class="btn btn--primary" id="save-ai-provider-btn">Save</button>
    </div>
    <div id="ai-feedback"
         style="margin-top:var(--space-3);font-size:var(--text-sm);display:none;"></div>
  </div>

  <!-- ── Managers ─────────────────────────────────────────────────── -->
  <div class="card" style="padding:var(--space-6);margin-bottom:var(--space-6);">
    <h2 style="font-size:var(--text-base);font-weight:600;margin-bottom:var(--space-2);">Managers</h2>
    <p class="text-sm text-muted" style="margin-bottom:var(--space-4);">
      Managers can view and operate orders, workers, and suppliers for this branch,
      but cannot access branch settings or manage other users.
    </p>

    <table class="data-table" style="margin-bottom:var(--space-6);">
      <thead>
        <tr>
          <th>Email</th>
          <th style="width:120px;"></th>
        </tr>
      </thead>
      <tbody id="managers-tbody">{managers_html}</tbody>
    </table>

    <hr style="border-color:var(--color-border);margin-bottom:var(--space-6);">

    <h3 style="font-size:var(--text-sm);font-weight:600;margin-bottom:var(--space-2);">Add Manager</h3>
    <p class="text-xs text-muted" style="margin-bottom:var(--space-4);">
      Enter the Manager's email. If they don't have a Biz-Brain account yet, also set a password
      to create one for them. If they already have an account, leave the password blank.
    </p>

    <div id="add-manager-error" class="error-banner hidden"
         style="margin-bottom:var(--space-4);"></div>
    <div id="add-manager-success" class="hidden"
         style="background:var(--color-state-done-bg);border:1px solid var(--color-state-done);
                border-radius:var(--radius-sm);color:var(--color-state-done);
                font-size:var(--text-sm);padding:var(--space-3);margin-bottom:var(--space-4);">
    </div>

    <div style="display:flex;flex-direction:column;gap:var(--space-4);">
      <div class="form-group">
        <label class="form-label" for="mgr-email">Email</label>
        <input class="form-input" id="mgr-email" type="email"
               placeholder="manager@example.com" autocomplete="off">
      </div>
      <div class="form-group">
        <label class="form-label" for="mgr-password">
          Password
          <span class="text-muted" style="font-weight:400;">
            (only needed for a new account — min 8 chars)
          </span>
        </label>
        <input class="form-input" id="mgr-password" type="password"
               placeholder="Leave blank to grant an existing account"
               autocomplete="new-password">
      </div>
      <div>
        <button class="btn btn--primary" id="add-manager-btn">Grant Access</button>
      </div>
    </div>
  </div>

</div>

<script src="/static/js/ui.js"></script>
<script>
(function () {{
  const branchId = '{branch_id}';

  // Thin fetch wrapper — identical contract to the one in ui.js BB.apiFetch.
  async function api(path, opts) {{
    const res = await fetch(`/api/v1${{path}}`, {{
      headers: {{ 'Content-Type': 'application/json', ...(opts?.headers ?? {{}}) }},
      ...opts,
    }});
    if (!res.ok) {{
      const text = await res.text().catch(() => res.statusText);
      throw new Error(text || `HTTP ${{res.status}}`);
    }}
    const ct = res.headers.get('content-type') ?? '';
    return ct.includes('application/json') ? res.json() : null;
  }}

  // ── Rename branch ─────────────────────────────────────────────── //

  document.getElementById('rename-branch-btn').addEventListener('click', async () => {{
    const name = document.getElementById('branch-name-input').value.trim();
    const fb   = document.getElementById('rename-feedback');
    if (!name) {{
      showFeedback(fb, 'Name required.', false);
      return;
    }}
    try {{
      await api(`/branches/${{branchId}}`, {{
        method: 'PATCH',
        body: JSON.stringify({{ name }}),
      }});
      showFeedback(fb, '✓ Branch renamed.', true);
    }} catch (e) {{
      showFeedback(fb, `Error: ${{e.message}}`, false);
    }}
  }});

  // ── AI provider ───────────────────────────────────────────────── //

  document.getElementById('save-ai-provider-btn').addEventListener('click', async () => {{
    const provider = document.getElementById('ai-provider-select').value;
    const fb       = document.getElementById('ai-feedback');
    try {{
      await api(`/branches/${{branchId}}/ai-provider`, {{
        method: 'PATCH',
        body: JSON.stringify({{ provider }}),
      }});
      const label = provider === 'gemini' ? 'Gemini' : 'Claude';
      showFeedback(fb, `✓ AI provider set to ${{label}}.`, true);
    }} catch (e) {{
      showFeedback(fb, `Error: ${{e.message}}`, false);
    }}
  }});

  // ── Add Manager ───────────────────────────────────────────────── //

  document.getElementById('add-manager-btn').addEventListener('click', async () => {{
    const email    = document.getElementById('mgr-email').value.trim();
    const password = document.getElementById('mgr-password').value;
    const errEl    = document.getElementById('add-manager-error');
    const okEl     = document.getElementById('add-manager-success');

    errEl.classList.add('hidden');
    okEl.classList.add('hidden');

    if (!email) {{
      errEl.textContent = 'Email is required.';
      errEl.classList.remove('hidden');
      return;
    }}

    const btn = document.getElementById('add-manager-btn');
    btn.disabled    = true;
    btn.textContent = 'Working…';

    try {{
      let managerId;

      if (password) {{
        // Create new Manager account, then grant access.
        if (password.length < 8) {{
          throw new Error('Password must be at least 8 characters.');
        }}
        const created = await api('/managers', {{
          method: 'POST',
          body: JSON.stringify({{ email, password }}),
        }});
        managerId = created.id;
      }} else {{
        // Look up an existing Manager by email.
        const found = await api(`/managers?email=${{encodeURIComponent(email)}}`);
        managerId = found.id;
      }}

      // Grant branch access.
      await api(`/branches/${{branchId}}/managers`, {{
        method: 'POST',
        body: JSON.stringify({{ manager_id: managerId }}),
      }});

      // Append row without full page reload.
      addManagerRow(managerId, email);

      // Reset form.
      document.getElementById('mgr-email').value    = '';
      document.getElementById('mgr-password').value = '';

      okEl.textContent = `✓ ${{email}} now has access to this branch.`;
      okEl.classList.remove('hidden');
      setTimeout(() => okEl.classList.add('hidden'), 5000);
    }} catch (e) {{
      errEl.textContent = e.message;
      errEl.classList.remove('hidden');
    }} finally {{
      btn.disabled    = false;
      btn.textContent = 'Grant Access';
    }}
  }});

  function addManagerRow(id, email) {{
    const tbody    = document.getElementById('managers-tbody');
    const emptyRow = tbody.querySelector('td[colspan]');
    if (emptyRow) emptyRow.closest('tr')?.remove();

    const tr = document.createElement('tr');
    tr.dataset.managerId = id;
    tr.innerHTML = `
      <td>${{escapeHtml(email)}}</td>
      <td><button class="btn btn--ghost btn--sm"
                  onclick="revokeManager('${{id}}', '${{escapeHtml(email)}}')">Revoke</button></td>`;
    tbody.appendChild(tr);
  }}

  // ── Revoke Manager ────────────────────────────────────────────── //

  window.revokeManager = async (managerId, email) => {{
    const ok = await BB.confirm(
      `Revoke ${{email}}'s access to this branch? They will be signed out on their next request.`
    );
    if (!ok) return;
    try {{
      await api(`/branches/${{branchId}}/managers/${{managerId}}`, {{ method: 'DELETE' }});
      const row = document.querySelector(`tr[data-manager-id="${{managerId}}"]`);
      row?.remove();
      const tbody = document.getElementById('managers-tbody');
      if (tbody.querySelectorAll('tr[data-manager-id]').length === 0) {{
        tbody.innerHTML =
          '<tr><td colspan="2" class="data-table__empty">No managers have access to this branch yet.</td></tr>';
      }}
      BB.showToast(`${{email}} access revoked.`, 'success');
    }} catch (e) {{
      BB.showToast(`Revoke failed: ${{e.message}}`, 'error');
    }}
  }};

  // ── Helpers ───────────────────────────────────────────────────── //

  function showFeedback(el, msg, ok) {{
    el.textContent  = msg;
    el.style.color  = ok ? 'var(--color-state-done)' : 'var(--color-state-err)';
    el.style.display = '';
    setTimeout(() => {{ el.style.display = 'none'; }}, 4000);
  }}

  function escapeHtml(s) {{
    return String(s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;')
      .replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }}
}})();
</script>
{shell_close}"#,
        shell_open          = shell_open("Branch Settings — Biz-Brain", t.lang()),
        topbar              = topbar_html(branch_id, &branch_name, &all_branches, "settings", &t, &current_path),
        branch_name_escaped = html_escape(&branch_name),
        claude_sel          = claude_sel,
        gemini_sel          = gemini_sel,
        managers_html       = managers_html,
        branch_id           = branch_id,
        shell_close         = shell_close(),
    );

    Html(html).into_response()
}

fn manager_row_html(id: Uuid, email: &str) -> String {
    format!(
        r#"<tr data-manager-id="{id}">
  <td>{email}</td>
  <td><button class="btn btn--ghost btn--sm"
              onclick="revokeManager('{id}', '{email_js}')">Revoke</button></td>
</tr>"#,
        id       = id,
        email    = html_escape(email),
        email_js = html_escape(email),
    )
}