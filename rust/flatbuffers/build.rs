use rustc_version::{version_meta, Channel};

fn main() {
    // Declare `nightly` as a known cfg flag so the compiler doesn't warn about
    // unexpected_cfgs when `#[cfg(nightly)]` is used in the source.
    println!("cargo::rustc-check-cfg=cfg(nightly)");

    let version_meta = version_meta().unwrap();

    // To use nightly features we declare this and then we can use
    // #[cfg(nightly)]
    // for nightly only features
    if version_meta.channel == Channel::Nightly {
        println!("cargo:rustc-cfg=nightly")
    }
}
