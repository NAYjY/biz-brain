/**
 * Shared UI utilities: toast notifications, modal open/close,
 * confirm dialog, JSON fetch wrapper.
 */

/* ── Toast ─────────────────────────────────────────────────────────── */

let toastContainer = null;

function ensureToastContainer() {
  if (toastContainer) return toastContainer;
  toastContainer = document.createElement('div');
  toastContainer.className = 'toast-container';
  document.body.appendChild(toastContainer);
  return toastContainer;
}

function showToast(message, kind = 'info') {
  const container = ensureToastContainer();
  const el = document.createElement('div');
  el.className = `toast toast--${kind}`;
  el.textContent = message;
  container.appendChild(el);
  setTimeout(() => el.remove(), 4000);
}

/* ── Modal ──────────────────────────────────────────────────────────── */

function openModal(backdropId) {
  document.getElementById(backdropId)?.classList.remove('hidden');
}

function closeModal(backdropId) {
  document.getElementById(backdropId)?.classList.add('hidden');
}

/* ── Confirm dialog ─────────────────────────────────────────────────── */

/**
 * Returns a Promise<boolean>. Resolves true if user confirmed, false otherwise.
 * Injects a temporary backdrop + dialog; cleans up after resolution.
 */
function confirm(message) {
  return new Promise((resolve) => {
    const backdrop = document.createElement('div');
    backdrop.className = 'modal-backdrop';

    backdrop.innerHTML = `
      <div class="confirm-dialog" role="dialog" aria-modal="true">
        <p>${escapeHtml(message)}</p>
        <div class="confirm-dialog__actions">
          <button class="btn btn--ghost" data-action="cancel">Cancel</button>
          <button class="btn btn--danger" data-action="confirm">Confirm</button>
        </div>
      </div>
    `;

    document.body.appendChild(backdrop);

    backdrop.addEventListener('click', (e) => {
      const action = e.target.closest('[data-action]')?.dataset.action;
      if (!action) return;
      backdrop.remove();
      resolve(action === 'confirm');
    });
  });
}

/* ── JSON API fetch ─────────────────────────────────────────────────── */

/** Thin wrapper: throws on non-2xx with parsed error body. */
async function apiFetch(url, options = {}) {
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json', ...options.headers },
    ...options,
  });

  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(text || `HTTP ${res.status}`);
  }

  const ct = res.headers.get('content-type') ?? '';
  return ct.includes('application/json') ? res.json() : null;
}

/* ── Helpers ────────────────────────────────────────────────────────── */

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/**
 * Returns lowercase state string for CSS class suffix.
 * e.g. "PENDING_CLARIFICATION" -> "pending_clarification"
 */
function stateClass(state) {
  return state.toLowerCase();
}

/** Renders a <span class="state-pill state-pill--{cls}">{label}</span> */
function statePill(state) {
  const cls = stateClass(state);
  const label = state.replace(/_/g, ' ');
  return `<span class="state-pill state-pill--${cls}">${escapeHtml(label)}</span>`;
}

/** Short UUID display: first 8 chars. */
function shortId(uuid) {
  return uuid?.slice(0, 8) ?? '—';
}

window.BB = { showToast, openModal, closeModal, confirm, apiFetch, escapeHtml, statePill, shortId };
