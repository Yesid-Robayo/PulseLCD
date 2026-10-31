//! Genera el icono del exe (dibujado por código) y lo incrusta junto con el manifiesto.

#[path = "src/icon.rs"]
#[allow(dead_code)]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let ico = std::path::Path::new(&out).join("app.ico");
    std::fs::write(&ico, icon::ico_file(&[16, 24, 32, 48, 64, 128, 256])).expect("icono");
    let rc = std::path::Path::new(&out).join("app.rc");
    let manifest = std::env::current_dir().expect("carpeta").join("app.manifest");
    let text = format!(
        "1 ICON \"{}\"\n1 24 \"{}\"\n",
        ico.display().to_string().replace('\\', "\\\\"),
        manifest.display().to_string().replace('\\', "\\\\")
    );
    std::fs::write(&rc, text).expect("recursos");
    embed_resource::compile(&rc, embed_resource::NONE).manifest_required().expect("recursos");
}
