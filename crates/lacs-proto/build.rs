fn main() {
    let mut config = prost_build::Config::new();
    config.protoc_executable(
        protoc_bin_vendored::protoc_bin_path().expect("failed to locate vendored protoc"),
    );
    config
        .compile_protos(&["proto/lacs/v1/lacs.proto"], &["proto"])
        .expect("failed to compile lacs proto");
}
