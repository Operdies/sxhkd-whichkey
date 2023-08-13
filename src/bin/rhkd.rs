fn main() -> anyhow::Result<()> {
    let config = rhkd::CliArguments::default();
    rhkd::rhkd::start(config)
}
