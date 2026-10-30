//! Typed integer IDs: one newtype per entity, so a node ID can never be passed as a job ID.

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! ids {
    ($($(#[$meta:meta])* $name:ident = $label:literal;)*) => {$(
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(i64);

        impl $name {
            pub const fn new(raw: i64) -> Self {
                Self(raw)
            }

            /// The raw value, as stored in SQLite and used in URLs and topics.
            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($label, "#{}"), self.0)
            }
        }
    )*};
}

ids! {
    /// An adopted node. The store assigns it and never reuses it, so history that outlives a
    /// deleted node cannot point at a newer one.
    NodeId = "node";
    /// A controller user. Assigned by the store and never reused, like [`NodeId`].
    UserId = "user";
}

#[cfg(test)]
mod tests {
    use super::NodeId;

    #[test]
    fn formats_and_serializes_as_the_raw_integer() {
        let id = NodeId::new(42);
        assert_eq!(id.to_string(), "42");
        assert_eq!(format!("{id:?}"), "node#42");
        assert_eq!(serde_json::to_string(&id).unwrap(), "42");
        assert_eq!(serde_json::from_str::<NodeId>("42").unwrap(), id);
    }
}
