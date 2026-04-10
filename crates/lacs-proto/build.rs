fn main() {
    let mut config = prost_build::Config::new();
    config.compile_well_known_types();
    config
        .compile_protos(&["proto/lacs/v1/lacs.proto"], &["proto"])
        .expect("failed to compile lacs proto");
}
