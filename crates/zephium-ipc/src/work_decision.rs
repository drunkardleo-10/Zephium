//! Per-profile typed-decision preference. The response reports whether the
//! TypeSafe Keychain item exists, never any part of its content.
use super::*;

/// Trusted per-profile choice; page and model output cannot select it.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkDecisionChoiceV1 {
    #[default]
    Recommended,
    Standard,
    Off,
}

impl WorkDecisionChoiceV1 {
    /// Exact durable spelling; the wire and the stored value stay one vocabulary.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recommended => "recommended",
            Self::Standard => "standard",
            Self::Off => "off",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "recommended" => Some(Self::Recommended),
            "standard" => Some(Self::Standard),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    /// What actually runs: Recommended without the TypeSafe item is Standard.
    pub const fn effective(self, typesafe_key_present: bool) -> Self {
        match self {
            Self::Recommended => {
                if typesafe_key_present {
                    Self::Recommended
                } else {
                    Self::Standard
                }
            }
            Self::Standard => Self::Standard,
            Self::Off => Self::Off,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkDecisionPreferenceV1 {
    pub version: u16,
    pub profile: String,
    pub choice: WorkDecisionChoiceV1,
    pub effective: WorkDecisionChoiceV1,
    pub typesafe_key_present: bool,
    pub error: Option<WorkFailureV1>,
}

impl WorkDecisionPreferenceV1 {
    pub fn settled(profile: String, choice: WorkDecisionChoiceV1, key_present: bool) -> Self {
        Self {
            version: 1,
            profile,
            choice,
            effective: choice.effective(key_present),
            typesafe_key_present: key_present,
            error: None,
        }
    }

    /// A refused read claims nothing about the stored choice or the Keychain.
    pub fn failed(profile: String, error: WorkFailureV1) -> Self {
        Self {
            version: 1,
            profile,
            choice: WorkDecisionChoiceV1::default(),
            effective: WorkDecisionChoiceV1::Standard,
            typesafe_key_present: false,
            error: Some(error),
        }
    }
}

/// Invalidation only: read the current preference after delivery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkDecisionPreferenceChangedV1 {
    pub profile: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_round_trip_through_their_exact_durable_spelling() {
        for choice in [
            WorkDecisionChoiceV1::Recommended,
            WorkDecisionChoiceV1::Standard,
            WorkDecisionChoiceV1::Off,
        ] {
            assert_eq!(WorkDecisionChoiceV1::parse(choice.as_str()), Some(choice));
            assert_eq!(
                serde_json::to_string(&choice).expect("encode"),
                format!("\"{}\"", choice.as_str())
            );
        }
        for stored in ["", "emulation", "disabled", "Recommended", " off"] {
            assert_eq!(WorkDecisionChoiceV1::parse(stored), None);
        }
    }

    #[test]
    fn recommended_without_the_typesafe_item_reports_standard_as_effective() {
        let present =
            WorkDecisionPreferenceV1::settled("p".into(), WorkDecisionChoiceV1::Recommended, true);
        assert_eq!(present.effective, WorkDecisionChoiceV1::Recommended);
        let missing =
            WorkDecisionPreferenceV1::settled("p".into(), WorkDecisionChoiceV1::Recommended, false);
        assert_eq!(missing.choice, WorkDecisionChoiceV1::Recommended);
        assert_eq!(missing.effective, WorkDecisionChoiceV1::Standard);
        for choice in [WorkDecisionChoiceV1::Standard, WorkDecisionChoiceV1::Off] {
            for key in [false, true] {
                assert_eq!(choice.effective(key), choice);
            }
        }
    }

    #[test]
    fn a_refused_read_never_claims_a_keychain_item() {
        let failed =
            WorkDecisionPreferenceV1::failed("p".into(), WorkFailureV1::ProfileUnavailable);
        assert_eq!(failed.version, 1);
        assert!(!failed.typesafe_key_present);
        assert_eq!(failed.effective, WorkDecisionChoiceV1::Standard);
        assert_eq!(failed.error, Some(WorkFailureV1::ProfileUnavailable));
    }
}
