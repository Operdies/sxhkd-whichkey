#[macro_use]
extern crate lazy_static;

pub mod keyboard;
pub mod parser;
pub mod rhkc;
pub mod rhkd;

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
pub struct CliArguments {
    /// Name of the keysym used for aborting chord chains.
    #[arg(short = 'a', long = "abort-keysym", default_value = Some("Escape"))]
    pub abort_keysym: Option<String>,
    /// Redirect the commands output to the given file.
    #[arg(short = 'r', long = "redir-file")]
    pub redir_file: Option<String>,
    /// Timeout in seconds for the recording of chord chains.
    #[arg(short = 't', long = "timeout", default_value_t = 3)]
    pub timeout: u32,
    /// Handle the first COUNT mapping notify events
    #[arg(short = 'm', long = "count")]
    pub count: Option<usize>,
    /// Output status information to the given FIFO. This is supported to maintain
    /// compatibility with sxhkd. Using IPC sockets with rhkc is preferred.
    #[arg(short = 's', long = "status-fifo")]
    pub status_fifo: Option<String>,
    /// Read the main configuration from the given file. It is also possible to configure rhkd
    /// with a bash script by using rhkc.
    #[arg(short = 'c', long = "config-path")]
    pub config_path: Option<String>,
}

impl Default for CliArguments {
    fn default() -> Self {
        CliArguments::parse()
    }
}
