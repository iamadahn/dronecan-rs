use std::env;

fn main() {
    // Read the actual native configuration after Cargo feature unification.
    // Looking only at this crate's features would miss flags enabled elsewhere.
    let defines = env::var("DEP_CANARD_DEFINES").expect("missing libcanard configuration");
    for (macro_name, cfg) in [
        ("CANARD_ENABLE_CANFD", "canard_canfd"),
        ("CANARD_ENABLE_DEADLINE", "canard_deadline"),
        ("CANARD_MULTI_IFACE", "canard_multi_iface"),
        ("CANARD_ENABLE_TAO_OPTION", "canard_tao"),
    ] {
        println!("cargo:rustc-check-cfg=cfg({cfg})");
        let enabled = defines.split(',').any(|d| d == format!("{macro_name}=1"))
            || (cfg == "canard_tao" && defines.split(',').any(|d| d == "CANARD_ENABLE_CANFD=1"));
        if enabled {
            println!("cargo:rustc-cfg={cfg}");
        }
    }
    println!("cargo:rerun-if-env-changed=DEP_CANARD_DEFINES");
}
