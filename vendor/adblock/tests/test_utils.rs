//! Convenience functions used for tests across different build targets. Import via `#[path = ]` if
//! needed outside of this directory.

/// Generates a deterministic, Zephium-authored network corpus without
/// embedding third-party list material or real browsing domains.
pub fn synthetic_network_rules(count: usize) -> String {
    use std::fmt::Write as _;

    let mut contents = String::with_capacity(count.saturating_mul(64));
    for index in 0..count {
        match index % 4 {
            0 => writeln!(contents, "||script-{index}.corpus.invalid^$script"),
            1 => writeln!(
                contents,
                "||image-{index}.corpus.invalid^$image,third-party"
            ),
            2 => writeln!(contents, "||metric-{index}.corpus.invalid^$xmlhttprequest"),
            _ => writeln!(contents, "||critical-{index}.corpus.invalid^$important"),
        }
        .expect("writing to a String cannot fail");
    }
    contents
}
