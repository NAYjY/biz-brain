//! P01: Branch-scoped reply templates.  Owner can customise the text the
//! Agent sends back to Workers/Suppliers after each event type.
//! Defaults are seeded when a Branch is created.

use sqlx::PgPool;
use uuid::Uuid;

/// Default templates seeded on Branch creation.
/// Keys match DomainEventVariant::as_sql() strings.
pub const DEFAULT_TEMPLATES: &[(&str, &str)] = &[
    ("worker_accepted",         "Got it ✓ Order accepted."),
    ("worker_unavailable",      "Understood, you've been removed from this order."),
    ("worker_cancelled",        "Noted, cancellation recorded. The Owner has been alerted."),
    ("clarification_requested", "I've flagged your question to the Owner. They'll get back to you shortly."),
    ("worker_ready_for_pickup", "Great, marked as ready for pickup ✓"),
    ("order_done",              "Order closed. Well done!"),
    ("invoice_received",        "Got it ✓ Your invoice has been received."),
    ("supplier_confirmed",      "Confirmed ✓ Thank you for confirming."),
];

pub struct ReplyTemplateRepository {
    pool: PgPool,
}

impl ReplyTemplateRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Seed default templates for a newly created Branch.
    /// ON CONFLICT DO NOTHING — safe to call repeatedly.
    pub async fn seed_defaults(&self, branch_id: Uuid) -> Result<(), sqlx::Error> {
        for (event_type, template) in DEFAULT_TEMPLATES {
            sqlx::query(
                "INSERT INTO reply_templates (branch_id, event_type, template) \
                 VALUES ($1, $2, $3) ON CONFLICT (branch_id, event_type) DO NOTHING",
            )
            .bind(branch_id)
            .bind(*event_type)
            .bind(*template)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Fetch the reply text for a given event type on this Branch.
    /// Falls back to the hardcoded default if no row exists.
    pub async fn fetch(&self, branch_id: Uuid, event_type: &str) -> Result<String, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT template FROM reply_templates \
             WHERE branch_id = $1 AND event_type = $2",
        )
        .bind(branch_id)
        .bind(event_type)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((template,)) = row {
            return Ok(template);
        }

        // Static fallback so the caller always gets a string.
        let fallback = DEFAULT_TEMPLATES
            .iter()
            .find(|(k, _)| *k == event_type)
            .map(|(_, v)| *v)
            .unwrap_or("Done ✓");
        Ok(fallback.to_string())
    }
}
