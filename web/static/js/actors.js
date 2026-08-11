/**
 * D08-5: Actors page — Confirm / Reject pending channel identity bindings.
 *
 * No SSE wiring needed — this page has no live-update signal (no SseSignal
 * variant for actor_directory changes). Owner manually refreshes or
 * actions disappear from the table after confirm/reject.
 */

function initActorsPage(branchId) {
  const api = (path, opts) =>
    BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  // Expose to inline onclick handlers on SSR-rendered rows.
  window.actorConfirm = async (bindingId) => {
    const ok = await BB.confirm(
      'Confirm this binding? Messages from this sender will be trusted from now on.'
    );
    if (!ok) return;

    try {
      await api(`/actors/${bindingId}/confirm`, { method: 'POST' });
      removeRow(bindingId);
      BB.showToast('Binding confirmed — sender is now trusted.', 'success');
    } catch (e) {
      BB.showToast(`Confirm failed: ${e.message}`, 'error');
    }
  };

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

  function removeRow(bindingId) {
    const row = document.querySelector(`tr[data-binding-id="${bindingId}"]`);
    if (!row) return;

    const tbody = document.getElementById('actors-tbody');
    row.remove();

    // If table is now empty, show the empty state row.
    if (tbody && tbody.querySelectorAll('tr').length === 0) {
      tbody.innerHTML =
        '<tr><td colspan="5" class="data-table__empty">No pending bindings.</td></tr>';
    }
  }
}