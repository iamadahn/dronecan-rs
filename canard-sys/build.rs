use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Public structs/enums verified against the target C compiler.
const CHECKED_TYPES: &[&str] = &[
    "CanardInstance",
    "CanardCANFrame",
    "CanardRxTransfer",
    "CanardTxTransfer",
    "CanardRxState",
    "CanardTxQueueItem",
    "CanardPoolAllocatorStatistics",
    "CanardTransferType",
    "CanardRequestResponse",
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let libcanard = manifest_dir.join("libcanard");
    let canard_c = libcanard.join("canard.c");
    let canard_h = libcanard.join("canard.h");

    if !canard_c.exists() {
        panic!(
            "libcanard sources not found at {}.\n\
             The git submodule is not checked out. Run:\n    \
             git submodule update --init canard-sys/libcanard",
            libcanard.display()
        );
    }

    // libcanard build-config macros derived from the selected cargo features.
    // The SAME list is fed to the C compiler and to bindgen so the compiled
    // object code and the generated Rust structs share one ABI.
    let defines = canard_defines();

    // ---- Compile canard.c for the target ----------------------------------
    // `cc` reads the TARGET env var and picks the right cross toolchain
    // (arm-none-eabi-gcc for thumbv*-none-eabi), so this is chip-agnostic.
    let mut build = cc::Build::new();
    build.file(&canard_c).include(&libcanard).warnings(false);
    // On Thumb-1 (Cortex-M0/M0+, thumbv6m) gcc lowers `switch` to compact jump
    // tables that call libgcc helpers (__gnu_thumb1_case_*). Those aren't in
    // Rust's compiler_builtins and we don't link libgcc, so disable jump tables
    // and let switches compile to plain branches instead.
    build.flag_if_supported("-fno-jump-tables");
    // ARM GCC defaults to short enums, while bindgen's raw enum aliases are
    // 32-bit integers. Fix their ABI explicitly for C and clang alike.
    build.flag_if_supported("-fno-short-enums");
    for (key, value) in &defines {
        build.define(key, Some(*value));
    }
    // Capture the fully-configured compiler (path + target flags) before the
    // compile so we can reuse it for header discovery and layout probing.
    let tool = build.get_compiler();
    build.compile("canard"); // emits libcanard.a + link metadata

    // ---- Generate Rust bindings from canard.h -----------------------------
    // bindgen is target-aware: it parses with clang set to the cargo TARGET, so
    // struct layout is computed for the actual MCU (matters e.g. for
    // CANARD_64_BIT, which keys off __WORDSIZE and changes internal index
    // widths). clang doesn't know where the arm-none-eabi libc headers live, so
    // we discover them from the cross compiler and pass them through.
    let mut builder = bindgen::Builder::default()
        .header(manifest_dir.join("wrapper.h").to_string_lossy())
        .clang_arg(format!("-I{}", libcanard.display()))
        .clang_arg("-fno-short-enums")
        .use_core()
        .ctypes_prefix("::core::ffi")
        // Restrict output to items declared in canard.h (drops libc noise).
        .allowlist_file(".*canard\\.h")
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));
    for dir in cross_include_dirs(tool.path()) {
        builder = builder.clang_arg("-isystem").clang_arg(dir);
    }
    for (key, value) in &defines {
        builder = builder.clang_arg(format!("-D{key}={value}"));
    }

    let bindings = builder
        .generate()
        .expect("failed to generate libcanard bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("failed to write bindings.rs");

    // ---- Emit compile-time layout checks ----------------------------------
    // Read size/alignment/offset constants from ELF symbol sizes. This works
    // without executing target code and is independent of diagnostic wording.
    write_layout_checks(&out_dir, &tool, &libcanard, &defines);

    // Export metadata for downstream `-sys` crates that compile C against
    // libcanard (e.g. dronecan-dsdl-sys). Because of `links = "canard"`, these
    // reach dependents as DEP_CANARD_INCLUDE / DEP_CANARD_DEFINES. Sharing the
    // exact same defines is what keeps the Canard struct ABI consistent across
    // crates.
    println!("cargo:include={}", libcanard.display());
    let defines_meta = defines
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    println!("cargo:defines={defines_meta}");
    println!("cargo:cflags=-fno-short-enums");

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed={}", canard_c.display());
    println!("cargo:rerun-if-changed={}", canard_h.display());
    println!(
        "cargo:rerun-if-changed={}",
        libcanard.join("canard_internals.h").display()
    );
}

