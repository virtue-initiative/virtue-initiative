fn main() {
    // Always rerun when .env.dev is added, changed, or removed.
    println!("cargo:rerun-if-changed=.env.dev");

    dotenv_build::output(dotenv_build::Config {
        filename: std::path::Path::new(".env.dev"),
        recursive_search: false,
        fail_if_missing_dotenv: false,
        ..Default::default()
    })
    .unwrap();
}
