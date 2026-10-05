fn main() {
    println!("cargo:rerun-if-changed=memory.x");
    // Required for embassy-stm32
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
}
