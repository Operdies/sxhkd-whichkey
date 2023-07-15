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

    cc::Build::new()
        .define("VERSION", Some(format!("\"{}\"", version.trim()).as_ref()))
        // .define("DEBUG", "true")
        .include("./sxhkd/src/")
        .file("c/sxhkd-helper.c")
        .file("sxhkd/src/grab.c")
        .file("sxhkd/src/helpers.c")
        .file("sxhkd/src/types.c")
        .file("sxhkd/src/parse.c")
        .static_flag(true)
        .compile("sxhkd");

    // println!("cargo:rerun-if-changed=c/sxhkd-helper.h");
    // let bindings = bindgen::Builder::default()
    //     .header("c/sxhkd-helper.h")
    //     .parse_callbacks(Box::new(bindgen::CargoCallbacks))
    //     .generate()
    //     .expect("Unable to generate bindings");
    //
    // // Write the bindings to the $OUT_DIR/bindings.rs file.
    // let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    // bindings
    //     .write_to_file(out_path.join("bindings.rs"))
    //     .expect("Couldn't write bindings!");

    Ok(())
}
