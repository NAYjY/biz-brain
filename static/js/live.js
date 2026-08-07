/**
 * D06: SSE live-update wiring.
 *
 * One EventSource per page-load, one connection per Branch (T07).
 * Signal received -> re-fetch whole list for that resource type (D03).
 * Re-fetch also fires on `open` so first connect and every reconnect
 * immediately syncs current state (T07 fallback).
 */

class BranchEventSource {
  #source = null;
  #branchId;
  #handlers = {};
  #badge;

  constructor(branchId) {
    this.#branchId = branchId;
  }

  /** Register a handler for a signal kind, e.g. "OrderChanged". */
  on(kind, fn) {
    this.#handlers[kind] = fn;
    return this;
  }

  /** Element with .live-badge class to update connection state UI. */
  withBadge(el) {
    this.#badge = el;
    return this;
  }

  connect() {
    const url = `/branches/${this.#branchId}/events`;
    this.#source = new EventSource(url);

    this.#source.addEventListener('open', () => {
      this.#setConnected(true);
      // Re-fetch on connect/reconnect to close any gap (T07 resolution, D06).
      for (const fn of Object.values(this.#handlers)) fn();
    });

    this.#source.addEventListener('message', (e) => {
      let signal;
      try { signal = JSON.parse(e.data); } catch { return; }
      const handler = this.#handlers[signal.kind];
      if (handler) handler(signal);
    });

    this.#source.addEventListener('error', () => {
      this.#setConnected(false);
      // EventSource auto-reconnects; no custom backoff needed (D06 resolution).
    });

    return this;
  }

  disconnect() {
    this.#source?.close();
    this.#source = null;
  }

  #setConnected(connected) {
    if (!this.#badge) return;
    this.#badge.classList.toggle('live-badge--connected', connected);
    this.#badge.classList.toggle('live-badge--disconnected', !connected);
    const label = this.#badge.querySelector('.live-badge__label');
    if (label) label.textContent = connected ? 'Live' : 'Reconnecting…';
  }
}

window.BranchEventSource = BranchEventSource;
