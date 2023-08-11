fn main() -> anyhow::Result<()> {
    let config = rhkd::Config::default();
    rhkd::rhkd::start(config)
}
