/**
 * Workers onboarding page — client-side logic.
 * Create worker row, delete worker row.
 * No SSE — worker list changes are infrequent Owner actions.
 * T16-04: refreshWorkerList() and appendWorkerRow() target both
 *         #workers-tbody (desktop table) and #workers-cards-list (mobile cards).
 */

function initWorkersPage(branchId) {
  const api = (path, opts) => BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  // ── Create Worker ─────────────────────────────────────────────── //

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
      appendWorkerRow(worker);
      BB.showToast(`Worker "${worker.name}" added — tell them to message your LINE bot.`, 'success');
    } catch (e) {
      BB.showToast(`Failed: ${e.message}`, 'error');
    }
  });

  // ── Delete Worker ─────────────────────────────────────────────── //

  // Exposed globally for inline onclick on SSR-rendered rows and cards
  window.workerDelete = async (workerId) => {
    const ok = await BB.confirm(
      'Remove this worker? Their LINE binding will also be removed. This cannot be undone.'
    );
    if (!ok) return;

    try {
      await api(`/workers/${workerId}`, { method: 'DELETE' });

      // Remove from table row
      const row = document.querySelector(`tr[data-worker-id="${workerId}"]`);
      if (row) row.remove();

      // Remove from mobile card (T16-04)
      const card = document.querySelector(`.worker-card[data-worker-id="${workerId}"]`);
      if (card) card.remove();

      maybeShowEmpty();
      BB.showToast('Worker removed', 'success');
    } catch (e) {
      BB.showToast(`Failed: ${e.message}`, 'error');
    }
  };

  // ── Helpers ───────────────────────────────────────────────────── //

  async function refreshWorkerList() {
    try {
      const workers = await api('/workers');

      const tbody     = document.getElementById('workers-tbody');
      const cardsList = document.getElementById('workers-cards-list');

      if (workers.length === 0) {
        if (tbody) {
          tbody.innerHTML = '<tr><td colspan="4" class="data-table__empty">No workers yet.</td></tr>';
        }
        if (cardsList) {
          cardsList.innerHTML = '<p class="workers-cards-empty">No workers yet.</p>';
        }
        return;
      }

      if (tbody) {
        tbody.innerHTML = workers.map(workerRowHtml).join('');
      }
      if (cardsList) {
        cardsList.innerHTML = workers.map(workerCardHtml).join('');
      }
    } catch (e) {
      BB.showToast(`Refresh failed: ${e.message}`, 'error');
    }
  }

  // Desktop table row HTML
  function workerRowHtml(w) {
    const channelLabel = { line: 'LINE', whats_app: 'WhatsApp', telegram: 'Telegram' };
    const bindingCell = w.bound
      ? `<span class="channel-badge channel-badge--${(w.channel || '').replace('_', '-')}">${channelLabel[w.channel] || w.channel}</span>`
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

  // T16-04: Mobile card HTML
  function workerCardHtml(w) {
    const channelLabel = { line: 'LINE', whats_app: 'WhatsApp', telegram: 'Telegram' };
    const bindingHtml = w.bound
      ? `<div class="worker-card__binding">
           <span class="channel-badge channel-badge--${(w.channel || '').replace('_', '-')}">${channelLabel[w.channel] || w.channel}</span>
           ${w.external_id ? `<span class="worker-card__sender">${BB.escapeHtml(w.external_id)}</span>` : ''}
         </div>`
      : `<span class="text-muted text-xs">Not bound</span>`;
    return `<div class="worker-card" data-worker-id="${w.id}">
  <div class="worker-card__header">
    <span class="worker-card__name">${BB.escapeHtml(w.name)}</span>
    <button class="btn btn--ghost btn--sm" onclick="workerDelete('${w.id}')">Remove</button>
  </div>
  ${bindingHtml}
</div>`;
  }

  function appendWorkerRow(worker) {
    const tbody     = document.getElementById('workers-tbody');
    const cardsList = document.getElementById('workers-cards-list');

    // Remove empty-state placeholders
    if (tbody) {
      const empty = tbody.querySelector('.data-table__empty');
      if (empty) empty.closest('tr').remove();
    }
    if (cardsList) {
      const empty = cardsList.querySelector('.workers-cards-empty');
      if (empty) empty.remove();
    }

    // Append table row
    if (tbody) {
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

    // Append mobile card (T16-04)
    if (cardsList) {
      const div = document.createElement('div');
      div.className = 'worker-card';
      div.dataset.workerId = worker.id;
      div.innerHTML = `
        <div class="worker-card__header">
          <span class="worker-card__name">${BB.escapeHtml(worker.name)}</span>
          <button class="btn btn--ghost btn--sm" onclick="workerDelete('${worker.id}')">Remove</button>
        </div>
        <span class="text-muted text-xs">Not bound</span>`;
      cardsList.appendChild(div);
    }
  }

  function maybeShowEmpty() {
    const tbody     = document.getElementById('workers-tbody');
    const cardsList = document.getElementById('workers-cards-list');

    if (tbody && tbody.querySelectorAll('tr[data-worker-id]').length === 0) {
      tbody.innerHTML =
        '<tr><td colspan="4" class="data-table__empty">No workers yet. Create one to get started.</td></tr>';
    }
    if (cardsList && cardsList.querySelectorAll('.worker-card').length === 0) {
      cardsList.innerHTML =
        '<p class="workers-cards-empty">No workers yet. Create one to get started.</p>';
    }
  }
}