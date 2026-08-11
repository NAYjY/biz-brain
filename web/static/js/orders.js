/**
 * D04: Orders page — client-side logic.
 * D08-2: load customer id->name map; use name in row render.
 * D08-3: call attachRowActions on SSR rows at DOMContentLoaded, not only after re-fetch.
 * Confirm-before-submit for Assign-Worker and Close (D04 resolution).
 */

function initOrdersPage(branchId) {
  const api = (path, opts) => BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  // ── State ────────────────────────────────────────────────────────── //

  let pendingAssignOrderId = null;
  // D08-2: id -> name map populated by loadCustomers()
  let customerMap = {};

  // ── On load ──────────────────────────────────────────────────────── //

  // D08-3: attach actions to SSR-rendered rows immediately
  attachRowActions();

  loadCustomers();
  loadWorkers();

  // ── SSE wiring (D06) ─────────────────────────────────────────────── //

  new BranchEventSource(branchId)
    .withBadge(document.getElementById('live-badge'))
    .on('OrderChanged', () => refreshOrderList())
    .connect();

  // ── Order list refresh ───────────────────────────────────────────── //

  async function refreshOrderList() {
    let orders;
    try { orders = await api('/orders'); }
    catch (e) { BB.showToast(`Refresh failed: ${e.message}`, 'error'); return; }

    const tbody = document.getElementById('orders-tbody');
    if (orders.length === 0) {
      tbody.innerHTML = '<tr><td colspan="5" class="data-table__empty">No orders yet.</td></tr>';
      return;
    }

    tbody.innerHTML = orders.map(orderRowHtml).join('');
    attachRowActions();
  }

  // D08-2: use customerMap for display name
  function orderRowHtml(o) {
    const pill = BB.statePill(o.state);
    const customerName = BB.escapeHtml(customerMap[o.customer_id] || BB.shortId(o.customer_id));
    const worker = o.worker_id
      ? `<span class="font-mono text-xs">${BB.shortId(o.worker_id)}</span>`
      : '—';
    return `
      <tr data-order-id="${o.id}">
        <td>${pill}</td>
        <td>${BB.escapeHtml(o.description)}</td>
        <td class="text-muted text-sm">${customerName}</td>
        <td class="text-muted text-xs">${worker}</td>
        <td><div class="order-actions" data-order-id="${o.id}" style="display:flex;gap:.5rem;"></div></td>
      </tr>`;
  }

  // D08-3: shared fn called both at init and after re-fetch
  function attachRowActions() {
    document.querySelectorAll('.order-actions').forEach(renderActions);
  }

  function renderActions(container) {
    const orderId = container.dataset.orderId;
    const row = container.closest('tr');
    const statePill = row?.querySelector('.state-pill');
    const state = statePill?.textContent?.trim().toUpperCase().replace(/ /g, '_') ?? '';

    container.innerHTML = '';

    if (['UNASSIGNED', 'UNAVAILABLE', 'CANCELLED'].includes(state)) {
      const btn = actionBtn('Assign', 'btn--ghost btn--sm');
      btn.onclick = () => openAssignWorker(orderId);
      container.appendChild(btn);
    }

    if (!['DONE', 'CANCELLED'].includes(state) && state !== '') {
      const btn = actionBtn('Close', 'btn--danger btn--sm');
      btn.onclick = () => closeOrder(orderId);
      container.appendChild(btn);
    }
  }

  function actionBtn(label, classes) {
    const b = document.createElement('button');
    b.className = `btn ${classes}`;
    b.textContent = label;
    return b;
  }

  // ── Create Order ─────────────────────────────────────────────────── //

  document.getElementById('new-customer-btn').addEventListener('click', () => {
    document.getElementById('new-customer-row').style.display = '';
  });

  document.getElementById('create-order-btn').addEventListener('click', async () => {
    const descEl = document.getElementById('order-description');
    const customerEl = document.getElementById('order-customer');
    const newNameEl = document.getElementById('new-customer-name');

    const description = descEl.value.trim();
    if (!description) { BB.showToast('Description required', 'error'); return; }

    let customerId = customerEl.value;

    const newName = newNameEl.value.trim();
    if (newName) {
      try {
        const c = await api('/customers', { method: 'POST', body: JSON.stringify({ name: newName }) });
        customerId = c.id;
        await loadCustomers();
      } catch (e) {
        BB.showToast(`Create customer failed: ${e.message}`, 'error');
        return;
      }
    }

    if (!customerId) { BB.showToast('Select or create a customer', 'error'); return; }

    try {
      await api('/orders', { method: 'POST', body: JSON.stringify({ customer_id: customerId, description }) });
      BB.closeModal('create-order-modal');
      descEl.value = '';
      newNameEl.value = '';
      document.getElementById('new-customer-row').style.display = 'none';
      await refreshOrderList();
    } catch (e) {
      BB.showToast(`Create order failed: ${e.message}`, 'error');
    }
  });

  // ── Assign Worker ────────────────────────────────────────────────── //

  function openAssignWorker(orderId) {
    pendingAssignOrderId = orderId;
    BB.openModal('assign-worker-modal');
  }

  document.getElementById('assign-worker-btn').addEventListener('click', async () => {
    const workerId = document.getElementById('assign-worker-select').value;
    if (!workerId) { BB.showToast('Select a worker', 'error'); return; }

    const ok = await BB.confirm(
      'Assign this worker? A LINE message will be sent to them immediately.'
    );
    if (!ok) return;

    try {
      await api(`/orders/${pendingAssignOrderId}/assign-worker`, {
        method: 'POST',
        body: JSON.stringify({ worker_id: workerId }),
      });
      BB.closeModal('assign-worker-modal');
      BB.showToast('Worker assigned', 'success');
    } catch (e) {
      BB.showToast(`Assign failed: ${e.message}`, 'error');
    }
  });

  // ── Close Order ──────────────────────────────────────────────────── //

  async function closeOrder(orderId) {
    const ok = await BB.confirm('Close this order? This marks it as Done.');
    if (!ok) return;

    try {
      await api(`/orders/${orderId}/close`, { method: 'POST' });
      BB.showToast('Order closed', 'success');
    } catch (e) {
      BB.showToast(`Close failed: ${e.message}`, 'error');
    }
  }

  // ── Data loaders ─────────────────────────────────────────────────── //

  // D08-2: build id->name map on load
  async function loadCustomers() {
    try {
      const customers = await api('/customers');
      customerMap = {};
      customers.forEach(c => { customerMap[c.id] = c.name; });

      const sel = document.getElementById('order-customer');
      sel.innerHTML = '<option value="">Select customer…</option>' +
        customers.map(c => `<option value="${c.id}">${BB.escapeHtml(c.name)}</option>`).join('');
    } catch { /* non-fatal */ }
  }

  async function loadWorkers() {
    try {
      const workers = await api('/workers');
      const sel = document.getElementById('assign-worker-select');
      sel.innerHTML = '<option value="">Select worker…</option>' +
        workers.map(w => `<option value="${w.id}">${BB.escapeHtml(w.name)}</option>`).join('');
    } catch { /* non-fatal */ }
  }
}