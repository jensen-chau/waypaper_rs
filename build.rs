fn main() {
    println!("cargo:rerun-if-changed=protocol/wayland.xml");
    println!("cargo:rerun-if-changed=protocol/wlr-layer-shell-unstable-v1.xml");
    
    // Add PKG_CONFIG_PATH for ffmpeg if not already set
    if std::env::var("PKG_CONFIG_PATH").is_err() {
        println!("cargo:rustc-env=PKG_CONFIG_PATH=/usr/lib/ffmpeg4.4/pkgconfig");
    }
}