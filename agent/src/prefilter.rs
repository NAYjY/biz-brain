//! T03 / P06: cheap keyword/regex routing before invoking Claude.
//! Falls through to a live classify call when no keyword matches.
//!
//! P06: SupplierConfirmed added to supplier prefilter.
//! Thai keywords added to worker prefilter (P01).

use domain::DomainEventVariant;
use regex::RegexSet;

pub struct Prefilter {
    patterns: RegexSet,
    variants: Vec<DomainEventVariant>,
}

impl Prefilter {
    /// Worker-facing keyword set — Thai + English.
    pub fn worker_events() -> Self {
        // Each tuple: (pattern, variant). First match wins.
        let rules: &[(&str, DomainEventVariant)] = &[
            // Accepted — Thai and English
            (r"(?i)\b(accept|accepted|i'?ll take it|รับ|รับงาน|โอเค)\b", DomainEventVariant::WorkerAccepted),
            // Unavailable
            (r"(?i)\b(can'?t|cannot|unavailable|reject|ไม่ว่าง|ไม่รับ|ไม่ได้)\b", DomainEventVariant::WorkerUnavailable),
            // Cancelled
            (r"(?i)\b(cancel|cancelling|backing out|ยกเลิก|ขอยกเลิก)\b", DomainEventVariant::WorkerCancelled),
            // Clarification
            (r"(?i)\b(question|clarify|not sure|confused|ไม่เข้าใจ|สอบถาม|ถาม)\b", DomainEventVariant::ClarificationRequested),
            // Ready for pickup
            (r"(?i)\b(ready|done prepping|ready for pickup|พร้อม|พร้อมรับ)\b", DomainEventVariant::WorkerReadyForPickup),
            // Done — Thai and English
            (r"(?i)\b(done|finished|complete|เสร็จ|เสร็จแล้ว|เรียบร้อย)\b", DomainEventVariant::OrderDone),
        ];

        let patterns = RegexSet::new(rules.iter().map(|(p, _)| *p))
            .expect("static worker prefilter patterns are valid");
        let variants = rules.iter().map(|(_, v)| *v).collect();
        Self { patterns, variants }
    }

    /// Supplier-facing keyword set — P06: SupplierConfirmed added.
    pub fn supplier_events() -> Self {
        let rules: &[(&str, DomainEventVariant)] = &[
            (r"(?i)\b(invoice|price list|quote|ใบเสนอราคา|ราคา)\b", DomainEventVariant::InvoiceReceived),
            // P06: supplier confirmation
            (r"(?i)\b(confirm|confirmed|ยืนยัน|ยืนยันแล้ว|โอเค|ok)\b", DomainEventVariant::SupplierConfirmed),
        ];

        let patterns = RegexSet::new(rules.iter().map(|(p, _)| *p))
            .expect("static supplier prefilter patterns are valid");
        let variants = rules.iter().map(|(_, v)| *v).collect();
        Self { patterns, variants }
    }

    /// First matching rule wins; `None` falls through to a live classify call.
    pub fn classify(&self, message: &str) -> Option<DomainEventVariant> {
        self.patterns
            .matches(message)
            .into_iter()
            .next()
            .map(|i| self.variants[i])
    }
}
