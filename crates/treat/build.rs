fn main() {
    println!("cargo:rustc-cfg=track_caller");
    println!("cargo:rustc-check-cfg=cfg(track_caller)");
}
