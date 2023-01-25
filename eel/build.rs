fn main() {
    tonic_build::configure()
        .build_server(false)
        .compile(
            &["../submodules/lspd/rpc/lspd.proto"],
            &["../submodules/lspd/rpc"],
        )
        .unwrap();
}
