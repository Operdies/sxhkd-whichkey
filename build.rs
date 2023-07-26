use std::process::Command;

fn main() -> std::io::Result<()> {
    let version = String::from_utf8(
        Command::new("git")
            .arg("describe")
            .arg("--tags")
            .current_dir("./sxhkd/")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();

    println!("cargo:rustc-link-lib=xcb");
    println!("cargo:rustc-link-lib=xcb-keysyms");
    println!("cargo:rerun-if-changed=sxhkd/src/");
    println!("cargo:rerun-if-changed=c/sxhkd-helper.c");

    let mut builder = cc::Build::new();
    if cfg!(debug_assertions) {
        // builder.define("DEBUG", "true");
    }

    builder
        .define("VERSION", Some(format!("\"{}\"", version.trim()).as_ref()))
        .include("./sxhkd/src/")
        .file("c/sxhkd-helper.c")
        .file("sxhkd/src/grab.c")
        .file("sxhkd/src/helpers.c")
        .file("sxhkd/src/types.c")
        .file("sxhkd/src/parse.c")
        .static_flag(true)
        .compile("sxhkd");

    Ok(())
}
