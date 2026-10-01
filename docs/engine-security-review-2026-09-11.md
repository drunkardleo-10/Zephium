# Engine security review — September 11, 2026

The version floors are unchanged. Their maintenance review deadline is now
September 18, 2026, inclusive. No expired or older runtime was admitted by
lowering a version floor.

Apple's [security release index](https://support.apple.com/en-us/100100)
still lists macOS Tahoe 26.6.2, Sequoia 15.7.9 and Sonoma 14.8.9 as the latest
releases for those lines. The index and [Safari advisory](https://support.apple.com/en-us/148286)
list Safari 26.6.1, released August 18. Tahoe 26.6.2 was released August 17;
the existing shared August 18 anchor covers the complete OS/Safari cohort.

Microsoft's [security release notes](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-relnotes-security)
still identify Stable 152.0.4191.66 (September 4) as containing the latest
desktop security fixes, including the exploited Chromium CVE-2026-87491.
The newer September 8 entry concerns Android. The
[Microsoft Update Catalog](https://www.catalog.update.microsoft.com/Search.aspx?q=Microsoft+WebView2+Runtime)
independently lists WebView2 Runtime 152.0.4191.66 for x86, x64 and ARM64.
The Edge enterprise-products JSON was also inspected, but it contains Edge
channels rather than WebView2 and is not used as WebView2 availability evidence.

This refresh is source evidence for the existing engine floors. It is not
Windows execution, installer testing, or complete release qualification.
