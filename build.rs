//! Build script: embeds the Windows resources (application icon, version-info strings and
//! application manifest) into the executable. Nothing else happens at build time.

/// Cargo build entry point. Only runs the resource compiler when targeting Windows.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        // Icon shown in Explorer / the taskbar, plus the "Details" tab strings of the .exe.
        res.set_icon_with_id("assets/icon.ico", "1");
        res.set("FileDescription", "Hunger Overlay");
        res.set("ProductName", "Hunger Overlay");
        res.set("CompanyName", "Hunger Companion");
        res.set("LegalCopyright", "Hunger Companion (non officiel)");
        // Manifest: Common Controls v6 (modern visual styles), per-monitor DPI awareness (V2),
        // and `asInvoker` = runs with the user's normal rights, never asks for elevation.
        res.set_manifest(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="0.1.0.0" processorArchitecture="amd64" name="HungerOverlay" type="win32"/>
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
  </dependentAssembly></dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings>
    <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
    <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
  </windowsSettings></application>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges>
    <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
  </requestedPrivileges></security></trustInfo>
</assembly>"#);
        res.compile().expect("ressources Windows");
    }
    // Re-run this script only when the icon changes.
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
