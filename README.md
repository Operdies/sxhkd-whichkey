# sxhkd-whichkey

This utility is similar in functionality to the which-key plugin for nvim, but for the hotkey daemon sxhkd.

An sxhkd status-fifo is required for this to work. A fifo can be created with `mkfifo <STATUS_FIFO>`. sxhkd must be started with `sxhkd -s <STATUS_FIFO>`.

When a chain is started, and no commands are executed within a given timeframe, the application will show the valid continuations. When a continuation is chosen or the chain ends, the continuations will disappear.

# Running
```bash
git clone --recurse-submodules https://github.com/Operdies/sxhkd-whichkey
cargo run --bin gui -- -s <STATUS_FIFO
```
