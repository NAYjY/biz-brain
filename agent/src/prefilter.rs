//! Cheap keyword routing before invoking the AI classifier.
//! Kept intentionally minimal: only patterns that are unambiguous
//! regardless of state or conversation context.
//! Everything else falls through to the model.

use domain::DomainEventVariant;
use regex::RegexSet;

pub struct Prefilter {
    patterns: RegexSet,
    variants: Vec<DomainEventVariant>,
}

impl Prefilter {
    /// Worker: only cancel is unambiguous enough to short-circuit.
    pub fn worker_events() -> Self {
        let rules: &[(&str, DomainEventVariant)] = &[
            (r"(?i)\b(cancel|cancelling|backing out|ยกเลิก|ขอยกเลิก)\b", DomainEventVariant::WorkerCancelled),
        ];

        let patterns = RegexSet::new(rules.iter().map(|(p, _)| *p))
            .expect("static worker prefilter patterns are valid");
        let variants = rules.iter().map(|(_, v)| *v).collect();
        Self { patterns, variants }
    }

    /// Supplier: only invoice keywords are unambiguous.
    pub fn supplier_events() -> Self {
        let rules: &[(&str, DomainEventVariant)] = &[
            (r"(?i)\b(invoice|ใบเสนอราคา|ราคา)\b", DomainEventVariant::InvoiceReceived),
        ];

        let patterns = RegexSet::new(rules.iter().map(|(p, _)| *p))
            .expect("static supplier prefilter patterns are valid");
        let variants = rules.iter().map(|(_, v)| *v).collect();
        Self { patterns, variants }
    }

    /// First matching rule wins; `None` falls through to the AI classifier.
    pub fn classify(&self, message: &str) -> Option<DomainEventVariant> {
        self.patterns
            .matches(message)
            .into_iter()
            .next()
            .map(|i| self.variants[i])
    }
}