/**
 * D05 / P05: Supply Requests page — client-side logic.
 * P05: Approve-Invoice modal now shows inline media (image/PDF) when available.
 */

function initSupplyRequestsPage(branchId) {
  const api = (path, opts) => BB.apiFetch(`/api/v1/branches/${branchId}${path}`, opts);

  let pendingApproveSupplyRequestId = null;

  // ── SSE wiring ───────────────────────────────────────────────────── //

  new BranchEventSource(branchId)
    .withBadge(document.getElementById('live-badge'))
    .on('SupplyRequestChanged', () => refreshList())
    .connect();

  loadInFlightOrders();
  attachRowActions();

  // ── List refresh ─────────────────────────────────────────────────── //

  async function refreshList() {
    let rows;
    try { rows = await api('/supply-requests'); }
    catch (e) { BB.showToast(`Refresh failed: ${e.message}`, 'error'); return; }

    const tbody = document.getElementById('sr-tbody');
    if (rows.length === 0) {
      tbody.innerHTML = '<tr><td colspan="4" class="data-table__empty">No supply requests yet.</td></tr>';
      return;
    }

    tbody.innerHTML = rows.map(srRowHtml).join('');
    attachRowActions();
  }

  function srRowHtml(sr) {
    const pill = BB.statePill(sr.state);
    const chips = (sr.order_ids ?? [])
      .map(id => `<a href="/branches/${branchId}/orders" class="chip" title="${id}">${BB.shortId(id)}</a>`)
      .join('');
    return `
      <tr data-sr-id="${sr.id}">
        <td>${pill}</td>
        <td>${BB.escapeHtml(sr.description)}</td>
        <td><div class="chip-list">${chips}</div></td>
        <td><div class="sr-actions" data-sr-id="${sr.id}" style="display:flex;gap:.5rem;"></div></td>
      </tr>`;
  }

  function attachRowActions() {
    document.querySelectorAll('.sr-actions').forEach(renderActions);
  }

  function renderActions(container) {
    const srId = container.dataset.srId;
    const row = container.closest('tr');
    const statePill = row?.querySelector('.state-pill');
    const state = statePill?.textContent?.trim().toUpperCase().replace(/ /g, '_') ?? '';
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

  // ── Send supply request ───────────────────────────────────────────── //

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

  // ── Create Supply Request ─────────────────────────────────────────── //

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
      await refreshList();
    } catch (e) {
      BB.showToast(`Create failed: ${e.message}`, 'error');
    }
  });

  // ── Approve Invoice (P05: with media preview) ─────────────────────── //

  async function openApproveInvoice(srId) {
    pendingApproveSupplyRequestId = srId;
    await loadInvoices(srId);
    BB.openModal('approve-invoice-modal');
  }

  document.getElementById('approve-invoice-select').addEventListener('change', async (e) => {
    const invoiceId = e.target.value;
    await renderInvoiceMedia(invoiceId);
  });

  async function renderInvoiceMedia(invoiceId) {
    const container = document.getElementById('invoice-media-preview');
    if (!container) return;
    container.innerHTML = '';
    if (!invoiceId) return;

    // Check if this invoice has media.
    const invoices = await api('/invoices?state=Sent').catch(() => []);
    const invoice = invoices.find(i => i.id === invoiceId);
    if (!invoice?.has_media) {
      container.innerHTML = '<p class="text-xs text-muted">No image attached.</p>';
      return;
    }

    // P05: load the media via the dedicated endpoint.
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

  // ── Data loaders ─────────────────────────────────────────────────── //

  async function loadInFlightOrders() {
    try {
      const orders = await api('/orders');
      const active = orders.filter(o => !['DONE', 'CANCELLED'].includes(o.state));
      const sel = document.getElementById('sr-order-ids');
      sel.innerHTML = active
        .map(o => `<option value="${o.id}">${BB.shortId(o.id)} — ${BB.escapeHtml(o.description)}</option>`)
        .join('');
    } catch { /* non-fatal */ }
  }

  async function loadInvoices(srId) {
    try {
      const invoices = await api('/invoices?state=Sent');
      const relevant = invoices.filter(i => i.supply_request_id === srId);
      const sel = document.getElementById('approve-invoice-select');
      if (relevant.length === 0) {
        sel.innerHTML = '<option value="">No Sent invoices for this request</option>';
        document.getElementById('invoice-media-preview').innerHTML = '';
      } else {
        sel.innerHTML = relevant
          .map(i => `<option value="${i.id}">${BB.shortId(i.id)}${i.notes ? ` — ${BB.escapeHtml(i.notes)}` : ''}${i.has_media ? ' 📎' : ''}</option>`)
          .join('');
        // Render media for first invoice immediately.
        await renderInvoiceMedia(relevant[0].id);
      }
    } catch { /* non-fatal */ }
  }
}
