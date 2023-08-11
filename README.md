# RHKD

This is a clone of [sxhkd](https://github.com/baskerville/sxhkd). It is meant to be fully compatible with SXHKD, but with a couple of bugfixes, and some additional features.

The original intent was to bring the neovim plugin [which-key](https://github.com/folke/which-key.nvim) to SXHKD. But after writing the parser I decided to also write the hotkey daemon in order to fix some small bugs I discovered related to chord chaining, and to support socket-based IPC similar to `bspc` instead of the FIFO and signal-based IPC supported by `sxhkd`.

When a chain is active, rhkd-whichkey will show the continuations of the current chain in a small GUI window. When the chain is aborted, the window disappears. The chain is aborted if it times out. If you find the timeout too short, it can be configured when launching `sxhkd` with the `-t` flag.

## rhkd

`rhkd` can be configured with a configuration file like sxhkd. If the file `~/.config/rhkd/rhkdrc` exists, it will be used as the main configuration file. Otherwise, `~/.config/sxhkd/sxhkdrc` will be used. If neither file exists, `rhkd` will still listen for configuration events on its IPC socket.

## rhkd-whichkey

An sxhkd status-fifo is required for this to work with sxhkd. A fifo can be created with `mkfifo <STATUS_FIFO>`. sxhkd must be started with `sxhkd -s <STATUS_FIFO>`. If you are using `rhkd`, the default IPC mechanism is socket-based, and does not require setup.

That's it! By default, the description of each rule in the GUI window is the command which the hotkey will execute.
To improve rhkd-whichkey experience, I recommend enriching your sxhkdrc with comments. If the line before a binding contains a comment, that comment will be interpreted as the description of the binding, and will show up in the UI. Binding descriptions support the same replacement mechanisms as commands, so each variant in a binding can have its own description. A description can optionally be preceded by a title; this title will appear at the top of the rhkd-whichkey window. If no title is specified, the active chain will be used instead. See the examples for more information.

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
