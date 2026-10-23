fn main() {
    // The app icon is compiled into the Windows executable here; rebuild it
    // whenever the icons change, not only when the configuration does.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
