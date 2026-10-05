use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// (cargo feature env suffix, DSDL namespace directory). The order also decides
/// the order namespaces are passed to the generator; `uavcan` (the base every
/// other namespace references) comes first.
const NAMESPACES: &[(&str, &str)] = &[
    ("UAVCAN", "uavcan"),
    ("DRONECAN", "dronecan"),
    ("ARDUPILOT", "ardupilot"),
    ("COM", "com"),
    ("CUAV", "cuav"),
    ("MPPT", "mppt"),
];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.parent().unwrap().to_path_buf();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let dsdl_root = manifest_dir.join("DSDL");
    let dsdlc = manifest_dir.join("dronecan_dsdlc/dronecan_dsdlc.py");
    let vendored = env::var_os("CARGO_FEATURE_VENDORED").is_some();

    // Namespaces selected via cargo features (dependencies already pulled the
    // required base namespaces in — see Cargo.toml [features]).
    let namespaces: Vec<PathBuf> = NAMESPACES
        .iter()
        .filter(|(feat, _)| env::var_os(format!("CARGO_FEATURE_{feat}")).is_some())
        .map(|(_, dir)| dsdl_root.join(dir))
        .collect();

    let generated = out_dir.join("generated");
    let gen_include = generated.join("include");
    let gen_src = generated.join("src");

    // canard.h location and the exact defines libcanard was compiled with,
    // exported by canard-sys via `links` metadata. Reusing the same defines
    // keeps the Canard struct ABI identical to canard-sys.
    let canard_include = PathBuf::from(
        env::var("DEP_CANARD_INCLUDE").expect("canard-sys did not export DEP_CANARD_INCLUDE"),
    );
    let canard_defines = parse_defines(&env::var("DEP_CANARD_DEFINES").unwrap_or_default());
    let canard_cflags =
        env::var("DEP_CANARD_CFLAGS").expect("canard-sys did not export its ABI compiler flags");

    // ---- Run the DSDL -> C generator --------------------------------------
    let _ = fs::remove_dir_all(&generated); // avoid stale files from other feature sets
    if namespaces.is_empty() {
        // No namespaces selected: nothing to generate, emit empty bindings.
        fs::create_dir_all(&out_dir).unwrap();
        fs::write(out_dir.join("bindings.rs"), "use canard_sys as _;\n").unwrap();
        return;
    }

    if vendored {
        let vendor = manifest_dir.join("vendored");
        assert!(
            vendor.join("manifest.json").exists(),
            "vendored DSDL missing; run .venv/bin/python tools/generate_dsdl.py"
        );
        fs::create_dir_all(&gen_include).unwrap();
        fs::create_dir_all(&gen_src).unwrap();
        let selected: Vec<_> = namespaces
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy())
            .collect();
        for (directory, destination) in [("include", &gen_include), ("src", &gen_src)] {
            for entry in fs::read_dir(vendor.join(directory)).expect("read vendored DSDL") {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy();
                if selected
                    .iter()
                    .any(|namespace| name.starts_with(&format!("{namespace}.")))
                {
                    fs::copy(&path, destination.join(path.file_name().unwrap()))
                        .expect("copy vendored DSDL");
                }
            }
        }
        let mut headers: Vec<_> = fs::read_dir(&gen_include)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        headers.sort();
        let aggregate = headers
            .iter()
            .map(|name| format!("#include <{name}>\n"))
            .collect::<String>();
        fs::write(gen_include.join("dronecan_msgs.h"), aggregate).unwrap();
        println!("cargo:rerun-if-changed={}", vendor.display());
    } else {
        assert!(
            dsdlc.exists(),
            "DSDL generator missing at {}; initialize Git submodules or use a complete Cargo package",
            dsdlc.display()
        );
        let python = python_interpreter(&workspace_root);
        let output = Command::new(&python)
            // Cargo registry sources may be read-only. Python imports must not
            // create __pycache__ next to the packaged generator modules.
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .arg(&dsdlc)
            .args(["-j", "2"])
            .arg("-O")
            .arg(&generated)
            .args(&namespaces)
            .output()
            .unwrap_or_else(|e| panic!("failed to run dronecan_dsdlc ({}): {e}", python.display()));
        if !output.status.success() {
            panic!(
                "dronecan_dsdlc failed ({}):\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    // ---- Compile the generated (de)serialization C ------------------------
    // CANARD_DSDLC_TEST_BUILD is intentionally left undefined so the test-only
    // helpers (`test_helpers.h`, random sample generators) are excluded.
    let mut build = cc::Build::new();
    build
        .include(&gen_include)
        .include(&canard_include)
        .warnings(false);
    // See canard-sys/build.rs: avoid libgcc's __gnu_thumb1_case_* helpers that
    // gcc emits for switch jump tables on Thumb-1 (Cortex-M0+).
    build.flag_if_supported("-fno-jump-tables");
    for flag in canard_cflags.split_whitespace() {
        build.flag(flag);
    }
    for (key, value) in &canard_defines {
        build.define(key, Some(value.as_str()));
    }
    let mut sources: Vec<_> = fs::read_dir(&gen_src)
        .expect("generator produced no src/ directory")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "c"))
        .collect();
    sources.sort();
    assert!(!sources.is_empty(), "generator produced no .c files");
    build.files(sources);
    build.compile("dronecan_dsdl"); // emits libdronecan_dsdl.a + link metadata

    // ---- Generate Rust bindings from the aggregate header -----------------
    // The Canard types/functions are declared in canard.h (pulled in by the
    // generated headers); block them here and reuse canard-sys's definitions so
    // there's a single, consistent set of Canard types across the two crates.
    let tool = cc::Build::new().get_compiler();
    let mut builder = bindgen::Builder::default()
        .header(manifest_dir.join("wrapper.h").to_string_lossy())
        .clang_arg(format!("-I{}", gen_include.display()))
        .clang_arg(format!("-I{}", canard_include.display()))
        .use_core()
        .ctypes_prefix("::core::ffi")
        // #define constants are unsigned unless a callback narrows them; this
        // keeps 64-bit signatures (high bit set) from becoming negative i64.
        .default_macro_constant_type(bindgen::MacroTypeVariation::Unsigned)
        // Emit only what the generator produced; canard.h stays in canard-sys.
        .allowlist_file(".*generated/include/.*")
        .blocklist_type("Canard.*")
        .blocklist_function("canard.*")
        .raw_line("use canard_sys::*;")
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .parse_callbacks(Box::new(DsdlCallbacks));
    for dir in cross_include_dirs(tool.path()) {
        builder = builder.clang_arg("-isystem").clang_arg(dir);
    }
    for (key, value) in &canard_defines {
        builder = builder.clang_arg(format!("-D{key}={value}"));
    }
    for flag in canard_cflags.split_whitespace() {
        builder = builder.clang_arg(flag);
    }

    let bindings = builder
        .generate()
        .expect("failed to generate DSDL bindings");
    // Give the anonymous nested structs (dynamic arrays) readable names before
    // writing the bindings out.
    let source = rename_anon_types(&bindings.to_string());
    fs::write(out_dir.join("bindings.rs"), source).expect("failed to write bindings.rs");

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed={}", dsdl_root.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("dronecan_dsdlc").display()
    );
    println!("cargo:rerun-if-env-changed=DRONECAN_DSDLC_PYTHON");
}

