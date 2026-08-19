/**
 * D04 / P04 / P16: Orders page — full Owner control.
 *
 * P16 additions:
 *   - Force-state buttons: Accepted, Unavailable, Clarification, Ready
 *   - Reassign worker (swap without cancel+reset)
 *   - Edit description inline
 *   - Delete order (soft, blocked if in-flight)
 */

function initOrdersPage(branchId) {
  const api = (path, opts) => BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  let pendingAssignOrderId = null;
  let pendingReassignOrderId = null;
  let customerMap = {};

  // ── On load ──────────────────────────────────────────────────────── //

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

  function orderRowHtml(o) {
    const pill = BB.statePill(o.state);
    const customerName = BB.escapeHtml(customerMap[o.customer_id] || BB.shortId(o.customer_id));
    const worker = o.worker_id
      ? `<span class="font-mono text-xs">${BB.shortId(o.worker_id)}</span>`
      : '—';

    const messageRow = o.last_worker_message ? `
      <tr class="worker-message-row">
        <td colspan="5">
          <div class="worker-message-bubble">
            <span class="worker-message-label">Worker said:</span>
            <span class="worker-message-text">${BB.escapeHtml(o.last_worker_message)}</span>
            <div class="worker-reply-box">
              <input class="form-input worker-reply-input" type="text"
                     placeholder="${o.state === 'PENDING_CLARIFICATION' ? 'Reply to resolve clarification…' : 'Reply to worker…'}"
                     id="reply-${o.id}">
              <button class="btn btn--primary btn--sm"
                      onclick="sendWorkerReply('${o.id}', '${o.state}')">Send</button>
            </div>
          </div>
        </td>
      </tr>` : '';

    return `
      <tr data-order-id="${o.id}" data-state="${o.state}">
        <td>${pill}</td>
        <td>
          <span class="order-desc" id="desc-${o.id}">${BB.escapeHtml(o.description)}</span>
        </td>
        <td class="text-muted text-sm">${customerName}</td>
        <td class="text-muted text-xs" id="worker-${o.id}">${worker}</td>
        <td><div class="order-actions" data-order-id="${o.id}" style="display:flex;gap:.5rem;flex-wrap:wrap;" ></div></td>
      </tr>${messageRow}`;
  }

  function attachRowActions() {
    document.querySelectorAll('.order-actions').forEach(renderActions);
  }

  function renderActions(container) {
    const orderId = container.dataset.orderId;
    const row = container.closest('tr');
    const statePill = row?.querySelector('.state-pill');
    const state = statePill?.textContent?.trim().toUpperCase().replace(/ /g, '_') ?? '';

    container.innerHTML = '';

    const done      = state === 'DONE';
    const cancelled = state === 'CANCELLED';
    const unassigned= state === 'UNASSIGNED';
    const unavail   = state === 'UNAVAILABLE';
    const assigned  = state === 'ASSIGNED';
    const accepted  = state === 'ACCEPTED';
    const clarif    = state === 'PENDING_CLARIFICATION';
    const ready     = state === 'READY_FOR_PICKUP';
    const active    = assigned || accepted || clarif || unavail || ready;
    const terminal  = done;

    // ── Standard flow ────────────────────────────────────────────── //
    if (unassigned || unavail || cancelled) {
      container.appendChild(actionBtn('Assign', 'btn--ghost btn--sm', () => openAssignWorker(orderId)));
    }
    if (active) {
      container.appendChild(actionBtn('Reassign', 'btn--ghost btn--sm', () => openReassignWorker(orderId)));
    }
    if (!done && !cancelled) {
      container.appendChild(actionBtn('Cancel', 'btn--danger btn--sm', () => cancelOrder(orderId)));
    }
    if (cancelled || unavail) {
      container.appendChild(actionBtn('Reset', 'btn--ghost btn--sm', () => resetOrder(orderId)));
    }
    if (active || ready) {
      container.appendChild(actionBtn('Close', 'btn--ghost btn--sm', () => closeOrder(orderId)));
    }

    // ── P16: Force-state group ───────────────────────────────────── //
    if (!terminal) {
      const forceMenu = buildForceMenu(orderId, state);
      container.appendChild(forceMenu);
    }

    // ── P16: Edit description ────────────────────────────────────── //
    if (!terminal) {
      container.appendChild(actionBtn('Edit', 'btn--ghost btn--sm', () => editDescription(orderId)));
    }

    // ── P16: Delete ──────────────────────────────────────────────── //
    if (done || cancelled || unassigned || unavail) {
      container.appendChild(actionBtn('Delete', 'btn--danger btn--sm', () => deleteOrder(orderId)));
    }
  }

  function buildForceMenu(orderId, state) {
    const wrapper = document.createElement('div');
    wrapper.style.cssText = 'position:relative;display:inline-block;';

    const trigger = actionBtn('Force ▾', 'btn--ghost btn--sm');
    wrapper.appendChild(trigger);

    const menu = document.createElement('div');
    menu.style.cssText = [
      'display:none;position:absolute;top:100%;left:0;z-index:50;',
      'background:var(--color-surface);border:1px solid var(--color-border);',
      'border-radius:var(--radius-md);min-width:180px;box-shadow:0 4px 16px rgba(0,0,0,.3);',
    ].join('');

    const items = [
      { label: 'Force → Accepted',     fn: () => forceState(orderId, 'force-accepted') },
      { label: 'Force → Unavailable',  fn: () => forceState(orderId, 'force-unavailable') },
      { label: 'Force → Clarification',fn: () => forceState(orderId, 'force-clarification') },
      { label: 'Force → Ready',        fn: () => forceState(orderId, 'force-ready') },
    ];

    items.forEach(({ label, fn }) => {
      const item = document.createElement('button');
      item.textContent = label;
      item.style.cssText = [
        'display:block;width:100%;padding:.5rem .75rem;text-align:left;',
        'background:none;border:none;cursor:pointer;font-size:var(--text-sm);',
        'color:var(--color-text);transition:background .1s;',
      ].join('');
      item.onmouseenter = () => { item.style.background = 'var(--color-surface-2)'; };
      item.onmouseleave = () => { item.style.background = 'none'; };
      item.onclick = () => { menu.style.display = 'none'; fn(); };
      menu.appendChild(item);
    });

    wrapper.appendChild(menu);

    trigger.onclick = (e) => {
      e.stopPropagation();
      const open = menu.style.display === 'block';
      document.querySelectorAll('.force-menu-open').forEach(m => { m.style.display = 'none'; m.classList.remove('force-menu-open'); });
      menu.style.display = open ? 'none' : 'block';
      if (!open) menu.classList.add('force-menu-open');
    };

    document.addEventListener('click', () => { menu.style.display = 'none'; }, { once: true, capture: true });

    return wrapper;
  }

  function actionBtn(label, classes, onClick) {
    const b = document.createElement('button');
    b.className = `btn ${classes}`;
    b.textContent = label;
    if (onClick) b.onclick = onClick;
    return b;
  }

  // ── Worker reply / P04 resolve clarification ─────────────────────── //

  window.sendWorkerReply = async (orderId, orderState) => {
    const input = document.getElementById(`reply-${orderId}`);
    const text = input?.value.trim();
    if (!text) { BB.showToast('Type a message first', 'error'); return; }

    const isClarification = orderState === 'PENDING_CLARIFICATION';
    const endpoint = isClarification
      ? `/orders/${orderId}/resolve-clarification`
      : `/orders/${orderId}/message-worker`;

    try {
      await api(endpoint, {
        method: 'POST',
        body: JSON.stringify(isClarification ? { message: text } : { text }),
      });
      input.value = '';
      BB.showToast(isClarification ? 'Clarification resolved ✓' : 'Message sent ✓', 'success');
    } catch (e) {
      BB.showToast(`Send failed: ${e.message}`, 'error');
    }
  };
  BB.sendWorkerReply = window.sendWorkerReply;

  // ── Create Order ─────────────────────────────────────────────────── //

  document.getElementById('new-customer-btn').addEventListener('click', () => {
    document.getElementById('new-customer-row').style.display = '';
  });

  document.getElementById('create-order-btn').addEventListener('click', async () => {
    const descEl     = document.getElementById('order-description');
    const customerEl = document.getElementById('order-customer');
    const newNameEl  = document.getElementById('new-customer-name');

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
    document.getElementById('assign-worker-modal-title').textContent = 'Assign Worker';
    BB.openModal('assign-worker-modal');
  }

  document.getElementById('assign-worker-btn').addEventListener('click', async () => {
    const workerId = document.getElementById('assign-worker-select').value;
    if (!workerId) { BB.showToast('Select a worker', 'error'); return; }

    const ok = await BB.confirm('Assign this worker? A message will be sent to them immediately.');
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

  // ── P16: Reassign Worker ─────────────────────────────────────────── //

  function openReassignWorker(orderId) {
    pendingReassignOrderId = orderId;
    document.getElementById('assign-worker-modal-title').textContent = 'Reassign Worker';
    BB.openModal('assign-worker-modal');
  }

  // Override assign button to handle reassign too
  const originalAssignClick = document.getElementById('assign-worker-btn').onclick;
  document.getElementById('assign-worker-btn').onclick = async () => {
    const workerId = document.getElementById('assign-worker-select').value;
    if (!workerId) { BB.showToast('Select a worker', 'error'); return; }

    if (pendingReassignOrderId && !pendingAssignOrderId) {
      const ok = await BB.confirm('Reassign this order to a different worker? They will be notified.');
      if (!ok) return;
      try {
        await api(`/orders/${pendingReassignOrderId}/reassign-worker`, {
          method: 'POST',
          body: JSON.stringify({ worker_id: workerId }),
        });
        BB.closeModal('assign-worker-modal');
        pendingReassignOrderId = null;
        BB.showToast('Worker reassigned', 'success');
      } catch (e) {
        BB.showToast(`Reassign failed: ${e.message}`, 'error');
      }
      return;
    }

    // Normal assign path
    const ok = await BB.confirm('Assign this worker? A message will be sent to them immediately.');
    if (!ok) return;
    try {
      await api(`/orders/${pendingAssignOrderId}/assign-worker`, {
        method: 'POST',
        body: JSON.stringify({ worker_id: workerId }),
      });
      BB.closeModal('assign-worker-modal');
      pendingAssignOrderId = null;
      BB.showToast('Worker assigned', 'success');
    } catch (e) {
      BB.showToast(`Assign failed: ${e.message}`, 'error');
    }
  };

  // ── P16: Force state ─────────────────────────────────────────────── //

  async function forceState(orderId, endpoint) {
    const label = endpoint.replace('force-', '').replace(/-/g, ' ');
    const ok = await BB.confirm(`Force order to "${label}"? This bypasses normal Worker messaging.`);
    if (!ok) return;
    try {
      await api(`/orders/${orderId}/${endpoint}`, { method: 'POST', body: JSON.stringify({}) });
      BB.showToast(`Forced → ${label}`, 'success');
    } catch (e) {
      BB.showToast(`Force failed: ${e.message}`, 'error');
    }
  }

  // ── P16: Edit description ────────────────────────────────────────── //

  async function editDescription(orderId) {
    const descEl = document.getElementById(`desc-${orderId}`);
    const current = descEl?.textContent?.trim() ?? '';

    const backdrop = document.createElement('div');
    backdrop.className = 'modal-backdrop';
    backdrop.innerHTML = `
      <div class="modal" style="width:480px;">
        <div class="modal__header">
          <span class="modal__title">Edit description</span>
          <button class="btn btn--ghost btn--sm" data-action="cancel">✕</button>
        </div>
        <div class="modal__body">
          <div class="form-group">
            <label class="form-label">Description</label>
            <textarea class="form-textarea" id="edit-desc-input" style="min-height:100px;">${BB.escapeHtml(current)}</textarea>
          </div>
        </div>
        <div class="modal__footer">
          <button class="btn btn--ghost" data-action="cancel">Cancel</button>
          <button class="btn btn--primary" data-action="save">Save</button>
        </div>
      </div>`;

    document.body.appendChild(backdrop);
    backdrop.querySelector('#edit-desc-input').focus();

    backdrop.addEventListener('click', async (e) => {
      const action = e.target.closest('[data-action]')?.dataset.action;
      if (!action) return;
      if (action === 'cancel') { backdrop.remove(); return; }

      const newDesc = backdrop.querySelector('#edit-desc-input').value.trim();
      if (!newDesc) { BB.showToast('Description cannot be empty', 'error'); return; }

      try {
        await api(`/orders/${orderId}/description`, {
          method: 'PATCH',
          body: JSON.stringify({ description: newDesc }),
        });
        backdrop.remove();
        if (descEl) descEl.textContent = newDesc;
        BB.showToast('Description updated', 'success');
      } catch (err) {
        BB.showToast(`Save failed: ${err.message}`, 'error');
      }
    });
  }

  // ── P04: Cancel / Reset / Close ──────────────────────────────────── //

  async function cancelOrder(orderId) {
    const ok = await BB.confirm('Cancel this order? The Worker will be notified.');
    if (!ok) return;
    try {
      await api(`/orders/${orderId}/cancel`, { method: 'POST' });
      BB.showToast('Order cancelled', 'success');
    } catch (e) {
      BB.showToast(`Cancel failed: ${e.message}`, 'error');
    }
  }

  async function resetOrder(orderId) {
    const ok = await BB.confirm('Reset this order back to Unassigned?');
    if (!ok) return;
    try {
      await api(`/orders/${orderId}/reset`, { method: 'POST' });
      BB.showToast('Order reset to Unassigned', 'success');
    } catch (e) {
      BB.showToast(`Reset failed: ${e.message}`, 'error');
    }
  }

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

  // ── P16: Delete ──────────────────────────────────────────────────── //

  async function deleteOrder(orderId) {
    const ok = await BB.confirm('Permanently delete this order? This cannot be undone.');
    if (!ok) return;
    try {
      await api(`/orders/${orderId}`, { method: 'DELETE' });
      const row = document.querySelector(`tr[data-order-id="${orderId}"]`);
      const msgRow = row?.nextElementSibling;
      if (msgRow?.classList.contains('worker-message-row')) msgRow.remove();
      row?.remove();
      maybeShowEmpty();
      BB.showToast('Order deleted', 'success');
    } catch (e) {
      BB.showToast(`Delete failed: ${e.message}`, 'error');
    }
  }

  function maybeShowEmpty() {
    const tbody = document.getElementById('orders-tbody');
    if (tbody && tbody.querySelectorAll('tr[data-order-id]').length === 0) {
      tbody.innerHTML = '<tr><td colspan="5" class="data-table__empty">No orders yet.</td></tr>';
    }
  }

  // ── Data loaders ─────────────────────────────────────────────────── //

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
