fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("failed to locate bundled protoc");
    std::env::set_var("PROTOC", protoc);

    // 编译 protobuf
    prost_build::compile_protos(&["proto/asr.proto"], &["proto/"]).unwrap();

    tauri_build::build()
}
