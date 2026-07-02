//! Persistent entities use ULIDs (sortable, globally unique, sync-safe).
//! Runtime-only entities (windows) use local u64.

use std::fmt;

use ulid::Ulid;

macro_rules! ulid_id {
    ($name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Ulid);

        impl $name {
            /// Minted at the shell edge; aggregates receive ids, never create them.
            pub fn generate() -> Self {
                Self(Ulid::new())
            }

            pub fn parse(s: &str) -> Option<Self> {
                Ulid::from_string(s).ok().map(Self)
            }

            pub fn bytes(self) -> [u8; 16] {
                self.0 .0.to_be_bytes()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }

        impl From<u128> for $name {
            fn from(n: u128) -> Self {
                Self(Ulid(n))
            }
        }
    };
}

ulid_id!(ItemId);
ulid_id!(SpaceId);
ulid_id!(ProfileId);

pub type WindowId = u64;
