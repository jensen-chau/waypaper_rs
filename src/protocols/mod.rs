// This module contains Wayland protocol code generated from XML files
// using wayland-scanner macros

pub mod layer_shell {
    use wayland_client;
    
    // Import core protocol interfaces
    use wayland_client::protocol::*;
    
    // Generate interfaces from the XML protocol file
    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("../../protocol/wlr-layer-shell-unstable-v1.xml");
    }
    
    use self::__interfaces::*;
    
    // Generate client-side code from the XML protocol file
    wayland_scanner::generate_client_code!("../../protocol/wlr-layer-shell-unstable-v1.xml");
}