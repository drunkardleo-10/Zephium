//! Profile-bound WebView2 native-extension ownership.
//!
//! This module deliberately exposes no page-world bridge, package selector,
//! or environment constructor. Product startup remains inert until the
//! separately reviewed environment-authority boundary is joined.

mod native;
