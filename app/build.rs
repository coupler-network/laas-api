fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure().build_server(false).compile(
        &[
            "proto/lightning.proto",
            "proto/walletrpc/walletkit.proto",
            "proto/signrpc/signer.proto",
            "proto/routerrpc/router.proto",
        ],
        &["proto"],
    )?;
    Ok(())
}
