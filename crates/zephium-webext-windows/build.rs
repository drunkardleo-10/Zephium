fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var_os("CARGO_FEATURE_LAB").is_some()
    {
        // Wry imports GetWindowSubclass by name, which requires Common Controls
        // v6. The standalone lab does not inherit the desktop app's manifest.
        println!("cargo:rustc-link-arg-bin=webext-lab-windows=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bin=webext-lab-windows=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    }
}
