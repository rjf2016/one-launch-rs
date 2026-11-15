#![forbid(unsafe_code)]

const ICON: &str = "../../oracle/assets/icons/app.ico";

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed={ICON}");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(ICON)
        .set("ProductName", "One Launch")
        .set("FileDescription", "One Launch");
    resource
        .compile()
        .expect("the resource compiler of the Windows SDK embeds the icon");
}
