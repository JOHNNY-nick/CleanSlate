//! Generates `app/CleanSlate.Interop/NativeMethods.g.cs` from `src/lib.rs`.
//!
//! The file is versioned in Git; it is only rewritten when its content
//! changes, so .NET does not rebuild for nothing.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let target = manifest_dir.join("../../app/CleanSlate.Interop/NativeMethods.g.cs");
    let generated = PathBuf::from(env::var("OUT_DIR")?).join("NativeMethods.g.cs");

    csbindgen::Builder::default()
        .input_extern_file("src/lib.rs")
        .csharp_dll_name("cleanslate")
        .csharp_namespace("CleanSlate.Interop")
        .csharp_class_name("NativeMethods")
        .csharp_class_accessibility("internal")
        .csharp_use_function_pointer(true)
        .generate_csharp_file(&generated)?;

    let new_content = fs::read_to_string(&generated)?;
    let old_content = fs::read_to_string(&target).unwrap_or_default();
    if new_content != old_content {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, new_content)?;
    }
    Ok(())
}
