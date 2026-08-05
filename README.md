# Biz-Brain — clean-slate implementation

Rebuilt from scratch against the ticket resolutions in `map.md`/`T01`-`T07`/
`R01`-`R03`, after the prior (Qwen-generated) implementation was found to
contradict several locked decisions and not compile. T04 and T07 were still
open when this build started — see "T04 and T07 resolutions" below; they
were resolved first, then implemented alongside everything else.

**Not verified with `cargo check`** — no Rust toolchain was available in the
sandbox this was built in. Review before trusting; see "Known risk areas"
below for where to look first.

## Crate map

| Crate       | Ticket(s)   | Contents |
|-------------|-------------|----------|
| `domain`    | T01         | Newtype ids, `Order`/`SupplyRequest`/`Assignment`/`Invoice` state machines, single closed `DomainEvent` enum, `Channel`/`ChannelIdentity` (T04), `SseSignal` (T07) |
| `store`     | T02, T04    | Event tables (typed columns + CHECK, sequence-based optimistic concurrency), projection tables, `orders`/`supply_requests` metadata tables, `webhook_inbox` (T04 dedup), `workers`/`suppliers` identity tables |
| `agent`     | T03         | Keyword pre-filter → Claude classify fallback, set-valued thread context, two-lane output (DomainEvent vs NL) |
| `messaging` | T04         | `ChannelAdapter` trait, LINE + WhatsApp adapters, inbound dedup-and-land flow |
| `api`       | T05, T04, T07 | REST reads, one command endpoint per Owner-triggerable `DomainEvent` variant, webhook routes, per-Branch SSE source |
| `web`       | T06, T07    | Leptos SSR shell, browser-facing SSE relay |
| `server`    | T06         | The actual binary — composes `api` + `web` into one `Router`, one process |

## T04 and T07 resolutions (made in this pass, not pre-existing tickets)

**T04** — `line` renamed to `messaging`; one crate, shared `Channel` trait,
per-channel submodules. Inbound handoff is async via a durable
`webhook_inbox` table (dedup + ack-fast). Reply tokens are used *only* for a
synchronous in-handler ack; all Agent/Owner-triggered content goes through
push. `domain` owns the `Channel`/`external_id` shape on Worker/Supplier;
`messaging`/`store` own the lookup.

**T07** — SSE payload is a bare invalidation signal (not a raw `DomainEvent`
or diff), fired off the projection worker's write so a client re-fetch is
guaranteed consistent. One connection per Branch. No `Last-Event-ID` replay
— reconnect just triggers a normal REST re-fetch. `web` re-originates its
own SSE endpoint for the browser; `api`'s stream stays same-process.

## A gap caught during the build

Order/SupplyRequest *creation* isn't in T01's `DomainEvent` enum (the Owner
creates them directly; the Agent never originates them) — so `orders` and
`supply_requests` metadata tables exist in `store` outside the event
streams. `api`'s create-order/create-supply-request commands write there
directly and seed the projection row immediately.

## Known risk areas (check these first)

- **Leptos 0.6 API surface** (`web/src/routes/dashboard.rs`) — kept
  deliberately minimal (a static shell); the exact `leptos::ssr::render_to_string`
  call signature should be checked against whatever Leptos version actually
  resolves.
- **`agent`'s `active_orders_for_worker` query** (`store/src/actors.rs`) is
  an approximation (latest worker-bearing event per Order) — a real
  `assignments` read model would be more correct; flagged as a follow-up.
- **`InvoiceReceived` detail extraction** — `SupplierAgent` recognizes the
  keyword but doesn't extract line-items/totals from the message; that's
  out of scope for T03's classification step as resolved.
- Nothing here has been compiled. Run `cargo check --workspace` first.