/// Assigns sensible integer types to the generated `#define` constants, which
/// bindgen otherwise types too widely (or as signed).
#[derive(Debug)]
struct DsdlCallbacks;

impl bindgen::callbacks::ParseCallbacks for DsdlCallbacks {
    fn int_macro(&self, name: &str, value: i64) -> Option<bindgen::callbacks::IntKind> {
        use bindgen::callbacks::IntKind;
        if name.ends_with("_SIGNATURE") {
            // 64-bit data-type signatures; the high bit is frequently set, so
            // they must be unsigned (canardBroadcast takes a u64).
            Some(IntKind::U64)
        } else if name.ends_with("_ID") {
            // Data-type IDs are passed to canard as u16.
            Some(IntKind::U16)
        } else if (0..=i64::from(u8::MAX)).contains(&value) {
            Some(IntKind::U8)
        } else if (0..=i64::from(u16::MAX)).contains(&value) {
            Some(IntKind::U16)
        } else {
            None // fall back to default_macro_constant_type (unsigned)
        }
    }
}

/// bindgen names anonymous nested structs `<Parent>__bindgen_ty_N` (DSDL dynamic
/// arrays: `struct { len; data[N]; }`). Each is used by exactly one field, so
/// rename it after that field to `<Parent>_<field>`.
fn rename_anon_types(source: &str) -> String {
    let mut renames: Vec<(String, String)> = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        // Field declarations look like `pub <field>: <Parent>__bindgen_ty_N,`,
        // but a long field name may wrap the type onto the next line.
        let Some(rest) = line.trim().strip_prefix("pub ") else {
            continue;
        };
        let (field, ty) = if let Some((f, t)) = rest.split_once(": ") {
            (f, t)
        } else if let Some(f) = rest.strip_suffix(':') {
            (f, lines.get(i + 1).map(|s| s.trim()).unwrap_or(""))
        } else {
            continue;
        };
        let ty = ty.trim_end().trim_end_matches(',');
        let field = field.trim();
        if !is_anon_type(ty) || field.is_empty() || field.contains(char::is_whitespace) {
            continue;
        }
        let idx = ty.rfind("__bindgen_ty_").unwrap();
        renames.push((ty.to_string(), format!("{}_{field}", &ty[..idx])));
    }
    // Longest first so nested anon names resolve before their prefixes.
    renames.sort_by_key(|rename| std::cmp::Reverse(rename.0.len()));
    let mut out = source.to_string();
    for (old, new) in renames {
        out = out.replace(&old, &new);
    }
    out
}

fn is_anon_type(ty: &str) -> bool {
    ty.contains("__bindgen_ty_") && ty.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Parse the comma-joined `KEY=VALUE` list exported as DEP_CANARD_DEFINES.
fn parse_defines(raw: &str) -> Vec<(String, String)> {
    raw.split(',')
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// The Python interpreter that has pydronecan/empy installed. Defaults to the
/// project's `.venv`; override with DRONECAN_DSDLC_PYTHON.
fn python_interpreter(workspace_root: &Path) -> PathBuf {
    if let Some(p) = env::var_os("DRONECAN_DSDLC_PYTHON") {
        return PathBuf::from(p);
    }
    let venv = workspace_root.join(".venv/bin/python");
    if venv.exists() {
        return venv;
    }
    panic!(
        "no Python interpreter for DSDL generation. Expected a venv at {} \
         (with dronecan + empy installed), or set DRONECAN_DSDLC_PYTHON.",
        venv.display()
    );
}

/// Ask the compiler for its `#include <...>` system header search paths, so
/// bindgen's clang (running with the arm target) can find `assert.h` etc.
fn cross_include_dirs(compiler: &Path) -> Vec<String> {
    let output = Command::new(compiler)
        .args(["-E", "-Wp,-v", "-xc", "/dev/null"])
        .output()
        .expect("failed to query compiler for system include paths");
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
