fn main() {
    println!("cargo:rerun-if-env-changed=MURR_BUILD_VERSION");
    #[cfg(windows)]
    {
        let version = std::env::var("MURR_BUILD_VERSION")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "Murr Bunker Game");
        res.set("FileDescription", "Murr Bunker Game Client");
        res.set("OriginalFilename", "Murr-Bunker.exe");
        res.set("InternalName", "murr-bunker-game");
        res.set("LegalCopyright", "MIT License");
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
        if let Err(err) = res.compile() {
            println!("cargo:warning=Could not embed Windows version resource: {err}");
        }
    }
}
