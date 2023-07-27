use std::io::Result;
use sxhkd_whichkey::sxhkd::{config::load_hotkeys, Hotkeys};

fn load_config(config: &str) -> std::io::Result<Hotkeys> {
    let config_path = std::env::temp_dir().join("temp-sxhkdrc");
    std::fs::write(&config_path, config)?;
    let cfg = load_hotkeys(config_path.to_str());
    Ok(cfg)
}

#[test]
fn test_simple_parsing() -> Result<()> {
    let cfg = "
b ; c
  other thing
super + a 
  hello world # $(1) $(0)
";
    for _ in 1..10 {
        let hotkeys = load_config(cfg)?;
        assert_eq!(hotkeys.len(), 2);

        // Test simple chain parsing
        let hk = hotkeys.first().unwrap();
        assert_eq!(hk.chain.len(), 2);
        let first = hk.chain.first().unwrap();
        let second = hk.chain.last().unwrap();
        assert_eq!(first.keysym, 'b' as u32);
        assert_eq!(second.keysym, 'c' as u32);

        // Test simple expansion
        let hk = hotkeys.last().unwrap();
        assert_eq!(hk.chain.len(), 1);
        let ch = hk.chain.first().unwrap();
        assert_eq!(ch.repr, "super + a");
        assert_eq!(ch.keysym, 'a' as u32);
        assert_eq!(hk.description(), "world hello")
    }
    Ok(())
}

#[test]
fn test_update_config() -> Result<()> {
    let cfg = "a\n something";
    let hotkeys = load_config(cfg)?;
    assert_eq!(hotkeys.len(), 1);
    let cfg = "a\n something\nb\n something else";
    let hotkeys = load_config(cfg)?;
    assert_eq!(hotkeys.len(), 2);
    let cfg = "b\n something";
    let hotkeys = load_config(cfg)?;
    assert_eq!(hotkeys.len(), 1);
    Ok(())
}

#[test]
fn test_recursive_expansion() -> Result<()> {
    let cfg = "
super + c ; {1-2} ; { c, d }
  echo {-first, -second} { -charlie, -delta } #\
        -first:'arg 1' \
        '-second':'arg 2' \
        -charlie:'Charlie says $(1)' \
        -delta:'Delta \"$(2)\"' \
        $(2)
";
    let hotkeys = load_config(cfg)?;
    assert_eq!(hotkeys.len(), 4);
    let hk = &hotkeys[0];
    assert_eq!(hk.description(), "Charlie says arg 1");
    let hk = &hotkeys[3];
    assert_eq!(hk.description(), "Delta \"Delta \"$(2)\"\"");
    for hk in hotkeys {
        assert_eq!(hk.chain.len(), 3);
    }
    Ok(())
}

#[test]
fn test_circular_expansion() -> Result<()> {
    let cfg = "
# Test forward reference
0 
    echo a b # a:'hello forward $(2)' b:'forward' $(1)
# Test backward reference
1 
    echo a b # a:'backward' b:'hello backward $(1)' $(2)
# Test order of mapping expansion
2
    echo a b # a:'hello $(2)' b:'world $(1)' $(1)
# Test order of mapping expansion 2
3
    echo a b # b:'hello $(1)' a:'world $(2)' $(1)
# Test order of mapping expansion 3
4
    echo a b dud # a:'hello $(2)' b:'world $(1)' $(1) $(2) $(3)
# Test index error expansion
5
    echo hello # $(2)
";
    let hotkeys = load_config(cfg)?;
    assert_eq!(hotkeys.len(), 6);
    let expected = vec![
        "hello forward forward",
        "hello backward backward",
        "hello world hello $(2)",
        "world hello $(1)",
        "hello world hello $(2) world hello $(2) dud",
        "$(2)",
    ];
    for (hk, expected) in hotkeys.iter().zip(expected) {
        assert_eq!(hk.description(), expected);
    }
    Ok(())
}

// It would be embarassing if the readme broke
#[test]
fn test_readme_example() -> Result<()> {
    let cfg = "
super + { space, shift + space } : {1-3}
  bspc {desktop -f, node -d} '^{1-3}' #\
  ^1:first ^2:second ^3:third \
  desktop:'Switch to $(3) workspace' \
  node:'Move node to $(3) workspace' \
  $(1)
";
    let hotkeys = load_config(cfg)?;
    assert_eq!(hotkeys.len(), 6);
    assert_eq!(hotkeys[0].description(), "Switch to first workspace");
    assert_eq!(hotkeys[1].description(), "Move node to first workspace");
    assert_eq!(hotkeys[3].description(), "Move node to second workspace");
    assert_eq!(hotkeys[4].description(), "Switch to third workspace");
    Ok(())
}
