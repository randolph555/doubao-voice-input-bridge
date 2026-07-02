fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("failed to locate bundled protoc");
    std::env::set_var("PROTOC", protoc);

    // 编译 protobuf
    let proto_path = "../src-tauri/proto/asr.proto";
    prost_build::compile_protos(&[proto_path], &["../src-tauri/proto/"]).unwrap();

    println!("cargo:rerun-if-changed={}", proto_path);
}
