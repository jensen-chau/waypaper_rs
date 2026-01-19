fn main() {
    println!("cargo:rerun-if-changed=protocol/wayland.xml");
    println!("cargo:rerun-if-changed=protocol/wlr-layer-shell-unstable-v1.xml");
}