/**
 * F05: Order dates + follow-up alerts UI.
 *
 * Exported functions called from orders.js:
 *   openDatesModal(orderId, currentStartDate, currentDueDate, api)
 *   openAlertsModal(orderId, orderDesc, api)
 *
 * Both are standalone modals appended to document.body.
 */

// ── Date helpers ──────────────────────────────────────────────────────────── //

/**
 * Format a UTC ISO string for display in the owner's local timezone.
 * e.g. "2025-09-01T10:00:00Z" → "01 Sep 2025 10:00"
 */
function fmtDate(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  return d.toLocaleString([], {
    day: '2-digit', month: 'short', year: 'numeric',
    hour: '2-digit', minute: '2-digit',
  });
}

/**
 * Convert a local datetime-local input value ("2025-09-01T10:00")
 * to an ISO-8601 UTC string.
 */
function localInputToUtc(val) {
  if (!val) return null;
  return new Date(val).toISOString();
}

/**
 * Convert a UTC ISO string to a value suitable for <input type="datetime-local">.
 * Steps back from UTC to local time.
 */
function utcToLocalInput(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  // Subtract timezone offset to get local time in ISO format, strip seconds+Z.
  const local = new Date(d.getTime() - d.getTimezoneOffset() * 60000);
  return local.toISOString().slice(0, 16);
}

/**
 * Returns true if the given ISO string is in the past.
 */
function isOverdue(iso) {
  return iso && new Date(iso) < new Date();
}

// ── Date chips renderer (called from orders.js row builder) ──────────────── //

/**
 * Returns HTML string for start + due date chips.
 * Pass null/undefined to omit a chip.
 */
window.renderDateChips = function(startDate, dueDate) {
  let html = '';
  if (startDate) {
    html += `<span class="date-chip date-chip--start" title="Start date">▶ ${fmtDate(startDate)}</span>`;
  }
  if (dueDate) {
    const cls = isOverdue(dueDate) ? 'date-chip--overdue' : 'date-chip--due';
    const icon = isOverdue(dueDate) ? '⚠️' : '⏰';
    html += `<span class="date-chip ${cls}" title="Due date">${icon} ${fmtDate(dueDate)}</span>`;
  }
  return html;
};

// ── Dates modal ───────────────────────────────────────────────────────────── //

window.openDatesModal = function(orderId, startDateIso, dueDateIso, api) {
  const backdrop = document.createElement('div');
  backdrop.className = 'modal-backdrop';
  backdrop.innerHTML = `
    <div class="modal" style="width:440px;">
      <div class="modal__header">
        <span class="modal__title">📅 Set dates</span>
        <button class="btn btn--ghost btn--sm" data-action="cancel">✕</button>
      </div>
      <div class="modal__body">
        <div class="form-group">
          <label class="form-label" for="f05-start">Start date &amp; time</label>
          <input class="form-input" id="f05-start" type="datetime-local"
                 value="${utcToLocalInput(startDateIso)}">
          <span class="text-xs text-muted">Leave blank to clear</span>
        </div>
        <div class="form-group">
          <label class="form-label" for="f05-due">Due date &amp; time</label>
          <input class="form-input" id="f05-due" type="datetime-local"
                 value="${utcToLocalInput(dueDateIso)}">
          <span class="text-xs text-muted">Leave blank to clear</span>
        </div>
      </div>
      <div class="modal__footer">
        <button class="btn btn--ghost" data-action="cancel">Cancel</button>
        <button class="btn btn--primary" data-action="save">Save</button>
      </div>
    </div>`;

  document.body.appendChild(backdrop);
  backdrop.querySelector('#f05-start').focus();

  backdrop.addEventListener('click', async (e) => {
    const action = e.target.closest('[data-action]')?.dataset.action;
    if (!action) return;
    if (action === 'cancel') { backdrop.remove(); return; }

    const startVal = backdrop.querySelector('#f05-start').value;
    const dueVal   = backdrop.querySelector('#f05-due').value;

    const startUtc = localInputToUtc(startVal);
    const dueUtc   = localInputToUtc(dueVal);

    if (startUtc && dueUtc && new Date(startUtc) > new Date(dueUtc)) {
      BB.showToast('Start date must be before due date', 'error');
      return;
    }

    const saveBtn = backdrop.querySelector('[data-action="save"]');
    saveBtn.disabled = true;
    saveBtn.textContent = 'Saving…';

    try {
      await api(`/orders/${orderId}/dates`, {
        method: 'PATCH',
        body: JSON.stringify({ start_date: startUtc, due_date: dueUtc }),
      });
      backdrop.remove();
      BB.showToast('Dates saved ✓', 'success');
    } catch (err) {
      saveBtn.disabled = false;
      saveBtn.textContent = 'Save';
      BB.showToast(`Save failed: ${err.message}`, 'error');
    }
  });
};

