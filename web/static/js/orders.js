/**
 * D04 / P04 / P16 / F04 / F01 / T09: Orders page — full Owner control.
 *
 * T09 additions:
 *  - initOrdersPage now accepts initialCursor (passed from SSR)
 *  - Filter bar: state dropdown, worker dropdown, search input with debounce
 *  - Infinite scroll via IntersectionObserver on #orders-scroll-sentinel
 *  - SSE-triggered refresh reloads page 1 (cursor reset), shows toast if scrolled deep
 *  - Filter changes reset cursor and refetch from page 1
 *
 * F01 changes:
 *  - "Job name" input in Create Order modal (optional, ≤20 chars, with counter)
 *  - order row shows short_name tag when set
 *  - Gear menu has "✏️ Set job name" item (set/edit/clear)
 *  - 409 Conflict on duplicate short_name gives a readable error
 */

function initOrdersPage(branchId, initialCursor) {
  const api = (path, opts) => BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  let pendingAssignOrderId   = null;
  let pendingReassignOrderId = null;
  let customerMap = {};

  // ── T09: Pagination state ─────────────────────────────────────────── //

  let nextCursor = initialCursor ?? null; // null = no more pages
  let isLoading  = false;
  let allLoaded  = (initialCursor === null || initialCursor === undefined);

  // Current filter state (mirrors filter bar inputs).
  function currentFilter() {
    return {
      state:     document.getElementById('filter-state')?.value  || '',
      worker_id: document.getElementById('filter-worker')?.value || '',
      q:         document.getElementById('filter-q')?.value.trim() || '',
    };
  }

  function filterToParams(filter, cursor) {
    const params = new URLSearchParams();
    if (filter.state)     params.set('state',     filter.state);
    if (filter.worker_id) params.set('worker_id', filter.worker_id);
    if (filter.q)         params.set('q',         filter.q);
    if (cursor)           params.set('after',     cursor);
    params.set('limit', '50');
    return params.toString();
  }

  // ── IntersectionObserver for infinite scroll ──────────────────────── //

  const sentinel  = document.getElementById('orders-scroll-sentinel');
  const statusEl  = document.getElementById('orders-load-status');

  function setStatus(msg) {
    if (!statusEl) return;
    statusEl.textContent = msg;
    statusEl.style.display = msg ? '' : 'none';
  }

  const scrollObserver = new IntersectionObserver(
    async (entries) => {
      if (!entries[0].isIntersecting) return;
      if (isLoading || allLoaded) return;
      await loadNextPage();
    },
    { rootMargin: '200px' }
  );

  if (sentinel) scrollObserver.observe(sentinel);

  async function loadNextPage() {
    if (isLoading || allLoaded || !nextCursor) return;
    isLoading = true;
    setStatus('Loading…');

    try {
      const qs = filterToParams(currentFilter(), nextCursor);
      const data = await api(`/orders?${qs}`);
      appendRows(data.items);
      nextCursor = data.next_cursor ?? null;
      allLoaded  = !nextCursor;
      setStatus(allLoaded ? 'All orders loaded.' : '');
    } catch (e) {
      setStatus('');
      BB.showToast(`Load failed: ${e.message}`, 'error');
    } finally {
      isLoading = false;
    }
  }

  function appendRows(orders) {
  const tbody     = document.getElementById('orders-tbody');
  const cardsList = document.getElementById('orders-cards-list');
 
  // Remove empty-state placeholders
  if (tbody) {
    const emptyRow = tbody.querySelector('td[colspan]');
    if (emptyRow) emptyRow.closest('tr')?.remove();
  }
  if (cardsList) {
    const emptyCard = cardsList.querySelector('.order-cards-empty');
    if (emptyCard) emptyCard.remove();
  }
 
  for (const o of orders) {
    // Desktop table row
    if (tbody) {
      const tr = document.createElement('tr');
      tr.innerHTML = orderRowHtml(o);
      tbody.appendChild(tr);
    }
 
    // Mobile card
    if (cardsList) {
      const div = document.createElement('div');
      div.innerHTML = orderCardHtml(o);
      // orderCardHtml returns a single root .order-card div; grab it
      const card = div.firstElementChild;
      if (card) cardsList.appendChild(card);
    }
  }

  function hydrateSsrCardDates() {
    document.querySelectorAll('.order-card__dates[data-start-date], .order-card__dates[data-due-date]')
      .forEach(el => {
        if (!window.renderDateChips) return;
        const start = el.dataset.startDate || null;
        const due   = el.dataset.dueDate   || null;
        if (!start && !due) return;
        const html = window.renderDateChips(start || null, due || null);
        if (html) el.innerHTML = html;
      });
  }
 
  attachRowActions();
  hydrateSsrCardDates();
}

  // ── Filter bar ────────────────────────────────────────────────────── //

  let filterDebounce = null;

  function onFilterChange() {
    clearTimeout(filterDebounce);
    filterDebounce = setTimeout(resetAndRefetch, 300);
  }

  async function resetAndRefetch() {
    nextCursor = null;
    allLoaded  = false;
    await refreshOrderList();
  }

  document.getElementById('filter-state')?.addEventListener('change', onFilterChange);
  document.getElementById('filter-worker')?.addEventListener('change', onFilterChange);
  document.getElementById('filter-q')?.addEventListener('input', onFilterChange);

  document.getElementById('filter-clear-btn')?.addEventListener('click', () => {
    const s = document.getElementById('filter-state');
    const w = document.getElementById('filter-worker');
    const q = document.getElementById('filter-q');
    if (s) s.value = '';
    if (w) w.value = '';
    if (q) q.value = '';
    resetAndRefetch();
  });

  // ── On load ──────────────────────────────────────────────────────── //

  attachRowActions();
  loadCustomers();
  loadWorkersForFilter();

  const shortNameInput   = document.getElementById('order-short-name');
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
    .on('OrderChanged', () => {
      // SSE reload: reset to page 1. If user is scrolled deep, show a toast
      // rather than jarring them; they can scroll up to see new activity.
      const scrolled = window.scrollY > 600;
      refreshOrderList().then(() => {
        if (scrolled) {
          BB.showToast('List updated — scroll up to see recent activity.', 'info');
        }
      });
    })
    .connect();

  // ── Order list refresh (page 1) ──────────────────────────────────── //

  async function refreshOrderList() {
    isLoading = true;
    setStatus('');
  
    const qs = filterToParams(currentFilter(), null);
    let data;
    try {
      data = await api(`/orders?${qs}`);
    } catch (e) {
      BB.showToast(`Refresh failed: ${e.message}`, 'error');
      isLoading = false;
      return;
    }
  
    const tbody     = document.getElementById('orders-tbody');
    const cardsList = document.getElementById('orders-cards-list');
  
    if (data.items.length === 0) {
      const emptyMsg = 'No orders yet.';
      if (tbody) {
        tbody.innerHTML =
          `<tr><td colspan="5" class="data-table__empty">${emptyMsg}</td></tr>`;
      }
      if (cardsList) {
        cardsList.innerHTML =
          `<p class="order-cards-empty">${emptyMsg}</p>`;
      }
    } else {
      if (tbody) {
        tbody.innerHTML = data.items.map(orderRowHtml).join('');
      }
      if (cardsList) {
        cardsList.innerHTML = data.items.map(orderCardHtml).join('');
      }
    }
  
    attachRowActions();
    nextCursor = data.next_cursor ?? null;
    allLoaded  = !nextCursor;
    setStatus(allLoaded && data.items.length > 0 ? 'All orders loaded.' : '');
    isLoading = false;
  }

  // ── Row HTML builder ─────────────────────────────────────────────── //

  function orderRowHtml(o) {
    const pill     = BB.statePill(o.state);
    const customer = BB.escapeHtml(customerMap[o.customer_id] || BB.shortId(o.customer_id));
    const worker   = o.worker_name ? BB.escapeHtml(o.worker_name) : '—';

    const namePrefix = o.short_name
      ? `<span class="order-tag" title="Job name">${BB.escapeHtml(o.short_name)}</span> `
      : '';

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

    const aiBadge = o.ai_routed_low_confidence
      ? `<span class="ai-badge" title="AI-routed with low confidence — review recommended">🤖?</span>`
      : '';

    const datePart = window.renderDateChips
      ? window.renderDateChips(o.start_date, o.due_date)
      : '';

    const alertBell = o.alert_count > 0
      ? `<button class="alert-btn" data-order-id="${o.id}" data-branch-id="${branchId}"
               data-active="true" onclick="openAlertsModal('${o.id}',${JSON.stringify(o.description)},api)"
               title="Follow-up alerts">
           🔔 <span class="alert-count-badge">${o.alert_count}</span>
         </button>`
      : '';

    return `
      <tr data-order-id="${o.id}" data-state="${o.state}"
          data-short-name="${BB.escapeHtml(o.short_name || '')}"
          data-start-date="${o.start_date ?? ''}"
          data-due-date="${o.due_date ?? ''}">
        <td>${pill}</td>
        <td>${namePrefix}<span class="order-desc" id="desc-${o.id}">${BB.escapeHtml(o.description)}</span>
          ${datePart ? `<div style="display:flex;gap:var(--space-2);flex-wrap:wrap;margin-top:3px;">${datePart}</div>` : ''}
        </td>
        <td class="text-muted text-sm">${customer}</td>
        <td class="text-muted text-xs" id="worker-${o.id}">${worker}</td>
        <td>
          <div style="display:flex;gap:.5rem;align-items:center;flex-wrap:wrap;">
            ${threadBtn}${aiBadge}${alertBell}
            <div class="order-gear-wrap" data-order-id="${o.id}"
                 style="position:relative;display:inline-block;"></div>
          </div>
        </td>
      </tr>`;
  }

  function orderCardHtml(o) {
    const pill      = BB.statePill(o.state);
    const customer  = BB.escapeHtml(customerMap[o.customer_id] || BB.shortId(o.customer_id));
    const worker    = o.worker_name ? BB.escapeHtml(o.worker_name) : '—';
  
    const namePrefix = o.short_name
      ? `<span class="order-tag" title="Job name">${BB.escapeHtml(o.short_name)}</span> `
      : '';
  
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
  
    const aiBadge = o.ai_routed_low_confidence
      ? `<span class="ai-badge" title="AI-routed with low confidence — review recommended">🤖?</span>`
      : '';
  
    const alertBell = o.alert_count > 0
      ? `<button class="alert-btn" data-order-id="${o.id}" data-branch-id="${branchId}"
                data-active="true" title="Follow-up alerts">
          🔔 <span class="alert-count-badge">${o.alert_count}</span>
        </button>`
      : '';
  
    // Date chips: render via the same helper as the table row (f05_alerts.js)
    const datePart = window.renderDateChips
      ? window.renderDateChips(o.start_date, o.due_date)
      : '';
  
    return `
      <div class="order-card"
          data-order-id="${o.id}"
          data-state="${o.state}"
          data-short-name="${BB.escapeHtml(o.short_name || '')}"
          data-start-date="${o.start_date ?? ''}"
          data-due-date="${o.due_date ?? ''}">
  
        <!-- header: pill + gear -->
        <div class="order-card__header">
          ${pill}
          <div class="order-card-gear-wrap" data-order-id="${o.id}"
              style="position:relative;display:inline-block;"></div>
        </div>
  
        <!-- description -->
        <div class="order-card__desc">
          ${namePrefix}<span class="order-desc" id="desc-${o.id}">${BB.escapeHtml(o.description)}</span>
        </div>
  
        <!-- divider -->
        <div class="order-card__divider"></div>
  
        <!-- meta -->
        <div class="order-card__meta">
          <span>${customer}</span>
          <span class="order-card__meta-sep">·</span>
          <span id="worker-${o.id}">${worker}</span>
        </div>
  
        ${datePart ? `<div class="order-card__dates">${datePart}</div>` : ''}
  
        <!-- action buttons -->
        <div class="order-card__actions">
          ${threadBtn}${aiBadge}${alertBell}
        </div>
      </div>`;
  }


  function attachRowActions() {
    // Desktop: gear wraps in table rows
    document.querySelectorAll('.order-gear-wrap').forEach(renderGearButton);
    // Mobile: gear wraps in cards (same renderGearButton logic)
    document.querySelectorAll('.order-card-gear-wrap').forEach(renderGearButton);
  
    // Thread buttons exist in both contexts — single selector covers both
    document.querySelectorAll('.thread-btn').forEach(btn => {
      // Avoid double-binding if already wired
      if (btn.dataset.wired) return;
      btn.dataset.wired = '1';
      btn.addEventListener('click', () => {
        const orderId = btn.dataset.orderId;
        // Look for .order-desc in table row OR card
        const desc =
          document.querySelector(`[data-order-id="${orderId}"] .order-desc`)
            ?.textContent ?? orderId;
        openThreadModal(orderId, desc);
      });
    });
  
    // Alert buttons (f05_alerts.js openAlertsModal is already global)
    document.querySelectorAll('.alert-btn').forEach(btn => {
      if (btn.dataset.wired) return;
      btn.dataset.wired = '1';
      btn.addEventListener('click', () => {
        const orderId = btn.dataset.orderId;
        const desc =
          document.querySelector(`[data-order-id="${orderId}"] .order-desc`)
            ?.textContent ?? orderId;
        openAlertsModal(orderId, desc, api);
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
    // T16-06: keep modal height in sync with visual viewport so the
    // sticky footer stays above the iOS soft keyboard.
    if (window.visualViewport) {
      const onViewportResize = () => {
        const modal = backdrop.querySelector('.modal');
        if (modal) modal.style.height = `${window.visualViewport.height}px`;
      };
      window.visualViewport.addEventListener('resize', onViewportResize);
      backdrop._cleanupViewport = () =>
        window.visualViewport.removeEventListener('resize', onViewportResize);
    }
    const body = backdrop.querySelector('#thread-msg-body');
    body.scrollTop = body.scrollHeight;
    backdrop.querySelector('#thread-reply-input')?.focus();

    backdrop.addEventListener('click', async (e) => {
      const action = e.target.closest('[data-action]')?.dataset.action;
      if (!action) return;
      if (action === 'close') { 
        backdrop._cleanupViewport?.(); 
        backdrop.remove(); 
        return; 
      }
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
      if (e.key === 'Escape') {
        backdrop._cleanupViewport?.();   // T16-06
        backdrop.remove();
        document.removeEventListener('keydown', onKeyDown);
      }
    };
    document.addEventListener('keydown', onKeyDown);
  }

  function threadBubble(msg) {
    const isWorker = msg.role === 'user';
    const cls      = isWorker ? 'worker' : 'owner';
    const label    = isWorker ? 'Worker' : 'Bot / Owner';
    const time     = new Date(msg.created_at).toLocaleTimeString([], {
      hour: '2-digit', minute: '2-digit',
    });
    return `
      <div class="thread-bubble thread-bubble--${cls}">
        <div class="thread-bubble__body">${BB.escapeHtml(msg.content)}</div>
        <span class="thread-bubble__meta">${label} · ${time}</span>
      </div>`;
  }

  // ── Gear button + menu ───────────────────────────────────────────── //

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
    const orderId          = wrap.dataset.orderId;

    // Resolve state from table row OR mobile card (T16-02)
    const container        = document.querySelector(`tr[data-order-id="${orderId}"], .order-card[data-order-id="${orderId}"]`);
    const state    = container?.dataset.state ?? '';
    const currentShortName = container?.dataset.shortName ?? '';

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

    // ── Build items array ────────────────────────────────────────── //
    // Each entry is one of:
    //   { type: 'item',    label, kind, onClick }
    //   { type: 'divider' }
    //   { type: 'label',   text }

    const items = [];

    if (unassigned || unavail || cancelled) {
      items.push({ type: 'item', label: '👤 Assign worker',    kind: 'normal', onClick: () => openAssignWorker(orderId) });
    }
    if (active) {
      items.push({ type: 'item', label: '🔄 Reassign worker',  kind: 'normal', onClick: () => openReassignWorker(orderId) });
    }
    if (active || ready) {
      items.push({ type: 'item', label: '✅ Close (mark Done)', kind: 'normal', onClick: () => closeOrder(orderId) });
    }
    if (!done && !cancelled) {
      items.push({ type: 'item', label: '❌ Cancel order',      kind: 'danger', onClick: () => cancelOrder(orderId) });
    }
    if (cancelled || unavail) {
      items.push({ type: 'item', label: '↩ Reset to Unassigned', kind: 'normal', onClick: () => resetOrder(orderId) });
    }
    if (clarif) {
      items.push({ type: 'item', label: '💬 Reply to worker…', kind: 'normal', onClick: () => {
        const desc = container?.querySelector('.order-desc')?.textContent ?? orderId;
        openThreadModal(orderId, desc);
      }});
    }

    if (!terminal) items.push({ type: 'divider' });
    if (!terminal) {
      items.push({ type: 'item', label: '✏️ Edit description', kind: 'normal', onClick: () => editDescription(orderId) });
    }

    const shortNameLabel = currentShortName
      ? `🏷 Edit job name (${currentShortName})`
      : '🏷 Set job name';
    items.push({ type: 'item', label: shortNameLabel, kind: 'normal', onClick: () => editShortName(orderId, currentShortName) });

    items.push({ type: 'item', label: '📅 Set dates', kind: 'normal', onClick: () => {
      const startIso = container?.dataset.startDate ?? null;
      const dueIso   = container?.dataset.dueDate   ?? null;
      openDatesModal(orderId, startIso, dueIso, api);
    }});

    items.push({ type: 'item', label: '🔔 Follow-up alerts', kind: 'normal', onClick: () => {
      const desc = container?.querySelector('.order-desc')?.textContent ?? orderId;
      openAlertsModal(orderId, desc, api);
    }});

    if (!terminal) {
      items.push({ type: 'label', text: 'Force state (bypass messaging)' });
      items.push({ type: 'item', label: '→ Force Accepted',      kind: 'warn', onClick: () => forceState(orderId, 'force-accepted') });
      items.push({ type: 'item', label: '→ Force Unavailable',   kind: 'warn', onClick: () => forceState(orderId, 'force-unavailable') });
      items.push({ type: 'item', label: '→ Force Clarification', kind: 'warn', onClick: () => forceState(orderId, 'force-clarification') });
      items.push({ type: 'item', label: '→ Force Ready',         kind: 'warn', onClick: () => forceState(orderId, 'force-ready') });
    }
    if (done || cancelled || unassigned || unavail) {
      items.push({ type: 'divider' });
      items.push({ type: 'item', label: '🗑 Delete order', kind: 'danger', onClick: () => deleteOrder(orderId) });
    }

    // ── Render ───────────────────────────────────────────────────── //

    if (window.innerWidth <= 768) {
      openMobileActionSheet(items);
    } else {
      openDesktopDropdown(wrap, items);
    }
  }

  // ── Mobile: full-screen action sheet ─────────────────────────────── //

  function openMobileActionSheet(items) {
    const backdrop = document.createElement('div');
    backdrop.className = 'modal-backdrop';
    backdrop.innerHTML = `
      <div class="modal" style="display:flex;flex-direction:column;">
        <div class="modal__header">
          <span class="modal__title">Order actions</span>
          <button class="btn btn--ghost btn--sm" data-action="close">✕</button>
        </div>
        <div class="modal__body" style="padding:0;overflow-y:auto;">
          <div id="action-sheet-list"></div>
        </div>
      </div>`;

    const list = backdrop.querySelector('#action-sheet-list');

    items.forEach((entry) => {
      if (entry.type === 'divider') {
        const hr = document.createElement('div');
        hr.style.cssText = 'height:1px;background:var(--color-border);margin:var(--space-2) 0;';
        list.appendChild(hr);
        return;
      }

      if (entry.type === 'label') {
        const lbl = document.createElement('div');
        lbl.textContent = entry.text;
        lbl.style.cssText = [
          'padding:var(--space-2) var(--space-5) var(--space-1);',
          'font-size:var(--text-xs);color:var(--color-text-muted);',
          'font-weight:600;letter-spacing:.06em;text-transform:uppercase;',
        ].join('');
        list.appendChild(lbl);
        return;
      }

      // type === 'item'
      const btn = document.createElement('button');
      btn.textContent = entry.label;
      btn.style.cssText = [
        'display:block;width:100%;',
        'padding:var(--space-4) var(--space-5);',
        'text-align:left;background:none;border:none;',
        'border-bottom:1px solid var(--color-border);',
        'font-size:var(--text-base);font-family:var(--font-body);cursor:pointer;',
        'min-height:44px;',
        entry.kind === 'danger' ? 'color:var(--color-state-err);'
          : entry.kind === 'warn' ? 'color:var(--color-state-warn);'
          : 'color:var(--color-text);',
      ].join('');
      btn.addEventListener('click', () => {
        backdrop.remove();
        entry.onClick();
      });
      list.appendChild(btn);
    });

    backdrop.addEventListener('click', (e) => {
      if (e.target.closest('[data-action="close"]') || e.target === backdrop) {
        backdrop.remove();
      }
    });

    document.body.appendChild(backdrop);
  }

  // ── Desktop: absolute dropdown ────────────────────────────────────── //

  function openDesktopDropdown(wrap, items) {
    const menu = document.createElement('div');
    menu.className = 'gear-menu';
    menu.style.cssText = [
      'position:absolute;right:0;top:100%;z-index:200;',
      'background:var(--color-surface);border:1px solid var(--color-border);',
      'border-radius:var(--radius-md);min-width:220px;',
      'box-shadow:0 4px 20px rgba(0,0,0,.35);',
      'padding:.25rem 0;',
    ].join('');

    items.forEach((entry) => {
      if (entry.type === 'divider') {
        const hr = document.createElement('div');
        hr.style.cssText = 'border-top:1px solid var(--color-border);margin:.25rem 0;';
        menu.appendChild(hr);
        return;
      }

      if (entry.type === 'label') {
        const lbl = document.createElement('div');
        lbl.textContent = entry.text;
        lbl.style.cssText = [
          'padding:.3rem .9rem .15rem;font-size:var(--text-xs);',
          'color:var(--color-text-muted);font-weight:600;',
          'letter-spacing:.06em;text-transform:uppercase;',
        ].join('');
        menu.appendChild(lbl);
        return;
      }

      const btn = document.createElement('button');
      btn.textContent = entry.label;
      btn.style.cssText = [
        'display:block;width:100%;padding:.45rem .9rem;text-align:left;',
        'background:none;border:none;cursor:pointer;font-size:var(--text-sm);',
        'font-family:var(--font-body);transition:background .1s;',
        entry.kind === 'danger' ? 'color:var(--color-state-err);'
          : entry.kind === 'warn' ? 'color:var(--color-state-warn);'
          : 'color:var(--color-text);',
      ].join('');
      btn.onmouseenter = () => btn.style.background = 'var(--color-surface-2)';
      btn.onmouseleave = () => btn.style.background = 'none';
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        closeAllMenus();
        entry.onClick();
      });
      menu.appendChild(btn);
    });

    wrap.appendChild(menu);
  }

  function closeAllMenus() {
    document.querySelectorAll('.gear-menu').forEach(m => m.remove());
  }

  // ── Create Order ─────────────────────────────────────────────────── //

  document.getElementById('new-customer-btn').addEventListener('click', () => {
    document.getElementById('new-customer-row').style.display = '';
  });

  document.getElementById('create-order-btn').addEventListener('click', async () => {
    const descEl      = document.getElementById('order-description');
    const customerEl  = document.getElementById('order-customer');
    const newNameEl   = document.getElementById('new-customer-name');
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
      if (shortNameEl) {
        shortNameEl.value = '';
        if (shortNameCounter) shortNameCounter.textContent = '0/20';
      }
      document.getElementById('new-customer-row').style.display = 'none';
      await resetAndRefetch();
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
            Leave blank to clear. Names must be unique within this branch.
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
      if (newValue === currentValue) { backdrop.remove(); return; }

      try {
        await api(`/orders/${orderId}/short-name`, {
          method: 'PATCH',
          body: JSON.stringify({ short_name: newValue || null }),
        });
        backdrop.remove();

        const row = document.querySelector(`tr[data-order-id="${orderId}"]`);
        if (row) {
          row.dataset.shortName = newValue;
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

  // ── Cancel / Reset / Close / Delete ─────────────────────────────── //

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

  async function deleteOrder(orderId) {
    const ok = await BB.confirm('Permanently delete this order? This cannot be undone.');
    if (!ok) return;
    try {
      await api(`/orders/${orderId}`, { method: 'DELETE' });
      const row = document.querySelector(`tr[data-order-id="${orderId}"]`);
      row?.remove();
      // mobile card (T16-02)
      const card = document.querySelector(`.order-card[data-order-id="${orderId}"]`);
      card?.remove()
      maybeShowEmpty();
      BB.showToast('Order deleted', 'success');
    } catch (e) {
      BB.showToast(`Delete failed: ${e.message}`, 'error');
    }
  }

  function maybeShowEmpty() {
    const tbody     = document.getElementById('orders-tbody');
    const cardsList = document.getElementById('orders-cards-list');
  
    const tableEmpty = tbody &&
      tbody.querySelectorAll('tr[data-order-id]').length === 0;
    const cardsEmpty = cardsList &&
      cardsList.querySelectorAll('.order-card').length === 0;
  
    if (tableEmpty && tbody) {
      tbody.innerHTML =
        '<tr><td colspan="5" class="data-table__empty">No orders yet.</td></tr>';
    }
    if (cardsEmpty && cardsList) {
      cardsList.innerHTML =
        '<p class="order-cards-empty">No orders yet.</p>';
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

  async function loadWorkersForFilter() {
    try {
      const workers = await api('/workers');

      // Populate assign/reassign modal select.
      const assignSel = document.getElementById('assign-worker-select');
      if (assignSel) {
        assignSel.innerHTML = '<option value="">Select worker…</option>' +
          workers.map(w => `<option value="${w.id}">${BB.escapeHtml(w.name)}</option>`).join('');
      }

      // T09: populate filter bar worker dropdown.
      const filterSel = document.getElementById('filter-worker');
      if (filterSel) {
        filterSel.innerHTML = '<option value="">All workers</option>' +
          workers
            .filter(w => w.bound)
            .map(w => `<option value="${w.id}">${BB.escapeHtml(w.name)}</option>`)
            .join('');
      }
    } catch { /* non-fatal */ }
  }
}