use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use ulid::Ulid;

macro_rules! durable_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[cfg_attr(feature = "ipc-types", derive(specta::Type))]
        #[cfg_attr(feature = "ipc-types", specta(type = String))]
        pub struct $name(Ulid);

        impl $name {
            /// Mints a new identity at a trusted shell edge.
            pub fn generate() -> Self {
                Self(Ulid::new())
            }

            /// Parses one canonical ULID and rejects every other spelling.
            pub fn parse(value: &str) -> Option<Self> {
                let parsed = Ulid::from_string(value).ok()?;
                (parsed.to_string() == value).then_some(Self(parsed))
            }

            /// Returns stable big-endian identity bytes for durable joins.
            pub const fn bytes(self) -> [u8; 16] {
                self.0 .0.to_be_bytes()
            }
        }

        impl From<u128> for $name {
            fn from(value: u128) -> Self {
                Self(Ulid(value))
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($name), "([redacted])"))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value)
                    .ok_or_else(|| serde::de::Error::custom(concat!("invalid ", stringify!($name))))
            }
        }
    };
}

durable_id!(
    WorkId,
    "Durable profile-owned Work identity; never an execution capability."
);
durable_id!(
    WorkPlanId,
    "Durable plan identity, independent of its immutable revisions."
);
durable_id!(
    WorkPlanNodeId,
    "Durable responsibility in a draft; not an execution lease or a Task."
);
durable_id!(WorkQuestionId, "Durable clarification question identity.");
durable_id!(
    WorkExecutionId,
    "Durable execution identity, never a live admission."
);
durable_id!(WorkAttemptId, "Exact durable worker attempt identity.");
durable_id!(WorkArtifactId, "Immutable semantic artifact identity.");
durable_id!(
    WorkEnvironmentId,
    "Persistent working environment, independent of objective execution."
);
durable_id!(
    WorkElementId,
    "One resource representation in a Work environment."
);
durable_id!(
    WorkAreaId,
    "Named spatial group, never execution authority."
);
durable_id!(
    WorkRelationId,
    "Explicit relationship between two canvas elements."
);
durable_id!(
    WorkCommandId,
    "Idempotency correlation only, never an entity capability."
);
durable_id!(
    WorkRuntimeSessionId,
    "Store incarnation; never serialized as execution authority."
);
