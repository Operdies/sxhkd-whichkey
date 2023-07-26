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
        assert_eq!("world hello", hk.description())
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
    assert_eq!(hotkeys.len(), 2 * 2);
    let hk = &hotkeys[0];
    assert_eq!(hk.description(), "Charlie says arg 1");
    let hk = &hotkeys[3];
    assert_eq!(hk.description(), "Delta \"Delta \"$(2)\"\"");
    for hk in hotkeys {
        assert_eq!(3, hk.chain.len());
        println!("{}", hk.description());
    }
    Ok(())
}
