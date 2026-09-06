//! Cocktail Manager control plane — v0.1 (26Q3)

fn main() {
    let mut args = std::env::args();
    let _exe = args.next();
    if args.next().as_deref() == Some("--stdin-bridge") {
        let pipe = args.next().unwrap_or_default();
        if let Err(e) = cocktail_control::run_stdin_bridge(&pipe) {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        return;
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    if let Err(e) = rt.block_on(cocktail_control::run_plane()) {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}
