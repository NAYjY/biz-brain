//! T03: cheap keyword/regex routing before invoking Claude at all. Only
//! falls through to a live classify call when no keyword matches.

use domain::DomainEventVariant;
use regex::RegexSet;

pub struct Prefilter {
    patterns: RegexSet,
    variants: Vec<DomainEventVariant>,
}

impl Prefilter {
    /// Worker-facing keyword set. (Supplier/WhatsApp has its own — see `whatsapp_worker_events`.)
    pub fn worker_events() -> Self {
        let rules: Vec<(&str, DomainEventVariant)> = vec![
            (r"(?i)\b(accept|accepted|i'?ll take it)\b", DomainEventVariant::WorkerAccepted),
            (r"(?i)\b(can'?t|cannot|unavailable|reject)\b", DomainEventVariant::WorkerUnavailable),
            (r"(?i)\b(cancel|cancelling|backing out)\b", DomainEventVariant::WorkerCancelled),
            (r"(?i)\b(question|clarify|not sure|confused)\b", DomainEventVariant::ClarificationRequested),
            (r"(?i)\b(ready|done prepping|ready for pickup)\b", DomainEventVariant::WorkerReadyForPickup),
            (r"(?i)\b(done|finished|complete)\b", DomainEventVariant::OrderDone),
        ];
        let patterns = RegexSet::new(rules.iter().map(|(p, _)| *p)).expect("static patterns are valid regex");
        let variants = rules.into_iter().map(|(_, v)| v).collect();
        Self { patterns, variants }
    }

    pub fn supplier_events() -> Self {
        let rules: Vec<(&str, DomainEventVariant)> =
            vec![(r"(?i)\b(invoice|price list|quote)\b", DomainEventVariant::InvoiceReceived)];
        let patterns = RegexSet::new(rules.iter().map(|(p, _)| *p)).expect("static patterns are valid regex");
        let variants = rules.into_iter().map(|(_, v)| v).collect();
        Self { patterns, variants }
    }

    /// First matching rule wins; `None` means fall through to a live classify call.
    pub fn classify(&self, message: &str) -> Option<DomainEventVariant> {
        self.patterns.matches(message).into_iter().next().map(|i| self.variants[i])
    }
}
