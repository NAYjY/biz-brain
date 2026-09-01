/**
 * T10: Suppliers page — client-side logic.
 * Create supplier row, delete supplier row.
 * No SSE — supplier list changes are infrequent Owner actions.
 * T16-04: appendSupplierRow() and maybeShowEmpty() target both
 *         #suppliers-tbody (desktop table) and #suppliers-cards-list (mobile cards).
 */

function initSuppliersPage(branchId) {
  const api = (path, opts) => BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  // ── Create Supplier ───────────────────────────────────────────── //

  document.getElementById('create-supplier-btn').addEventListener('click', async () => {
    const nameEl = document.getElementById('supplier-name');
    const name = nameEl.value.trim();
    if (!name) { BB.showToast('Name required', 'error'); return; }

    try {
      const supplier = await api('/suppliers', {
        method: 'POST',
        body: JSON.stringify({ name }),
      });
      BB.closeModal('create-supplier-modal');
      nameEl.value = '';
      appendSupplierRow(supplier);
      BB.showToast(`Supplier "${supplier.name}" added — tell them to message your WhatsApp bot.`, 'success');
    } catch (e) {
      BB.showToast(`Failed: ${e.message}`, 'error');
    }
  });

  // ── Delete Supplier ───────────────────────────────────────────── //

  window.supplierDelete = async (supplierId) => {
    const ok = await BB.confirm(
      'Remove this supplier? Their WhatsApp binding will also be removed. This cannot be undone.'
    );
    if (!ok) return;

    try {
      await api(`/suppliers/${supplierId}`, { method: 'DELETE' });

      // Remove from table row
      const row = document.querySelector(`tr[data-supplier-id="${supplierId}"]`);
      if (row) row.remove();

      // Remove from mobile card (T16-04)
      const card = document.querySelector(`.supplier-card[data-supplier-id="${supplierId}"]`);
      if (card) card.remove();

      maybeShowEmpty();
      BB.showToast('Supplier removed', 'success');
    } catch (e) {
      BB.showToast(`Failed: ${e.message}`, 'error');
    }
  };

  // ── Helpers ───────────────────────────────────────────────────── //

  function appendSupplierRow(supplier) {
    const tbody     = document.getElementById('suppliers-tbody');
    const cardsList = document.getElementById('suppliers-cards-list');

    // Remove empty-state placeholders
    if (tbody) {
      const empty = tbody.querySelector('.data-table__empty');
      if (empty) empty.closest('tr').remove();
    }
    if (cardsList) {
      const empty = cardsList.querySelector('.suppliers-cards-empty');
      if (empty) empty.remove();
    }

    // Append table row
    if (tbody) {
      const tr = document.createElement('tr');
      tr.dataset.supplierId = supplier.id;
      tr.innerHTML = `
        <td>${BB.escapeHtml(supplier.name)}</td>
        <td><span class="text-muted text-xs">Not bound</span></td>
        <td><span class="text-muted text-xs">—</span></td>
        <td>
          <button class="btn btn--ghost btn--sm" onclick="supplierDelete('${supplier.id}')">Remove</button>
        </td>`;
      tbody.appendChild(tr);
    }

    // Append mobile card (T16-04)
    if (cardsList) {
      const div = document.createElement('div');
      div.className = 'supplier-card';
      div.dataset.supplierId = supplier.id;
      div.innerHTML = `
        <div class="supplier-card__header">
          <span class="supplier-card__name">${BB.escapeHtml(supplier.name)}</span>
          <button class="btn btn--ghost btn--sm" onclick="supplierDelete('${supplier.id}')">Remove</button>
        </div>
        <span class="text-muted text-xs">Not bound</span>`;
      cardsList.appendChild(div);
    }
  }

  function maybeShowEmpty() {
    const tbody     = document.getElementById('suppliers-tbody');
    const cardsList = document.getElementById('suppliers-cards-list');

    if (tbody && tbody.querySelectorAll('tr[data-supplier-id]').length === 0) {
      tbody.innerHTML =
        '<tr><td colspan="4" class="data-table__empty">No suppliers yet. Create one to get started.</td></tr>';
    }
    if (cardsList && cardsList.querySelectorAll('.supplier-card').length === 0) {
      cardsList.innerHTML =
        '<p class="suppliers-cards-empty">No suppliers yet. Create one to get started.</p>';
    }
  }
}