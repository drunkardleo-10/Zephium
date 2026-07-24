use thiserror::Error;

const HARD_MAX_SOURCES: usize = 32;
const HARD_MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const HARD_MAX_TOTAL_SOURCE_BYTES: usize = 32 * 1024 * 1024;
const HARD_MAX_LINE_BYTES: usize = 64 * 1024;
const HARD_MAX_PHYSICAL_LINES: usize = 500_000;
const HARD_MAX_RULES: usize = 250_000;
const HARD_MAX_WEBKIT_RULES: usize = 140_000;
const HARD_MAX_WEBKIT_JSON_BYTES: usize = 32 * 1024 * 1024;
const HARD_MAX_REQUEST_URL_BYTES: usize = 32 * 1024;
const HARD_MAX_SOURCE_URL_BYTES: usize = 32 * 1024;

/// User-configurable values used to construct validated compilation limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompileLimitValues {
    /// Maximum number of independent list sources.
    pub max_sources: usize,
    /// Maximum UTF-8 bytes in one source.
    pub max_source_bytes: usize,
    /// Maximum UTF-8 bytes across all sources.
    pub max_total_source_bytes: usize,
    /// Maximum UTF-8 bytes in one physical source line.
    pub max_line_bytes: usize,
    /// Maximum candidate rule lines across all sources.
    pub max_rules: usize,
    /// Maximum physical lines, including comments and blanks, across all
    /// sources.
    pub max_physical_lines: usize,
    /// Maximum number of emitted WebKit content-blocking rules.
    pub max_webkit_rules: usize,
    /// Maximum bytes in canonical WebKit JSON.
    pub max_webkit_json_bytes: usize,
    /// Maximum bytes in a request URL evaluated at runtime.
    pub max_request_url_bytes: usize,
    /// Maximum bytes in a request's source-document URL.
    pub max_source_url_bytes: usize,
}

impl Default for CompileLimitValues {
    fn default() -> Self {
        Self {
            max_sources: 32,
            max_source_bytes: 16 * 1024 * 1024,
            max_total_source_bytes: HARD_MAX_TOTAL_SOURCE_BYTES,
            max_line_bytes: 64 * 1024,
            max_rules: HARD_MAX_RULES,
            max_physical_lines: HARD_MAX_PHYSICAL_LINES,
            // Keep deliberate headroom below WebKit's historically common
            // 150,000-rule content-blocker ceiling.
            max_webkit_rules: 140_000,
            // Must remain at or below zephium-core's native declarative
            // transport ceiling.
            max_webkit_json_bytes: 32 * 1024 * 1024,
            // Must remain at or below zephium-core's request-policy boundary.
            max_request_url_bytes: 32 * 1024,
            max_source_url_bytes: 32 * 1024,
        }
    }
}

/// A validated set of resource limits for compilation and matching.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompileLimits(CompileLimitValues);

