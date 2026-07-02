fn main() {
    // 编译 protobuf
    prost_build::compile_protos(&["proto/asr.proto"], &["proto/"]).unwrap();

    tauri_build::build()
}
