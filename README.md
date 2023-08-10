## sxhkd-whichkey

This utility is similar in functionality to [which-key](https://github.com/folke/which-key.nvim) for neovim, but for the hotkey daemon, [sxhkd](https://github.com/baskerville/sxhkd).

sxhkd is excellent. I like grouping similar hotkeys under a common mnemonic prefix. But I have trouble remembering all my hotkeys. In neovim, I don't have this issue, thanks to which-key. This project enables which-key-like functionality for sxhkd, with minimal setup required.

When a chain is active, sxhkd-whichkey will show the continuations of the current chain in a small GUI window. When the chain is aborted, the window disappears. The chain is aborted if it times out. If you find the timeout too short, it can be configured in when launching sxhkd with the `-t` flag. The timeout cannot be configured in `sxhkd-whichkey`, which will only show chains exactly when they are active.

## Setup

An sxhkd status-fifo is required for this to work. A fifo can be created with `mkfifo <STATUS_FIFO>`. sxhkd must be started with `sxhkd -s <STATUS_FIFO>`.

That's it! By default, the description of each rule in the GUI window is the command which the hotkey will execute.
To improve sxhkd-whichkey experience, I recommend enriching your sxhkdrc with comments. If the line before a binding contains a comment, that comment will be interpreted as the description of the binding, and will show up in the UI. Binding descriptions support the same replacement mechanisms as commands, so each variant in a binding can have its own description. A description can optionally be preceeded by a title; this title will appear at the top of the sxhkd-whichkey window. If no title is specified, the active chain will be used instead. See the examples for more information.

## Running

```bash
git clone --recurse-submodules https://github.com/Operdies/sxhkd-whichkey
cd sxhkd-whichkey
cargo run -- -s <STATUS_FIFO>
```

## Examples

### Brightness

I use this hotkey for controlling brightness. 
```bash
# Adjust brightness
# {Increase,Decrease} brightness by 5%
super + b : {l,h} 
  ~/.config/sxhkd/scripts/backlight.sh {--inc 5,--dec 5}

# {Double,Halve} brightness
super + b : {j,k} 
  ~/.config/sxhkd/scripts/backlight.sh {--halve,--double}

# {32%,100%} brightness
super + b : { shift + h, shift + l }
  ~/.config/sxhkd/scripts/backlight.sh {--set 32,--set 100}
```

![brightness](./doc/screenshots/brightness.png)

