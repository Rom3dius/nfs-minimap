fn main() {
    // ESP-IDF build setup
    embuild::espidf::sysenv::output();
    
    // Compile Slint UI
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);
    
    slint_build::compile_with_config("../../ui/minimap.slint", config).unwrap();
}
