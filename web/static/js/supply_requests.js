/**
 * D05 / P05 / T09 / T16-03: Supply Requests page — client-side logic.
 * T09: list is now paginated ({ items, next_cursor }).
 *      State filter dropdown + infinite scroll.
 * P05: Approve-Invoice modal shows inline media (image/PDF) when available.
 * T16-03: Mobile card layout — srCardHtml() mirrors srRowHtml().
 *         appendRows() and refreshList() target both #sr-tbody and #sr-cards-list.
 */

function initSupplyRequestsPage(branchId, initialCursor) {
  const api = (path, opts) => BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  let pendingApproveSupplyRequestId = null;

  // ── T09: Pagination state ─────────────────────────────────────── //

  let nextCursor = initialCursor ?? null;
  let isLoading  = false;
  let allLoaded  = (initialCursor === null || initialCursor === undefined);

  function currentFilter() {
    return {
      state: document.getElementById('sr-filter-state')?.value || '',
    };
  }

  function filterToParams(filter, cursor) {
    const params = new URLSearchParams();
    if (filter.state) params.set('state', filter.state);
    if (cursor)       params.set('after', cursor);
    params.set('limit', '50');
    return params.toString();
  }

  // ── IntersectionObserver ──────────────────────────────────────── //

  const sentinel = document.getElementById('sr-scroll-sentinel');
  const statusEl = document.getElementById('sr-load-status');

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
      const data = await api(`/supply-requests?${qs}`);
      appendRows(data.items);
      nextCursor = data.next_cursor ?? null;
      allLoaded  = !nextCursor;
      setStatus(allLoaded ? 'All supply requests loaded.' : '');
    } catch (e) {
      setStatus('');
      BB.showToast(`Load failed: ${e.message}`, 'error');
    } finally {
      isLoading = false;
    }
  }

  // ── T16-03: appendRows targets both table and card list ───────── //

  function appendRows(srs) {
    const tbody     = document.getElementById('sr-tbody');
    const cardsList = document.getElementById('sr-cards-list');

    // Remove empty-state placeholders
    if (tbody) {
      const emptyRow = tbody.querySelector('td[colspan]');
      if (emptyRow) emptyRow.closest('tr')?.remove();
    }
    if (cardsList) {
      const emptyCard = cardsList.querySelector('.sr-cards-empty');
      if (emptyCard) emptyCard.remove();
    }

    for (const sr of srs) {
      if (tbody) {
        const tr = document.createElement('tr');
        tr.innerHTML = srRowHtml(sr);
        tbody.appendChild(tr);
      }
      if (cardsList) {
        const div = document.createElement('div');
        div.innerHTML = srCardHtml(sr);
        const card = div.firstElementChild;
        if (card) cardsList.appendChild(card);
      }
    }

    attachRowActions();
  }

  // ── Filter bar ────────────────────────────────────────────────── //

  document.getElementById('sr-filter-state')?.addEventListener('change', () => {
    nextCursor = null;
    allLoaded  = false;
    refreshList();
  });

  // ── SSE wiring ───────────────────────────────────────────────── //

  new BranchEventSource(branchId)
    .withBadge(document.getElementById('live-badge'))
    .on('SupplyRequestChanged', () => {
      nextCursor = null;
      allLoaded  = false;
      refreshList();
    })
    .connect();

  loadInFlightOrders();
  attachRowActions();

  // ── List refresh (page 1) ─────────────────────────────────────── //

  async function refreshList() {
    isLoading = true;
    setStatus('');

    const qs = filterToParams(currentFilter(), null);
    let data;
    try {
      data = await api(`/supply-requests?${qs}`);
    } catch (e) {
      BB.showToast(`Refresh failed: ${e.message}`, 'error');
      isLoading = false;
      return;
    }

    const tbody     = document.getElementById('sr-tbody');
    const cardsList = document.getElementById('sr-cards-list');

    if (data.items.length === 0) {
      if (tbody) {
        tbody.innerHTML =
          '<tr><td colspan="4" class="data-table__empty">No supply requests yet.</td></tr>';
      }
      if (cardsList) {
        cardsList.innerHTML = '<p class="sr-cards-empty">No supply requests yet.</p>';
      }
    } else {
      if (tbody) {
        tbody.innerHTML = data.items.map(srRowHtml).join('');
      }
      if (cardsList) {
        cardsList.innerHTML = data.items.map(srCardHtml).join('');
      }
    }

    attachRowActions();
    nextCursor = data.next_cursor ?? null;
    allLoaded  = !nextCursor;
    setStatus(allLoaded && data.items.length > 0 ? 'All supply requests loaded.' : '');
    isLoading = false;
  }

  // ── Row HTML (desktop table) ──────────────────────────────────── //

  function srRowHtml(sr) {
    const pill = BB.statePill(sr.state);
    const chips = (sr.order_ids ?? [])
      .map(id => `<a href="/branches/${branchId}/orders" class="chip" title="${id}">${BB.shortId(id)}</a>`)
      .join('');
    return `
      <tr data-sr-id="${sr.id}" data-state="${sr.state}">
        <td>${pill}</td>
        <td>${BB.escapeHtml(sr.description)}</td>
        <td><div class="chip-list">${chips}</div></td>
        <td><div class="sr-actions" data-sr-id="${sr.id}" style="display:flex;gap:.5rem;"></div></td>
      </tr>`;
  }

  // ── T16-03: Card HTML (mobile) ────────────────────────────────── //

  function srCardHtml(sr) {
    const pill = BB.statePill(sr.state);
    const chips = (sr.order_ids ?? [])
      .map(id => `<a href="/branches/${branchId}/orders" class="chip" title="${id}">${BB.shortId(id)}</a>`)
      .join('');
    const hasChips = (sr.order_ids ?? []).length > 0;

    return `
      <div class="sr-card" data-sr-id="${sr.id}" data-state="${sr.state}">
        <div class="sr-card__header">
          ${pill}
        </div>
        <div class="sr-card__desc">${BB.escapeHtml(sr.description)}</div>
        ${hasChips ? `
        <div class="sr-card__divider"></div>
        <div class="sr-card__orders">
          <span class="sr-card__orders-label">Orders</span>
          <div class="chip-list">${chips}</div>
        </div>` : ''}
        <div class="sr-card__actions sr-actions" data-sr-id="${sr.id}"></div>
      </div>`;
  }

  // ── Row / card actions (shared logic) ────────────────────────── //

  function attachRowActions() {
    document.querySelectorAll('.sr-actions').forEach(renderActions);
  }

  function renderActions(container) {
    const srId      = container.dataset.srId;
    // Find state from closest table row OR card
    const parent    = container.closest('tr[data-sr-id], .sr-card[data-sr-id]');
    const state = parent?.dataset.state ?? '';
    container.innerHTML = '';

    if (state === 'DRAFT') {
      const btn = document.createElement('button');
      btn.className = 'btn btn--primary btn--sm';
      btn.textContent = 'Send to Supplier';
      btn.onclick = () => sendSupplyRequest(srId);
      container.appendChild(btn);
    }

    if (state === 'INVOICE_RECEIVED') {
      const btn = document.createElement('button');
      btn.className = 'btn btn--primary btn--sm';
      btn.textContent = 'Approve Invoice';
      btn.onclick = () => openApproveInvoice(srId);
      container.appendChild(btn);
    }
  }

  // ── Send supply request ───────────────────────────────────────── //

  async function sendSupplyRequest(srId) {
    const ok = await BB.confirm('Send to Supplier via WhatsApp?');
    if (!ok) return;
    try {
      await api(`/supply-requests/${srId}/send`, { method: 'POST' });
      BB.showToast('Supply request sent', 'success');
      await refreshList();
    } catch (e) {
      BB.showToast(`Send failed: ${e.message}`, 'error');
    }
  }

  // ── Create Supply Request ─────────────────────────────────────── //

  document.getElementById('create-sr-btn').addEventListener('click', async () => {
    const desc = document.getElementById('sr-description').value.trim();
    if (!desc) { BB.showToast('Description required', 'error'); return; }

    const sel = document.getElementById('sr-order-ids');
    const orderIds = Array.from(sel.selectedOptions).map(o => o.value);

    try {
      await api('/supply-requests', {
        method: 'POST',
        body: JSON.stringify({ description: desc, order_ids: orderIds }),
      });
      BB.closeModal('create-sr-modal');
      document.getElementById('sr-description').value = '';
      sel.selectedIndex = -1;
      nextCursor = null;
      allLoaded  = false;
      await refreshList();
    } catch (e) {
      BB.showToast(`Create failed: ${e.message}`, 'error');
    }
  });

  // ── Approve Invoice (P05: with media preview) ─────────────────── //

  async function openApproveInvoice(srId) {
    pendingApproveSupplyRequestId = srId;
    await loadInvoices(srId);
    BB.openModal('approve-invoice-modal');
  }

  document.getElementById('approve-invoice-select').addEventListener('change', async (e) => {
    await renderInvoiceMedia(e.target.value);
  });

  async function renderInvoiceMedia(invoiceId) {
    const container = document.getElementById('invoice-media-preview');
    if (!container) return;
    container.innerHTML = '';
    if (!invoiceId) return;

    const invoices = await api('/invoices?state=Sent').catch(() => []);
    const invoice = invoices.find ? invoices.find(i => i.id === invoiceId) : null;
    if (!invoice?.has_media) {
      container.innerHTML = '<p class="text-xs text-muted">No image attached.</p>';
      return;
    }

    const mediaUrl = `/api/v1/branches/${branchId}/invoices/${invoiceId}/media`;
    container.innerHTML = `
      <img src="${mediaUrl}" alt="Invoice media"
           style="max-width:100%;border-radius:var(--radius-sm);border:1px solid var(--color-border);"
           onerror="this.replaceWith(Object.assign(document.createElement('a'), {href:'${mediaUrl}',textContent:'Download invoice',target:'_blank',className:'btn btn--ghost btn--sm'}))">`;
  }

  document.getElementById('approve-invoice-btn').addEventListener('click', async () => {
    const invoiceId = document.getElementById('approve-invoice-select').value;
    if (!invoiceId) { BB.showToast('Select an invoice', 'error'); return; }

    const ok = await BB.confirm('Approve this invoice? This is a financial commitment and cannot be undone.');
    if (!ok) return;

    try {
      await api(`/supply-requests/${pendingApproveSupplyRequestId}/approve-invoice`, {
        method: 'POST',
        body: JSON.stringify({ invoice_id: invoiceId }),
      });
      BB.closeModal('approve-invoice-modal');
      BB.showToast('Invoice approved', 'success');
      await refreshList();
    } catch (e) {
      BB.showToast(`Approve failed: ${e.message}`, 'error');
    }
  });

  // ── Data loaders ─────────────────────────────────────────────── //

  async function loadInFlightOrders() {
    try {
      const data = await api('/orders');
      // Handle both paginated ({ items }) and legacy (array) shapes.
      const orders = Array.isArray(data) ? data : (data.items ?? []);
      const active = orders.filter(o => !['DONE', 'CANCELLED'].includes(o.state));
      const sel = document.getElementById('sr-order-ids');
      if (sel) {
        sel.innerHTML = active
          .map(o => `<option value="${o.id}">${BB.shortId(o.id)} — ${BB.escapeHtml(o.description)}</option>`)
          .join('');
      }
    } catch { /* non-fatal */ }
  }

  async function loadInvoices(srId) {
    try {
      const invoices = await api('/invoices?state=Sent');
      const list = Array.isArray(invoices) ? invoices : (invoices.items ?? []);
      const relevant = list.filter(i => i.supply_request_id === srId);
      const sel = document.getElementById('approve-invoice-select');
      if (relevant.length === 0) {
        sel.innerHTML = '<option value="">No Sent invoices for this request</option>';
        document.getElementById('invoice-media-preview').innerHTML = '';
      } else {
        sel.innerHTML = relevant
          .map(i => `<option value="${i.id}">${BB.shortId(i.id)}${i.notes ? ` — ${BB.escapeHtml(i.notes)}` : ''}${i.has_media ? ' 📎' : ''}</option>`)
          .join('');
        await renderInvoiceMedia(relevant[0].id);
      }
    } catch { /* non-fatal */ }
  }
}