/// Map enabled cargo features to libcanard `-D` build-config macros. Unset
/// options fall through to the header's own defaults (kept identical for the C
/// compile and the bindgen pass since both see the same `-D` set).
fn canard_defines() -> Vec<(&'static str, &'static str)> {
    // (cargo feature env suffix, macro name, value)
    const MAP: &[(&str, &str, &str)] = &[
        ("CANFD", "CANARD_ENABLE_CANFD", "1"),
        ("MULTI_IFACE", "CANARD_MULTI_IFACE", "1"),
        ("DEADLINE", "CANARD_ENABLE_DEADLINE", "1"),
        ("TAO_OPTION", "CANARD_ENABLE_TAO_OPTION", "1"),
        ("ASSERTS", "CANARD_ENABLE_ASSERTS", "1"),
        ("ALLOCATE_SEM", "CANARD_ALLOCATE_SEM", "1"),
    ];
    MAP.iter()
        .filter(|(feat, _, _)| env::var_os(format!("CARGO_FEATURE_{feat}")).is_some())
        .map(|(_, macro_name, value)| (*macro_name, *value))
        .collect()
}

/// Ask the cross compiler for its `#include <...>` system header search paths,
/// so bindgen's clang (running with the arm target) can find `assert.h` etc.
fn cross_include_dirs(compiler: &Path) -> Vec<String> {
    let output = Command::new(compiler)
        .args(["-E", "-Wp,-v", "-xc", "/dev/null"])
        .output()
        .expect("failed to query cross compiler for system include paths");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut dirs = Vec::new();
    let mut in_list = false;
    for line in stderr.lines() {
        if line.contains("#include <...> search starts here:") {
            in_list = true;
        } else if line.contains("End of search list.") {
            break;
        } else if in_list {
            let path = line.trim();
            if !path.is_empty() {
                dirs.push(path.to_string());
            }
        }
    }
    dirs
}

