fn main() -> anyhow::Result<()> {
    let config = rhkd::CliArguments::default();
    match rhkd::rhkd::start(config) {
        Err(e) => {
            eprintln!("rhkd stopped due to error: {}", e);
            Err(e)
        }
        _ => Ok(()),
    }
}
