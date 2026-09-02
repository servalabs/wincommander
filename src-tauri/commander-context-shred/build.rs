fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    embed_resource::compile("app.rc", embed_resource::NONE)
        .manifest_required()
        .expect("embed the context-shred asInvoker manifest");
}
