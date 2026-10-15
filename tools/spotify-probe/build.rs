fn main() {
    let proto = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/proto");
    println!(
        "cargo:rerun-if-changed={}",
        proto.join("collection.proto").display()
    );
    protobuf_codegen::Codegen::new()
        .pure()
        .cargo_out_dir("library_proto")
        .input(proto.join("collection.proto"))
        .include(proto)
        .run_from_script();
}
