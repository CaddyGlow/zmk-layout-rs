use zmk_layout_rs::cli;

fn main() {
    let _ =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("")).try_init();
    match cli::run() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    }
}
