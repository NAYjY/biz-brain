/**
 * Workers onboarding page — client-side logic.
 * Create worker row, delete worker row.
 * No SSE — worker list changes are infrequent Owner actions.
 */

function initWorkersPage(branchId) {
  const api = (path, opts) => BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  // ── Create Worker ─────────────────────────────────────────────────── //

  document.getElementById('create-worker-btn').addEventListener('click', async () => {
    const nameEl = document.getElementById('worker-name');
    const name = nameEl.value.trim();
    if (!name) { BB.showToast('Name required', 'error'); return; }

    try {
      const worker = await api('/workers', {
        method: 'POST',
        body: JSON.stringify({ name }),
      });
      BB.closeModal('create-worker-modal');
      nameEl.value = '';
      await refreshWorkerList();
      BB.showToast(`Worker "${worker.name}" added — tell them to message your LINE bot.`, 'success');
    } catch (e) {
      BB.showToast(`Failed: ${e.message}`, 'error');
    }
  });

  // ── Delete Worker ─────────────────────────────────────────────────── //

  // Exposed globally for inline onclick on SSR-rendered rows
  window.workerDelete = async (workerId) => {
    const ok = await BB.confirm(
      'Remove this worker? Their LINE binding will also be removed. This cannot be undone.'
    );
    if (!ok) return;

    try {
      await api(`/workers/${workerId}`, { method: 'DELETE' });
      const row = document.querySelector(`tr[data-worker-id="${workerId}"]`);
      if (row) {
        row.remove();
        maybeShowEmpty();
      }
      BB.showToast('Worker removed', 'success');
    } catch (e) {
      BB.showToast(`Failed: ${e.message}`, 'error');
    }
  };

  // ── Helpers ───────────────────────────────────────────────────────── //

  async function refreshWorkerList() {
      try {
          const workers = await api('/workers');
          const tbody = document.getElementById('workers-tbody');
          if (workers.length === 0) {
              tbody.innerHTML = '<tr><td colspan="4" class="data-table__empty">No workers yet.</td></tr>';
              return;
          }
          tbody.innerHTML = workers.map(workerRowHtml).join('');
      } catch (e) {
          BB.showToast(`Refresh failed: ${e.message}`, 'error');
      }
  }

  function workerRowHtml(w) {
      const channelLabel = { line: 'LINE', whats_app: 'WhatsApp', telegram: 'Telegram' };
      const bindingCell = w.bound
          ? `<span class="channel-badge channel-badge--${(w.channel || '').replace('_','-')}">${channelLabel[w.channel] || w.channel}</span>`
          : `<span class="text-muted text-xs">Not bound</span>`;
      const idCell = w.bound && w.external_id
          ? `<span class="font-mono text-xs">${BB.escapeHtml(w.external_id)}</span>`
          : `<span class="text-muted text-xs">—</span>`;
      return `<tr data-worker-id="${w.id}">
    <td>${BB.escapeHtml(w.name)}</td>
    <td>${bindingCell}</td>
    <td>${idCell}</td>
    <td><button class="btn btn--ghost btn--sm" onclick="workerDelete('${w.id}')">Remove</button></td>
  </tr>`;
  }
  function appendWorkerRow(worker) {
    const tbody = document.getElementById('workers-tbody');

    // Remove empty-state row if present
    const empty = tbody.querySelector('.data-table__empty');
    if (empty) empty.closest('tr').remove();

    const tr = document.createElement('tr');
    tr.dataset.workerId = worker.id;
    tr.innerHTML = `
      <td>${BB.escapeHtml(worker.name)}</td>
      <td><span class="text-muted text-xs">Not bound</span></td>
      <td><span class="text-muted text-xs">—</span></td>
      <td>
        <button class="btn btn--ghost btn--sm" onclick="workerDelete('${worker.id}')">Remove</button>
      </td>`;
    tbody.appendChild(tr);
  }

  function maybeShowEmpty() {
    const tbody = document.getElementById('workers-tbody');
    if (tbody && tbody.querySelectorAll('tr').length === 0) {
      tbody.innerHTML =
        '<tr><td colspan="4" class="data-table__empty">No workers yet. Create one to get started.</td></tr>';
    }
  }
}