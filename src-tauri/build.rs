fn main() {
    println!("cargo:rerun-if-changed=proto/collection.proto");
    protobuf_codegen::Codegen::new()
        .pure()
        .cargo_out_dir("library_proto")
        .input("proto/collection.proto")
        .include("proto")
        .run_from_script();
    tauri_build::build()
}
