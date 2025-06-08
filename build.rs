fn main() {
    let profile = std::env::var("PROFILE").unwrap();

    if profile == "debug" {
        println!("cargo:rustc-link-search=native=/usr/lib64");
        println!("cargo:rustc-link-lib=python3.12");
    }

    pyo3_build_config::add_extension_module_link_args();
}