// ── Alerts modal ──────────────────────────────────────────────────────────── //

window.openAlertsModal = async function(orderId, orderDesc, api) {
  // Load existing alerts first.
  let existing = [];
  try {
    existing = await api(`/orders/${orderId}/alerts`);
  } catch (e) {
    BB.showToast(`Could not load alerts: ${e.message}`, 'error');
    return;
  }

  const backdrop = document.createElement('div');
  backdrop.className = 'modal-backdrop';
  backdrop.innerHTML = `
    <div class="modal" style="width:500px;max-height:90vh;">
      <div class="modal__header">
        <span class="modal__title">🔔 Follow-up alerts — ${BB.escapeHtml(orderDesc)}</span>
        <button class="btn btn--ghost btn--sm" data-action="close">✕</button>
      </div>
      <div class="modal__body">

        <!-- Existing alerts -->
        <div id="f05-alert-list" class="alert-list">
          ${existing.length === 0
            ? '<p class="text-sm text-muted">No active alerts.</p>'
            : existing.map(alertItemHtml).join('')}
        </div>

        <hr style="border-color:var(--color-border);margin:var(--space-2) 0;">

        <!-- Create new alert -->
        <p class="form-label" style="margin-bottom:var(--space-2);">Add alert</p>

        <div class="form-group">
          <label class="form-label">Mode</label>
          <div class="alert-mode-tabs" id="f05-mode-tabs">
            ${['once','hourly','daily','every3d'].map(m =>
              `<button class="alert-mode-tab${m === 'once' ? ' selected' : ''}"
                       data-mode="${m}">${modeLabel(m)}</button>`
            ).join('')}
          </div>
        </div>

        <div class="form-group">
          <label class="form-label" for="f05-alert-at">
            <span id="f05-at-label">Alert at</span>
          </label>
          <input class="form-input" id="f05-alert-at" type="datetime-local">
          <span class="text-xs text-muted" id="f05-at-hint">
            One-time alert at this exact date &amp; time.
          </span>
        </div>

        <div class="form-group">
          <label class="form-label" for="f05-alert-msg">
            Custom message <span class="text-muted" style="font-weight:400;">(optional)</span>
          </label>
          <textarea class="form-textarea" id="f05-alert-msg"
                    style="min-height:56px;"
                    placeholder="Leave blank for default reminder text"></textarea>
        </div>

      </div>
      <div class="modal__footer">
        <button class="btn btn--ghost" data-action="close">Close</button>
        <button class="btn btn--primary" data-action="add">Add alert</button>
      </div>
    </div>`;

  document.body.appendChild(backdrop);

  // Mode tab switching.
  let selectedMode = 'once';
  backdrop.querySelectorAll('.alert-mode-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      selectedMode = tab.dataset.mode;
      backdrop.querySelectorAll('.alert-mode-tab').forEach(t => t.classList.remove('selected'));
      tab.classList.add('selected');
      updateHint(backdrop, selectedMode);
    });
  });

  // Delete existing alert.
  backdrop.addEventListener('click', async (e) => {
    const deleteBtn = e.target.closest('[data-delete-alert]');
    if (deleteBtn) {
      const alertId = deleteBtn.dataset.deleteAlert;
      try {
        await api(`/orders/${orderId}/alerts/${alertId}`, { method: 'DELETE' });
        deleteBtn.closest('.alert-item')?.remove();
        const list = backdrop.querySelector('#f05-alert-list');
        if (list && list.querySelectorAll('.alert-item').length === 0) {
          list.innerHTML = '<p class="text-sm text-muted">No active alerts.</p>';
        }
        BB.showToast('Alert removed', 'success');
        refreshAlertBtn(orderId);
      } catch (err) {
        BB.showToast(`Remove failed: ${err.message}`, 'error');
      }
      return;
    }

    const action = e.target.closest('[data-action]')?.dataset.action;
    if (!action) return;

    if (action === 'close') { backdrop.remove(); return; }

    if (action === 'add') {
      const atVal  = backdrop.querySelector('#f05-alert-at').value;
      const msgVal = backdrop.querySelector('#f05-alert-msg').value.trim();

      if (!atVal) { BB.showToast('Please set a date/time', 'error'); return; }
      const atUtc = localInputToUtc(atVal);
      if (new Date(atUtc) <= new Date()) {
        BB.showToast('Alert time must be in the future', 'error');
        return;
      }

      const addBtn = backdrop.querySelector('[data-action="add"]');
      addBtn.disabled = true;
      addBtn.textContent = 'Adding…';

      try {
        const result = await api(`/orders/${orderId}/alerts`, {
          method: 'POST',
          body: JSON.stringify({
            alert_mode: selectedMode,
            alert_at: atUtc,
            message: msgVal || null,
          }),
        });

        // Append to list immediately.
        const newAlert = {
          id: result.id,
          alert_mode: selectedMode,
          alert_at: atUtc,
          next_fire_at: atUtc,
          message: msgVal || null,
          fired_count: 0,
        };
        const list = backdrop.querySelector('#f05-alert-list');
        const emptyMsg = list.querySelector('p.text-muted');
        if (emptyMsg) emptyMsg.remove();
        list.insertAdjacentHTML('beforeend', alertItemHtml(newAlert));

        // Reset form.
        backdrop.querySelector('#f05-alert-at').value = '';
        backdrop.querySelector('#f05-alert-msg').value = '';
        addBtn.disabled = false;
        addBtn.textContent = 'Add alert';

        BB.showToast('Alert added ✓', 'success');
        refreshAlertBtn(orderId);
      } catch (err) {
        addBtn.disabled = false;
        addBtn.textContent = 'Add alert';
        BB.showToast(`Add failed: ${err.message}`, 'error');
      }
    }
  });
};

