use std::env::var_os;

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();

    // 编译 protobuf
    let proto_path = "../src-tauri/proto/asr.proto";
    prost_build::compile_protos(&[proto_path], &["../src-tauri/proto/"]).unwrap();

    println!("cargo:rerun-if-changed={}", proto_path);
}
