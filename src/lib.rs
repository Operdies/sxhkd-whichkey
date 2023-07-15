pub mod sxhkd;

pub mod cmd {
    use clap::Parser;

    #[derive(Parser, Debug)]
    #[command(
        author = "Alex",
        version = "0.1.0",
        about = "which-key like program for sxhdk",
        long_about = "This utility is similar in functionality to the which-key plugin for nvim, but for the hotkey daemon sxhkd.

An sxhkd status-fifo is required for this to work. A fifo can be created with 'mkfifo <STATUS_FIFO>'. sxhkd must be started with 'sxhkd -s <STATUS_FIFO>'.

When a chain is started, and no commands are executed within a given timeframe, the application will show the valid continuations. When a continuation is chosen or the chain ends, the continuations will disappear."
    )]
    pub struct Config {
        #[arg(short, long)]
        pub status_fifo: String,
        #[arg(short, long)]
        pub config_path: Option<String>,
    }

    impl Default for Config {
        fn default() -> Self {
            Config::parse()
        }
    }
}
