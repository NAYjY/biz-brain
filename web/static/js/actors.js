/**
 * D08-5: Actors page — Confirm / Reject pending channel identity bindings.
 *
 * Confirm: Owner picks which Worker this sender maps to from a dropdown.
 * Reject: removes the pending row; next message re-creates it.
 *
 * T16-04: removeRow() removes from both the desktop table row (#actors-tbody)
 *         and the mobile card (.actor-card[data-binding-id]) so both views
 *         stay in sync after a confirm or reject action.
 */

function initActorsPage(branchId) {
  const api = (path, opts) =>
    BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  // ── Confirm ───────────────────────────────────────────────────── //

  window.actorConfirm = async (bindingId) => {
    // Load workers for this branch so Owner can pick which one
    let workers = [];
    try {
      workers = await api('/workers');
    } catch (e) {
      BB.showToast(`Could not load workers: ${e.message}`, 'error');
      return;
    }

    if (workers.length === 0) {
      BB.showToast('Create a worker first before confirming a binding.', 'error');
      return;
    }

    const options = workers
      .map(w => `<option value="${w.id}">${BB.escapeHtml(w.name)}</option>`)
      .join('');

    const backdrop = document.createElement('div');
    backdrop.className = 'modal-backdrop';
    backdrop.innerHTML = `
      <div class="confirm-dialog" role="dialog" aria-modal="true">
        <p style="font-weight:600;">Link sender to which worker?</p>
        <div class="form-group" style="margin-top:.5rem;">
          <label class="form-label">Worker</label>
          <select class="form-select" id="confirm-worker-select">
            ${options}
          </select>
        </div>
        <div class="confirm-dialog__actions" style="margin-top:1rem;">
          <button class="btn btn--ghost"   data-action="cancel">Cancel</button>
          <button class="btn btn--primary" data-action="confirm">Confirm</button>
        </div>
      </div>`;

    document.body.appendChild(backdrop);

    backdrop.addEventListener('click', async (e) => {
      const action = e.target.closest('[data-action]')?.dataset.action;
      if (!action) return;
      if (action !== 'confirm') {
        backdrop.remove();
        return;
      }

      // Read value BEFORE removing the backdrop from the DOM
      const workerId = document.getElementById('confirm-worker-select')?.value;
      backdrop.remove();

      if (!workerId) {
        BB.showToast('No worker selected', 'error');
        return;
      }

      try {
        await api(`/actors/${bindingId}/confirm`, {
          method: 'POST',
          body: JSON.stringify({ worker_id: workerId }),
        });
        removeRow(bindingId);
        BB.showToast('Binding confirmed — sender is now trusted.', 'success');
      } catch (e) {
        BB.showToast(`Confirm failed: ${e.message}`, 'error');
      }
    });
  };

  // ── Reject ────────────────────────────────────────────────────── //

  window.actorReject = async (bindingId) => {
    const ok = await BB.confirm(
      'Reject this binding? The row will be removed. Their next message will create a new pending entry.'
    );
    if (!ok) return;

    try {
      await api(`/actors/${bindingId}/reject`, { method: 'POST' });
      removeRow(bindingId);
      BB.showToast('Binding rejected.', 'success');
    } catch (e) {
      BB.showToast(`Reject failed: ${e.message}`, 'error');
    }
  };

  // ── Helpers ───────────────────────────────────────────────────── //

  function removeRow(bindingId) {
    // Remove desktop table row
    const row = document.querySelector(`tr[data-binding-id="${bindingId}"]`);
    if (row) row.remove();

    // Remove mobile card (T16-04)
    const card = document.querySelector(`.actor-card[data-binding-id="${bindingId}"]`);
    if (card) card.remove();

    // Show empty state in table if no rows remain
    const tbody = document.getElementById('actors-tbody');
    if (tbody && tbody.querySelectorAll('tr[data-binding-id]').length === 0) {
      tbody.innerHTML =
        '<tr><td colspan="5" class="data-table__empty">No pending bindings.</td></tr>';
    }

    // Show empty state in card list if no cards remain (T16-04)
    const cardsList = document.getElementById('actors-cards-list');
    if (cardsList && cardsList.querySelectorAll('.actor-card').length === 0) {
      cardsList.innerHTML =
        '<p class="actors-cards-empty">No pending bindings.</p>';
    }
  }
}