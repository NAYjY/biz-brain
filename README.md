# Biz·Brain

**AI-powered operations hub for small business owners** — manage orders, workers, and suppliers through LINE, WhatsApp, and Telegram, with a real-time owner dashboard.

---

## What it does

Biz·Brain sits between your owner dashboard and your field workers / suppliers. Workers accept, update, and complete jobs by chatting with a bot on LINE or Telegram. Suppliers send invoices via WhatsApp. The AI classifies every incoming message and fires the right domain event — no app installs, no training required for your team.

```
Worker (LINE/Telegram) ──▶ AI Classifier ──▶ Order state machine
                                               │
Owner Dashboard (web) ◀── SSE live updates ◀──┘
Supplier (WhatsApp)   ──▶ AI Classifier ──▶ Invoice / Supply Request flow
```

---

## Key features

**For owners**
- Real-time order board with SSE push (no page refresh)
- Assign, reassign, force-state, cancel, reset, and close orders from the dashboard
- Message workers directly from the thread modal
- Follow-up alerts (one-time, hourly, daily, every-3-day) with push to worker
- Start/due dates with overdue highlighting
- Short job names (≤20 chars) workers can reference by text
- Per-branch AI provider toggle: Claude or Gemini
- Multi-role auth: Owners see all branches; Managers see only granted branches
- Cursor-based paginated order list with state/worker/search filters

**For workers** (no app needed)
- Receive orders over LINE or Telegram
- Reply in Thai or English — the AI understands both
- `รับงาน` → accept · `ไม่ว่าง` → unavailable · `เสร็จ` → done
- Multi-order disambiguation via yes/no flow when holding multiple jobs

**For suppliers** (no app needed)
- Receive supply requests over WhatsApp
- Reply with invoice (text or image); image bytes stored and surfaced to owner
- Confirm delivery after owner approves

---

## Architecture

```
Cargo workspace
├── domain/        Pure types, state machines, domain events (no I/O)
├── store/         Postgres event log + projections (sqlx)
├── agent/         AI classify clients: Claude + Gemini; prefilter; thread context
├── messaging/     LINE / WhatsApp / Telegram adapters + webhook ingestion
├── api/           Axum REST + SSE + background workers
├── web/           SSR dashboard (Axum + inline HTML; no JS framework)
└── server/        Single binary entry point
```

**Event sourcing** — `order_events` and `supply_request_events` are append-only streams. A projection worker maintains `order_current_state` and `supply_request_current_state` read models. The dashboard reads projections; never the event stream.

**State machines** — `Order` and `SupplyRequest` enforce valid transitions at the domain level. Owner force-state commands bypass normal messaging flows for operational overrides.

**AI pipeline**
1. Cheap keyword prefilter (regex) for unambiguous patterns
2. Claude Sonnet 4.6 (or Gemini) with conversation history + active order context
3. 8 s timeout · 3 attempts · 500 ms / 1500 ms backoff
4. Retries exhausted → `ClarificationRequested` (owner is alerted)
5. Low-confidence flag surfaced on dashboard for owner review

---

## Tech stack

| Layer | Technology |
|---|---|
| Language | Rust (2021 edition) |
| Web framework | Axum 0.7 |
| Database | PostgreSQL 16 (via sqlx 0.7) |
| Migrations | sqlx migrate (30+ migrations) |
| AI | Claude Sonnet 4.6 · Gemini 3.5 Flash Lite |
| Messaging | LINE Messaging API · WhatsApp Cloud API · Telegram Bot API |
| Auth | JWT in httpOnly cookie · bcrypt passwords · token versioning |
| Frontend | Server-side rendered HTML · Vanilla JS · CSS custom properties |
| Real-time | Server-Sent Events (per-branch broadcast channel) |
| Deploy | Single binary · Railway (PORT env var) |

---

## Getting started

### Prerequisites

- Rust (stable)
- PostgreSQL 16
- Docker (optional, for local Postgres)

### 1. Start Postgres

```bash
docker compose up -d
```

### 2. Configure environment

```bash
cp .env.example .env
# Fill in: DATABASE_URL, JWT_SECRET, ANTHROPIC_API_KEY
# LINE / WhatsApp / Telegram keys needed for messaging
```

### 3. Seed the database

```bash
cargo run --bin seed -- admin@example.com password "Main Branch"
```

### 4. Run the server

```bash
cargo run --bin biz-brain-server
```

Open `http://localhost:8080` and sign in with the credentials from step 3.

---

## Environment variables

| Variable | Required | Description |
|---|---|---|
| `DATABASE_URL` | ✅ | Postgres connection string |
| `JWT_SECRET` | ✅ | HS256 signing secret (use `openssl rand -hex 32`) |
| `ANTHROPIC_API_KEY` | ✅ | Claude API key |
| `LINE_CHANNEL_SECRET` | LINE | LINE channel secret |
| `LINE_CHANNEL_ACCESS_TOKEN` | LINE | LINE channel access token |
| `WHATSAPP_VERIFY_TOKEN` | WhatsApp | Webhook verification token |
| `WHATSAPP_ACCESS_TOKEN` | WhatsApp | Graph API access token |
| `WHATSAPP_PHONE_NUMBER_ID` | WhatsApp | Phone number ID |
| `TELEGRAM_SECRET_TOKEN` | Telegram | Webhook secret header |
| `TELEGRAM_BOT_TOKEN` | Telegram | Bot token |
| `TELEGRAM_WEBHOOK_URL` | optional | Auto-registers webhook on startup |
| `GEMINI_API_KEY` | optional | Required if any branch uses Gemini |
| `PORT` | optional | Default: 8080 |
| `STATIC_DIR` | optional | Default: `web/static` |

---

## Worker onboarding

1. Add a worker in the dashboard (name only)
2. Tell the worker to send any message to your LINE bot
3. The message appears as a pending binding on the **Workers & Suppliers** page
4. Confirm the binding to link their LINE account to the worker profile
5. Assign them an order — they receive a push notification immediately

Supplier onboarding follows the same flow over WhatsApp.

---

## Database migrations

Migrations live in `store/migrations/` and run automatically on startup via `sqlx::migrate!`. The sequence covers event tables, projection tables, actor directory, webhook inbox, invoices, follow-up alerts, and multi-role auth.

---

## Localization

The dashboard supports **English** and **Thai** (ภาษาไทย). Locale is resolved from:
1. `locale` cookie (set via the language switcher)
2. `Accept-Language` header
3. Default: English

The AI classifier also understands mixed Thai/English worker messages natively.

---

## License

MIT