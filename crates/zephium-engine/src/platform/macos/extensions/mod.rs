//! Side-effect-free macOS native-extension policy translation.
//!
//! Nothing in this module constructs or mutates a WebKit object. It is kept
//! separate from the feature-gated feasibility probe so representability can
//! be reviewed and tested without accidentally creating a product adapter.

mod grants;
