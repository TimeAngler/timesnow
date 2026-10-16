fn main() -> anyhow::Result<()> {
    timesnow::main_with_default(Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))))
}