// ── Helpers ───────────────────────────────────────────────────────────────── //

function modeLabel(mode) {
  return {
    once: '⏰ Once',
    hourly: '🔁 Hourly',
    daily: '📅 Daily',
    every3d: '🗓 Every 3 days',
  }[mode] || mode;
}

function updateHint(backdrop, mode) {
  const hints = {
    once:    'One-time alert at this exact date &amp; time.',
    hourly:  'First alert at this time, then every hour until cleared.',
    daily:   'First alert at this time, then every 24 hours until cleared.',
    every3d: 'First alert at this time, then every 3 days until cleared.',
  };
  const labels = {
    once: 'Alert at', hourly: 'First alert at', daily: 'First alert at', every3d: 'First alert at',
  };
  backdrop.querySelector('#f05-at-hint').innerHTML = hints[mode] || '';
  backdrop.querySelector('#f05-at-label').textContent = labels[mode] || 'Alert at';
}

function alertItemHtml(a) {
  const recurring = a.alert_mode !== 'once';
  const timeLabel = recurring
    ? `Next: ${fmtDate(a.next_fire_at)}`
    : `At: ${fmtDate(a.alert_at)}`;
  const firedNote = a.fired_count > 0 ? ` · fired ${a.fired_count}×` : '';

  return `
    <div class="alert-item" data-alert-id="${a.id}">
      <div class="alert-item__info">
        <span class="alert-item__mode">${modeLabel(a.alert_mode)}</span>
        <span class="alert-item__time">${timeLabel}${firedNote}</span>
        ${a.message ? `<span class="alert-item__msg">"${BB.escapeHtml(a.message)}"</span>` : ''}
      </div>
      <button class="btn btn--ghost btn--sm"
              data-delete-alert="${a.id}"
              title="Remove alert">✕</button>
    </div>`;
}

/**
 * After creating/deleting an alert, update the alert bell button in the row
 * without requiring a full list refresh.
 */
async function refreshAlertBtn(orderId) {
  const btn = document.querySelector(`.alert-btn[data-order-id="${orderId}"]`);
  if (!btn) return;
  // Quick re-fetch just the count.
  try {
    const alerts = await fetch(
      `/api/v1/branches/${btn.dataset.branchId}/orders/${orderId}/alerts`
    ).then(r => r.json());
    const count = alerts.length;
    btn.dataset.active = count > 0 ? 'true' : 'false';
    const badge = btn.querySelector('.alert-count-badge');
    if (count > 0) {
      if (badge) badge.textContent = count;
      else btn.insertAdjacentHTML('beforeend',
        `<span class="alert-count-badge">${count}</span>`);
    } else {
      badge?.remove();
    }
  } catch { /* non-fatal */ }
}