impl CompileLimits {
    /// Validates custom limit values.
    pub fn new(values: CompileLimitValues) -> Result<Self, LimitConfigurationError> {
        for (name, value) in [
            ("max_sources", values.max_sources),
            ("max_source_bytes", values.max_source_bytes),
            ("max_total_source_bytes", values.max_total_source_bytes),
            ("max_line_bytes", values.max_line_bytes),
            ("max_rules", values.max_rules),
            ("max_physical_lines", values.max_physical_lines),
            ("max_webkit_rules", values.max_webkit_rules),
            ("max_webkit_json_bytes", values.max_webkit_json_bytes),
            ("max_request_url_bytes", values.max_request_url_bytes),
            ("max_source_url_bytes", values.max_source_url_bytes),
        ] {
            if value == 0 {
                return Err(LimitConfigurationError::Zero { name });
            }
        }

        for (name, actual, maximum) in [
            ("max_sources", values.max_sources, HARD_MAX_SOURCES),
            (
                "max_source_bytes",
                values.max_source_bytes,
                HARD_MAX_SOURCE_BYTES,
            ),
            (
                "max_total_source_bytes",
                values.max_total_source_bytes,
                HARD_MAX_TOTAL_SOURCE_BYTES,
            ),
            ("max_line_bytes", values.max_line_bytes, HARD_MAX_LINE_BYTES),
            ("max_rules", values.max_rules, HARD_MAX_RULES),
            (
                "max_physical_lines",
                values.max_physical_lines,
                HARD_MAX_PHYSICAL_LINES,
            ),
            (
                "max_webkit_rules",
                values.max_webkit_rules,
                HARD_MAX_WEBKIT_RULES,
            ),
            (
                "max_webkit_json_bytes",
                values.max_webkit_json_bytes,
                HARD_MAX_WEBKIT_JSON_BYTES,
            ),
            (
                "max_request_url_bytes",
                values.max_request_url_bytes,
                HARD_MAX_REQUEST_URL_BYTES,
            ),
            (
                "max_source_url_bytes",
                values.max_source_url_bytes,
                HARD_MAX_SOURCE_URL_BYTES,
            ),
        ] {
            if actual > maximum {
                return Err(LimitConfigurationError::AboveHardMaximum {
                    name,
                    actual,
                    maximum,
                });
            }
        }

        if values.max_source_bytes > values.max_total_source_bytes {
            return Err(LimitConfigurationError::SourceExceedsTotal);
        }
        if values.max_line_bytes > values.max_source_bytes {
            return Err(LimitConfigurationError::LineExceedsSource);
        }

        Ok(Self(values))
    }

    /// Returns the maximum source count.
    pub const fn max_sources(self) -> usize {
        self.0.max_sources
    }

    /// Returns the per-source byte limit.
    pub const fn max_source_bytes(self) -> usize {
        self.0.max_source_bytes
    }

    /// Returns the aggregate source byte limit.
    pub const fn max_total_source_bytes(self) -> usize {
        self.0.max_total_source_bytes
    }

    /// Returns the physical-line byte limit.
    pub const fn max_line_bytes(self) -> usize {
        self.0.max_line_bytes
    }

    /// Returns the candidate-rule count limit.
    pub const fn max_rules(self) -> usize {
        self.0.max_rules
    }

    /// Returns the aggregate physical-line limit.
    pub const fn max_physical_lines(self) -> usize {
        self.0.max_physical_lines
    }

    /// Returns the emitted WebKit-rule count limit.
    pub const fn max_webkit_rules(self) -> usize {
        self.0.max_webkit_rules
    }

    /// Returns the canonical WebKit JSON byte limit.
    pub const fn max_webkit_json_bytes(self) -> usize {
        self.0.max_webkit_json_bytes
    }

    /// Returns the request URL byte limit.
    pub const fn max_request_url_bytes(self) -> usize {
        self.0.max_request_url_bytes
    }

    /// Returns the source-document URL byte limit.
    pub const fn max_source_url_bytes(self) -> usize {
        self.0.max_source_url_bytes
    }
}

impl Default for CompileLimits {
    fn default() -> Self {
        Self::new(CompileLimitValues::default()).expect("default blocker limits are valid")
    }
}

/// A malformed resource-limit configuration.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LimitConfigurationError {
    /// A limit which must be positive was configured as zero.
    #[error("{name} must be nonzero")]
    Zero {
        /// Name of the invalid limit.
        name: &'static str,
    },
    /// A caller attempted to weaken an audited process-wide resource ceiling.
    #[error("{name} is {actual}; hard maximum is {maximum}")]
    AboveHardMaximum {
        /// Name of the invalid limit.
        name: &'static str,
        /// Requested value.
        actual: usize,
        /// Audited process-wide ceiling.
        maximum: usize,
    },
    /// One source was allowed to exceed the aggregate input limit.
    #[error("max_source_bytes must not exceed max_total_source_bytes")]
    SourceExceedsTotal,
    /// One physical line was allowed to exceed its containing source.
    #[error("max_line_bytes must not exceed max_source_bytes")]
    LineExceedsSource,
}
