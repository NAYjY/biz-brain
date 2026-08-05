//! Newtype IDs per aggregate boundary (T01).
//!
//! Mirrors HouseMind's `id_type!` macro pattern. Each aggregate gets its own
//! ID type: Assignment, SupplyRequest, and Invoice are separate aggregates
//! (T01 resolution), so each gets its own id rather than reusing OrderId.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new(value: Uuid) -> Self {
                Self(value)
            }

            pub fn generate() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn into_inner(self) -> Uuid {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Uuid {
                id.0
            }
        }
    };
}

id_type!(OwnerId);
id_type!(CustomerId);
id_type!(OrderId);
id_type!(WorkerId);
id_type!(BranchId);
id_type!(SupplyRequestId);
id_type!(SupplierId);
id_type!(InvoiceId);
id_type!(AssignmentId);
id_type!(AgentId);