/// Each C array has (measurement + 1) bytes, including offset-zero fields.
/// ELF symbol sizes are supplied by the actual C target compiler. Any missing
/// measurement is a build error, never a silently skipped safety check.
fn write_layout_checks(
    out_dir: &Path,
    tool: &cc::Tool,
    libcanard: &Path,
    defines: &[(&str, &str)],
) {
    let mut checks: Vec<(String, String)> = Vec::new();
    for name in CHECKED_TYPES {
        checks.push((format!("sizeof({name})"), format!("size_of::<{name}>()")));
        checks.push((format!("_Alignof({name})"), format!("align_of::<{name}>()")));
    }
    for (name, fields) in [
        (
            "CanardInstance",
            &[
                "node_id",
                "on_reception",
                "should_accept",
                "allocator",
                "rx_states",
                "tx_queue",
                "user_reference",
            ][..],
        ),
        (
            "CanardCANFrame",
            &["id", "data", "data_len", "iface_id"][..],
        ),
        (
            "CanardRxTransfer",
            &[
                "timestamp_usec",
                "payload_head",
                "payload_middle",
                "payload_tail",
                "payload_len",
                "data_type_id",
                "transfer_type",
                "transfer_id",
                "priority",
                "source_node_id",
            ][..],
        ),
        (
            "CanardTxTransfer",
            &[
                "transfer_type",
                "data_type_signature",
                "data_type_id",
                "inout_transfer_id",
                "priority",
                "payload",
                "payload_len",
            ][..],
        ),
        ("CanardTxQueueItem", &["next", "frame"][..]),
        (
            "CanardPoolAllocatorStatistics",
            &[
                "capacity_blocks",
                "current_usage_blocks",
                "peak_usage_blocks",
            ][..],
        ),
    ] {
        for field in fields {
            checks.push((
                format!("offsetof({name}, {field})"),
                format!("offset_of!({name}, {field})"),
            ));
        }
    }
    let enabled = |key: &str| {
        defines
            .iter()
            .any(|(name, value)| *name == key && *value == "1")
    };
    let mut optional_fields = Vec::new();
    if enabled("CANARD_ENABLE_DEADLINE") {
        optional_fields.extend([
            ("CanardCANFrame", "deadline_usec"),
            ("CanardTxTransfer", "deadline_usec"),
        ]);
    }
    if enabled("CANARD_MULTI_IFACE") {
        optional_fields.extend([
            ("CanardCANFrame", "iface_mask"),
            ("CanardTxTransfer", "iface_mask"),
        ]);
    }
    if enabled("CANARD_ENABLE_CANFD") {
        optional_fields.extend([
            ("CanardCANFrame", "canfd"),
            ("CanardRxTransfer", "canfd"),
            ("CanardTxTransfer", "canfd"),
        ]);
    }
    if enabled("CANARD_ENABLE_TAO_OPTION") || enabled("CANARD_ENABLE_CANFD") {
        optional_fields.extend([
            ("CanardInstance", "tao_disabled"),
            ("CanardRxTransfer", "tao"),
            ("CanardTxTransfer", "tao"),
        ]);
    }
    for (name, field) in optional_fields {
        checks.push((
            format!("offsetof({name}, {field})"),
            format!("offset_of!({name}, {field})"),
        ));
    }
    let mut code = String::from("#include <stddef.h>\n#include \"canard.h\"\n");
    for (i, (expression, _)) in checks.iter().enumerate() {
        code.push_str(&format!(
            "unsigned char dronecan_layout_{i}[({expression}) + 1];\n"
        ));
    }
    let source = out_dir.join("layout_probe.c");
    let object_path = out_dir.join("layout_probe.o");
    std::fs::write(&source, code).expect("write C ABI probe");
    let mut command = tool.to_command();
    command
        .args(["-std=c11", "-fno-lto", "-c"])
        .arg(&source)
        .arg("-o")
        .arg(&object_path)
        .arg(format!("-I{}", libcanard.display()));
    for (key, value) in defines {
        command.arg(format!("-D{key}={value}"));
    }
    let output = command.output().expect("run target C ABI probe compiler");
    assert!(
        output.status.success(),
        "C ABI probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Ask the compiler for its binutils companion instead of adding a host
    // object parser dependency. GNU nm reports target ELF symbol sizes exactly.
    let nm_query = Command::new(tool.path())
        .arg("-print-prog-name=nm")
        .output()
        .expect("find target nm");
    assert!(nm_query.status.success(), "C compiler cannot locate nm");
    let nm = String::from_utf8(nm_query.stdout).expect("nm tool path is UTF-8");
    let output = Command::new(nm.trim())
        .args(["-S", "--defined-only"])
        .arg(&object_path)
        .output()
        .expect("run target nm for ABI checks");
    assert!(
        output.status.success(),
        "ABI symbol probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let symbols = String::from_utf8(output.stdout).expect("nm symbol output is UTF-8");
    let mut body = String::from(
        "// Generated C/Rust ABI checks. Do not edit.\nconst _: () = {\n    use core::mem::{size_of, align_of, offset_of};\n",
    );
    for (i, (c_expression, rust_expression)) in checks.iter().enumerate() {
        let symbol_name = format!("dronecan_layout_{i}");
        let fields = symbols
            .lines()
            .find_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                (fields.len() == 4 && fields[3] == symbol_name).then_some(fields)
            })
            .unwrap_or_else(|| panic!("missing C ABI measurement: {c_expression}"));
        let size = usize::from_str_radix(fields[1], 16)
            .expect("invalid ABI symbol size")
            .checked_sub(1)
            .expect("empty ABI probe symbol");
        body.push_str(&format!("    assert!({rust_expression} == {size}usize, \"canard-sys ABI mismatch: {c_expression}\");\n"));
    }
    body.push_str("};\n");
    std::fs::write(out_dir.join("layout_checks.rs"), body).expect("write Rust ABI checks");
}
