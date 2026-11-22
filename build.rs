fn main() {
    println!("cargo:rerun-if-changed=assets/camo-translator.ico");
    println!("cargo:rerun-if-changed=assets/camo-translator.rc");
    embed_resource::compile_for(
        "assets/camo-translator.rc",
        ["camo-translator"],
        embed_resource::NONE,
    )
    .manifest_required()
    .expect("could not embed the Camo Translator icon");
}
