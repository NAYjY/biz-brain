/**
 * D04 / P04 / P16 / F04 / F01: Orders page — full Owner control.
 *
 * F01 changes:
 *  - "Job name" input in Create Order modal (optional, ≤20 chars, with counter)
 *  - order row shows short_name tag when set
 *  - Gear menu has "✏️ Set job name" item (set/edit/clear)
 *  - 409 Conflict on duplicate short_name gives a readable error
 */

function initOrdersPage(branchId) {
  const api = (path, opts) => BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  let pendingAssignOrderId   = null;
  let pendingReassignOrderId = null;
  let customerMap = {};

  // ── On load ──────────────────────────────────────────────────────── //

  attachRowActions();
  loadCustomers();
  loadWorkers();

  // F01: character counter for short-name input in create modal.
  const shortNameInput = document.getElementById('order-short-name');
  const shortNameCounter = document.getElementById('short-name-counter');
  if (shortNameInput && shortNameCounter) {
    shortNameInput.addEventListener('input', () => {
      shortNameCounter.textContent = `${shortNameInput.value.length}/20`;
    });
  }

  document.addEventListener('click', closeAllMenus);

  // ── SSE wiring ───────────────────────────────────────────────────── //

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
    const pill     = BB.statePill(o.state);
    const customer = BB.escapeHtml(customerMap[o.customer_id] || BB.shortId(o.customer_id));
    const worker   = o.worker_name ? BB.escapeHtml(o.worker_name) : '—';

    // F01: short_name tag.
    const namePrefix = o.short_name
      ? `<span class="order-tag" title="Job name">${BB.escapeHtml(o.short_name)}</span> `
      : '';

    // F04: thread button.
    let threadBtn = '';
    if (o.worker_id) {
      const unread = o.unread_message_count || 0;
      const badge  = unread > 0
        ? ` <span class="thread-unread-badge">${unread}</span>`
        : '';
      threadBtn = `<button class="thread-btn"
          data-order-id="${o.id}" data-unread="${unread}"
          title="View conversation thread">💬${badge}</button>`;
    }

    // F04: AI low-confidence badge.
    const aiBadge = o.ai_routed_low_confidence
      ? `<span class="ai-badge" title="AI-routed with low confidence — review recommended">🤖?</span>`
      : '';

    return `
      <tr data-order-id="${o.id}" data-state="${o.state}" data-short-name="${BB.escapeHtml(o.short_name || '')}">
        <td>${pill}</td>
        <td>${namePrefix}<span class="order-desc" id="desc-${o.id}">${BB.escapeHtml(o.description)}</span></td>
        <td class="text-muted text-sm">${customer}</td>
        <td class="text-muted text-xs" id="worker-${o.id}">${worker}</td>
        <td>
          <div style="display:flex;gap:.5rem;align-items:center;flex-wrap:wrap;">
            ${threadBtn}${aiBadge}
            <div class="order-gear-wrap" data-order-id="${o.id}"
                 style="position:relative;display:inline-block;"></div>
          </div>
        </td>
      </tr>`;
  }

  function attachRowActions() {
    document.querySelectorAll('.order-gear-wrap').forEach(renderGearButton);
    // F04: wire thread buttons.
    document.querySelectorAll('.thread-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        const orderId = btn.dataset.orderId;
        const row = document.querySelector(`tr[data-order-id="${orderId}"]`);
        const desc = row?.querySelector('.order-desc')?.textContent ?? orderId;
        openThreadModal(orderId, desc);
      });
    });
  }

  // ── F04: Thread modal ────────────────────────────────────────────── //

  async function openThreadModal(orderId, orderDesc) {
    let messages = [];
    try {
      messages = await api(`/orders/${orderId}/thread`);
    } catch (e) {
      BB.showToast(`Could not load thread: ${e.message}`, 'error');
      return;
    }

    const threadBtn = document.querySelector(`.thread-btn[data-order-id="${orderId}"]`);
    if (threadBtn) {
      threadBtn.dataset.unread = '0';
      threadBtn.innerHTML = '💬';
    }
    const row = document.querySelector(`tr[data-order-id="${orderId}"]`);
    row?.querySelector('.ai-badge')?.remove();

    const backdrop = document.createElement('div');
    backdrop.className = 'modal-backdrop';
    backdrop.innerHTML = `
      <div class="modal" style="width:520px;max-height:85vh;display:flex;flex-direction:column;">
        <div class="modal__header">
          <span class="modal__title">💬 ${BB.escapeHtml(orderDesc)}</span>
          <button class="btn btn--ghost btn--sm" data-action="close">✕</button>
        </div>
        <div class="thread-body" id="thread-msg-body" style="flex:1;padding:var(--space-4);">
          ${messages.length === 0
            ? '<p class="text-muted text-sm">No messages yet.</p>'
            : messages.map(threadBubble).join('')}
        </div>
        <div class="modal__footer" style="flex-direction:column;gap:var(--space-2);align-items:stretch;
                                          border-top:1px solid var(--color-border);padding-top:var(--space-4);">
          <textarea class="form-textarea" id="thread-reply-input"
                    placeholder="Reply to worker…"
                    style="min-height:64px;resize:vertical;"></textarea>
          <div style="display:flex;justify-content:flex-end;gap:.5rem;">
            <button class="btn btn--ghost" data-action="close">Cancel</button>
            <button class="btn btn--primary" data-action="send">Send</button>
          </div>
        </div>
      </div>`;

    document.body.appendChild(backdrop);
    const body = backdrop.querySelector('#thread-msg-body');
    body.scrollTop = body.scrollHeight;
    backdrop.querySelector('#thread-reply-input')?.focus();

    backdrop.addEventListener('click', async (e) => {
      const action = e.target.closest('[data-action]')?.dataset.action;
      if (!action) return;
      if (action === 'close') { backdrop.remove(); return; }
      if (action === 'send') {
        const input = backdrop.querySelector('#thread-reply-input');
        const text  = input?.value.trim();
        if (!text) return;
        const sendBtn = backdrop.querySelector('[data-action="send"]');
        sendBtn.disabled = true;
        sendBtn.textContent = 'Sending…';
        try {
          await api(`/orders/${orderId}/message-worker`, {
            method: 'POST',
            body: JSON.stringify({ text }),
          });
          backdrop.remove();
          BB.showToast('Message sent ✓', 'success');
        } catch (err) {
          sendBtn.disabled = false;
          sendBtn.textContent = 'Send';
          BB.showToast(`Send failed: ${err.message}`, 'error');
        }
      }
    });

    const onKeyDown = (e) => {
      if (e.key === 'Escape') { backdrop.remove(); document.removeEventListener('keydown', onKeyDown); }
    };
    document.addEventListener('keydown', onKeyDown);
  }

  function threadBubble(msg) {
    const isWorker = msg.role === 'user';
    const cls      = isWorker ? 'worker' : 'owner';
    const label    = isWorker ? 'Worker' : 'Bot / Owner';
    const time     = new Date(msg.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    return `
      <div class="thread-bubble thread-bubble--${cls}">
        <div class="thread-bubble__body">${BB.escapeHtml(msg.content)}</div>
        <span class="thread-bubble__meta">${label} · ${time}</span>
      </div>`;
  }

  // ── Gear button + menu ───────────────────────────────────────────── //

  function renderGearButton(wrap) {
    wrap.innerHTML = '';
    const btn = document.createElement('button');
    btn.className = 'btn btn--ghost btn--sm';
    btn.title = 'Order actions';
    btn.innerHTML = '⚙️';
    btn.style.cssText = 'padding:.25rem .5rem;font-size:1rem;line-height:1;';
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const alreadyOpen = wrap.querySelector('.gear-menu');
      closeAllMenus();
      if (!alreadyOpen) openGearMenu(wrap);
    });
    wrap.appendChild(btn);
  }

  function openGearMenu(wrap) {
    const orderId  = wrap.dataset.orderId;
    const row      = document.querySelector(`tr[data-order-id="${orderId}"]`);
    const stateEl  = row?.querySelector('.state-pill');
    const state    = stateEl?.textContent?.trim().toUpperCase().replace(/ /g, '_') ?? '';
    // F01: read current short_name from data attribute set during render.
    const currentShortName = row?.dataset.shortName ?? '';

    const done       = state === 'DONE';
    const cancelled  = state === 'CANCELLED';
    const unassigned = state === 'UNASSIGNED';
    const unavail    = state === 'UNAVAILABLE';
    const assigned   = state === 'ASSIGNED';
    const accepted   = state === 'ACCEPTED';
    const clarif     = state === 'PENDING_CLARIFICATION';
    const ready      = state === 'READY_FOR_PICKUP';
    const active     = assigned || accepted || clarif || ready;
    const terminal   = done;

    const menu = document.createElement('div');
    menu.className = 'gear-menu';
    menu.style.cssText = [
      'position:absolute;right:0;top:100%;z-index:200;',
      'background:var(--color-surface);border:1px solid var(--color-border);',
      'border-radius:var(--radius-md);min-width:220px;',
      'box-shadow:0 4px 20px rgba(0,0,0,.35);',
      'padding:.25rem 0;',
    ].join('');

    if (unassigned || unavail || cancelled) {
      addItem(menu, '👤 Assign worker',     'normal', () => openAssignWorker(orderId));
    }
    if (active) {
      addItem(menu, '🔄 Reassign worker',   'normal', () => openReassignWorker(orderId));
    }
    if (active || ready) {
      addItem(menu, '✅ Close (mark Done)',  'normal', () => closeOrder(orderId));
    }
    if (!done && !cancelled) {
      addItem(menu, '❌ Cancel order',      'danger', () => cancelOrder(orderId));
    }
    if (cancelled || unavail) {
      addItem(menu, '↩ Reset to Unassigned','normal', () => resetOrder(orderId));
    }
    if (clarif) {
      addItem(menu, '💬 Reply to worker…',  'normal', () => {
        const desc = row?.querySelector('.order-desc')?.textContent ?? orderId;
        openThreadModal(orderId, desc);
      });
    }

    if (!terminal) addDivider(menu);
    if (!terminal) {
      addItem(menu, '✏️ Edit description',  'normal', () => editDescription(orderId));
    }

    // F01: Set / edit / clear job name — available at any non-deleted state.
    const shortNameLabel = currentShortName
      ? `🏷 Edit job name (${currentShortName})`
      : '🏷 Set job name';
    addItem(menu, shortNameLabel, 'normal', () => editShortName(orderId, currentShortName));

    if (!terminal) {
      addSectionLabel(menu, 'Force state (bypass messaging)');
      addItem(menu, '→ Force Accepted',       'warn', () => forceState(orderId, 'force-accepted'));
      addItem(menu, '→ Force Unavailable',    'warn', () => forceState(orderId, 'force-unavailable'));
      addItem(menu, '→ Force Clarification',  'warn', () => forceState(orderId, 'force-clarification'));
      addItem(menu, '→ Force Ready',          'warn', () => forceState(orderId, 'force-ready'));
    }
    if (done || cancelled || unassigned || unavail) {
      addDivider(menu);
      addItem(menu, '🗑 Delete order',       'danger', () => deleteOrder(orderId));
    }

    wrap.appendChild(menu);
  }

  function addItem(menu, label, kind, onClick) {
    const btn = document.createElement('button');
    btn.textContent = label;
    btn.style.cssText = [
      'display:block;width:100%;padding:.45rem .9rem;text-align:left;',
      'background:none;border:none;cursor:pointer;font-size:var(--text-sm);',
      'font-family:var(--font-body);transition:background .1s;',
      kind === 'danger' ? 'color:var(--color-state-err);'
        : kind === 'warn' ? 'color:var(--color-state-warn);'
        : 'color:var(--color-text);',
    ].join('');
    btn.onmouseenter = () => btn.style.background = 'var(--color-surface-2)';
    btn.onmouseleave = () => btn.style.background = 'none';
    btn.addEventListener('click', (e) => { e.stopPropagation(); closeAllMenus(); onClick(); });
    menu.appendChild(btn);
  }

  function addDivider(menu) {
    const hr = document.createElement('div');
    hr.style.cssText = 'border-top:1px solid var(--color-border);margin:.25rem 0;';
    menu.appendChild(hr);
  }

  function addSectionLabel(menu, text) {
    const lbl = document.createElement('div');
    lbl.textContent = text;
    lbl.style.cssText = [
      'padding:.3rem .9rem .15rem;font-size:var(--text-xs);',
      'color:var(--color-text-muted);font-weight:600;',
      'letter-spacing:.06em;text-transform:uppercase;',
    ].join('');
    menu.appendChild(lbl);
  }

  function closeAllMenus() {
    document.querySelectorAll('.gear-menu').forEach(m => m.remove());
  }

  // ── Create Order ─────────────────────────────────────────────────── //

  document.getElementById('new-customer-btn').addEventListener('click', () => {
    document.getElementById('new-customer-row').style.display = '';
  });

  document.getElementById('create-order-btn').addEventListener('click', async () => {
    const descEl     = document.getElementById('order-description');
    const customerEl = document.getElementById('order-customer');
    const newNameEl  = document.getElementById('new-customer-name');
    // F01: read short_name.
    const shortNameEl = document.getElementById('order-short-name');

    const description = descEl.value.trim();
    if (!description) { BB.showToast('Description required', 'error'); return; }

    const shortName = shortNameEl?.value.trim() || null;
    if (shortName && shortName.length > 20) {
      BB.showToast('Job name must be 20 characters or fewer', 'error');
      return;
    }

    let customerId = customerEl.value;
    const newName  = newNameEl.value.trim();
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
      await api('/orders', {
        method: 'POST',
        body: JSON.stringify({ customer_id: customerId, description, short_name: shortName }),
      });
      BB.closeModal('create-order-modal');
      descEl.value = '';
      newNameEl.value = '';
      if (shortNameEl) { shortNameEl.value = ''; shortNameCounter.textContent = '0/20'; }
      document.getElementById('new-customer-row').style.display = 'none';
      await refreshOrderList();
    } catch (e) {
      BB.showToast(`Create order failed: ${e.message}`, 'error');
    }
  });

  // ── F01: Set / edit short name ───────────────────────────────────── //

  async function editShortName(orderId, currentValue) {
    const backdrop = document.createElement('div');
    backdrop.className = 'modal-backdrop';
    backdrop.innerHTML = `
      <div class="modal" style="width:420px;">
        <div class="modal__header">
          <span class="modal__title">🏷 Set job name</span>
          <button class="btn btn--ghost btn--sm" data-action="cancel">✕</button>
        </div>
        <div class="modal__body">
          <div class="form-group">
            <label class="form-label" for="sn-input">
              Job name <span class="text-muted" style="font-weight:400;">(max 20 chars)</span>
            </label>
            <input class="form-input font-mono" id="sn-input" type="text"
                   maxlength="20" value="${BB.escapeHtml(currentValue)}"
                   placeholder="e.g. AC-B3, ท่อชั้น2" autocomplete="off">
            <span class="text-xs text-muted" id="sn-counter">${currentValue.length}/20</span>
          </div>
          <p class="text-xs text-muted">
            Leave blank to clear the job name.
            Names must be unique within this branch.
          </p>
        </div>
        <div class="modal__footer">
          <button class="btn btn--ghost" data-action="cancel">Cancel</button>
          <button class="btn btn--primary" data-action="save">Save</button>
        </div>
      </div>`;

    document.body.appendChild(backdrop);

    const input   = backdrop.querySelector('#sn-input');
    const counter = backdrop.querySelector('#sn-counter');
    input.focus();
    input.select();
    input.addEventListener('input', () => {
      counter.textContent = `${input.value.length}/20`;
    });

    backdrop.addEventListener('click', async (e) => {
      const action = e.target.closest('[data-action]')?.dataset.action;
      if (!action) return;
      if (action === 'cancel') { backdrop.remove(); return; }

      const newValue = input.value.trim();

      // Nothing changed — close silently.
      if (newValue === currentValue) { backdrop.remove(); return; }

      try {
        await api(`/orders/${orderId}/short-name`, {
          method: 'PATCH',
          body: JSON.stringify({ short_name: newValue || null }),
        });
        backdrop.remove();

        // Update the row's data attribute so the gear menu reflects the new value
        // until the next SSE refresh rebuilds the row.
        const row = document.querySelector(`tr[data-order-id="${orderId}"]`);
        if (row) {
          row.dataset.shortName = newValue;
          // Update or remove the tag in the description cell.
          const descCell = row.querySelector('td:nth-child(2)');
          if (descCell) {
            const existingTag = descCell.querySelector('.order-tag');
            if (newValue) {
              if (existingTag) {
                existingTag.textContent = newValue;
              } else {
                const tag = document.createElement('span');
                tag.className = 'order-tag';
                tag.title = 'Job name';
                tag.textContent = newValue;
                descCell.prepend(document.createTextNode(' '));
                descCell.prepend(tag);
              }
            } else {
              existingTag?.remove();
              // Remove any leading text node that was the space after the tag.
              if (descCell.firstChild?.nodeType === Node.TEXT_NODE) {
                descCell.firstChild.remove();
              }
            }
          }
        }

        const msg = newValue ? `Job name set to "${newValue}" ✓` : 'Job name cleared';
        BB.showToast(msg, 'success');
      } catch (err) {
        const msg = err.message.includes('409') || err.message.toLowerCase().includes('already exists')
          ? 'That job name is already used by another order in this branch.'
          : `Save failed: ${err.message}`;
        BB.showToast(msg, 'error');
      }
    });
  }

  // ── Assign / Reassign ────────────────────────────────────────────── //

  function openAssignWorker(orderId) {
    pendingAssignOrderId   = orderId;
    pendingReassignOrderId = null;
    document.getElementById('assign-worker-modal-title').textContent = 'Assign Worker';
    BB.openModal('assign-worker-modal');
  }

  function openReassignWorker(orderId) {
    pendingReassignOrderId = orderId;
    pendingAssignOrderId   = null;
    document.getElementById('assign-worker-modal-title').textContent = 'Reassign Worker';
    BB.openModal('assign-worker-modal');
  }

  document.getElementById('assign-worker-btn').addEventListener('click', async () => {
    const workerId = document.getElementById('assign-worker-select').value;
    if (!workerId) { BB.showToast('Select a worker', 'error'); return; }

    if (pendingReassignOrderId) {
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
  });

  // ── Force state ──────────────────────────────────────────────────── //

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

  // ── Edit description ─────────────────────────────────────────────── //

  async function editDescription(orderId) {
    const descEl  = document.getElementById(`desc-${orderId}`);
    const current = descEl?.textContent?.trim() ?? '';

    const backdrop = document.createElement('div');
    backdrop.className = 'modal-backdrop';
    backdrop.innerHTML = `
      <div class="modal" style="width:480px;">
        <div class="modal__header">
          <span class="modal__title">✏️ Edit description</span>
          <button class="btn btn--ghost btn--sm" data-action="cancel">✕</button>
        </div>
        <div class="modal__body">
          <div class="form-group">
            <label class="form-label">Before</label>
            <div style="padding:.5rem .75rem;background:var(--color-surface-2);
                        border:1px solid var(--color-border);border-radius:var(--radius-sm);
                        font-size:var(--text-sm);color:var(--color-text-muted);
                        white-space:pre-wrap;">${BB.escapeHtml(current)}</div>
          </div>
          <div class="form-group">
            <label class="form-label">After</label>
            <textarea class="form-textarea" id="edit-desc-input"
                      style="min-height:100px;">${BB.escapeHtml(current)}</textarea>
          </div>
          <p class="text-xs text-muted" style="margin-top:-.25rem;">
            Worker will receive: old description → new description.
          </p>
        </div>
        <div class="modal__footer">
          <button class="btn btn--ghost" data-action="cancel">Cancel</button>
          <button class="btn btn--primary" data-action="save">Save &amp; notify worker</button>
        </div>
      </div>`;

    document.body.appendChild(backdrop);
    const textarea = backdrop.querySelector('#edit-desc-input');
    textarea.focus();
    textarea.setSelectionRange(textarea.value.length, textarea.value.length);

    backdrop.addEventListener('click', async (e) => {
      const action = e.target.closest('[data-action]')?.dataset.action;
      if (!action) return;
      if (action === 'cancel') { backdrop.remove(); return; }

      const newDesc = textarea.value.trim();
      if (!newDesc) { BB.showToast('Description cannot be empty', 'error'); return; }
      if (newDesc === current) { backdrop.remove(); return; }

      try {
        await api(`/orders/${orderId}/description`, {
          method: 'PATCH',
          body: JSON.stringify({ description: newDesc }),
        });
        backdrop.remove();
        if (descEl) descEl.textContent = newDesc;
        BB.showToast('Description updated and worker notified ✓', 'success');
      } catch (err) {
        BB.showToast(`Save failed: ${err.message}`, 'error');
      }
    });
  }

  // ── Cancel / Reset / Close ───────────────────────────────────────── //

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

  // ── Delete ───────────────────────────────────────────────────────── //

  async function deleteOrder(orderId) {
    const ok = await BB.confirm('Permanently delete this order? This cannot be undone.');
    if (!ok) return;
    try {
      await api(`/orders/${orderId}`, { method: 'DELETE' });
      const row = document.querySelector(`tr[data-order-id="${orderId}"]`);
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