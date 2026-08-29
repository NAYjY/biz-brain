/**
 * T10: Suppliers page — client-side logic.
 * Create supplier row, delete supplier row.
 * No SSE — supplier list changes are infrequent Owner actions.
 */

function initSuppliersPage(branchId) {
  const api = (path, opts) => BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  // ── Create Supplier ───────────────────────────────────────────────── //

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

  // ── Delete Supplier ───────────────────────────────────────────────── //

  window.supplierDelete = async (supplierId) => {
    const ok = await BB.confirm(
      'Remove this supplier? Their WhatsApp binding will also be removed. This cannot be undone.'
    );
    if (!ok) return;

    try {
      await api(`/suppliers/${supplierId}`, { method: 'DELETE' });
      const row = document.querySelector(`tr[data-supplier-id="${supplierId}"]`);
      if (row) {
        row.remove();
        maybeShowEmpty();
      }
      BB.showToast('Supplier removed', 'success');
    } catch (e) {
      BB.showToast(`Failed: ${e.message}`, 'error');
    }
  };

  // ── Helpers ───────────────────────────────────────────────────────── //

  function appendSupplierRow(supplier) {
    const tbody = document.getElementById('suppliers-tbody');

    // Remove empty-state row if present
    const empty = tbody.querySelector('.data-table__empty');
    if (empty) empty.closest('tr').remove();

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

  function maybeShowEmpty() {
    const tbody = document.getElementById('suppliers-tbody');
    if (tbody && tbody.querySelectorAll('tr').length === 0) {
      tbody.innerHTML =
        '<tr><td colspan="4" class="data-table__empty">No suppliers yet. Create one to get started.</td></tr>';
    }
  }
}