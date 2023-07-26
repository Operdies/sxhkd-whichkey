use sxhkd_whichkey::sxhkd::subscribe::{Event, KeyEvent, Subscriber};

fn main() -> std::io::Result<()> {
    Subscriber::default().register(|evt| match evt {
        Event::KeyEvent(KeyEvent { keys, config }) if !keys.is_empty() => {
            let arrow = " ➜ ";
            let path = keys
                .iter()
                .map(|c| c.trim())
                .collect::<Vec<_>>()
                .join(arrow);
            let mut fmt = format!("Current Chain: {}\n", path);
            for hk in config {
                let path = hk
                    .chain
                    .iter()
                    .map(|ch| ch.repr.as_ref())
                    .collect::<Vec<_>>()
                    .join(arrow);
                fmt = format!("{}\n{}: {}", fmt, path, hk.command);
            }
            println!("{}", fmt);
            false
        }
        _ => false,
    });
    Ok(())
}
