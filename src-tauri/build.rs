fn main() {
    tauri_build::build();

    // icon.ico (referenced via tauri.conf.json's bundle.icon) is embedded
    // into the exe's PE resource table by tauri-build's bundled tauri-winres
    // pass. No extra winres gluing needed here; doing so causes a duplicate
    // VERSIONINFO resource at link time.
}
