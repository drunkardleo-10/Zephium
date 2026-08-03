use std::error::Error;
use std::fmt;

use crate::PrivateFsError;

/// Failure from a consuming private-filesystem capability transition.
///
/// A capability is recoverable only when the operation proved that its commit
/// point was never crossed and revalidated the unchanged capability and
/// namespace boundary. Terminal failures intentionally contain no capability:
/// a mutation may have committed, settlement is unknown, and the shared lease
/// has been quarantined.
#[must_use]
pub enum PrivateFsTransitionError<S> {
    /// A clean pre-commit failure with the unchanged capability returned.
    Recoverable {
        /// The precise clean failure.
        error: PrivateFsError,
        /// The unchanged linear capability, boxed only on the error path.
        state: Box<S>,
    },
    /// A terminal failure after a possible or proven commit.
    Terminal {
        /// The terminal error, normally [`PrivateFsError::SettlementUnknown`].
        error: PrivateFsError,
    },
}

impl<S> PrivateFsTransitionError<S> {
    pub(crate) fn recoverable(error: PrivateFsError, state: S) -> Self {
        Self::Recoverable {
            error,
            state: Box::new(state),
        }
    }

    pub(crate) const fn terminal(error: PrivateFsError) -> Self {
        Self::Terminal { error }
    }

    /// Returns the filesystem error without consuming this transition result.
    #[must_use]
    pub const fn error(&self) -> PrivateFsError {
        match self {
            Self::Recoverable { error, .. } | Self::Terminal { error } => *error,
        }
    }

    /// Returns whether the unchanged capability can be recovered.
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(self, Self::Recoverable { .. })
    }

    /// Splits the failure into its error and optional unchanged capability.
    #[must_use]
    pub fn into_parts(self) -> (PrivateFsError, Option<S>) {
        match self {
            Self::Recoverable { error, state } => (error, Some(*state)),
            Self::Terminal { error } => (error, None),
        }
    }
}

impl<S> fmt::Debug for PrivateFsTransitionError<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recoverable { error, .. } => formatter
                .debug_struct("PrivateFsTransitionError::Recoverable")
                .field("error", error)
                .field("state", &"<private capability>")
                .finish(),
            Self::Terminal { error } => formatter
                .debug_struct("PrivateFsTransitionError::Terminal")
                .field("error", error)
                .finish(),
        }
    }
}

impl<S> fmt::Display for PrivateFsTransitionError<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error().fmt(formatter)
    }
}

impl<S> Error for PrivateFsTransitionError<S> {}
