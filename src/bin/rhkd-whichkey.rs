use std::hash::Hash;

use rhkd::parser::subscribe::{Event, KeyEvent, Subscriber};
use rhkd::parser::Hotkey;

use gtk::{
    gdk,
    glib::{self, MainContext},
    prelude::{
        ApplicationExt, ApplicationExtManual, BinExt, ContainerExt, Continue, CssProviderExt,
        GridExt, Inhibit, WidgetExt,
    },
    ApplicationWindow,
};

fn group_by<T, P, T2>(input: Vec<T>, selector: P) -> Vec<Vec<T>>
where
    T2: PartialOrd + Eq + Hash + ?Sized,
    P: Fn(&T) -> &T2,
{
    let mut result: Vec<Vec<T>> = vec![];
    for item in input.into_iter() {
        let key_1 = selector(&item);
        let pos = result.iter().position(|p| {
            let key_2 = selector(&p[0]);
            std::cmp::Ordering::Equal == key_1.partial_cmp(key_2).unwrap()
        });
        let idx = if let Some(i) = pos {
            i
        } else {
            result.push(vec![]);
            result.len() - 1
        };
        result[idx].push(item);
    }
    result
}

fn build_grid(event: &KeyEvent) -> gtk::Grid {
    let main_grid = gtk::Grid::builder().build();

    let triangle = "";
    #[allow(unused)]
    let lock = "";
    let arrow = "";

    let window_title = &event
        .config
        .iter()
        .find_map(|hk| hk.title.clone())
        .unwrap_or_else(|| {
            let join = "  ";
            let mut title = String::new();
            for (i, chord) in event.keys.iter().enumerate() {
                title.push_str(&chord.repr);
                if i != (event.keys.len() - 1) {
                    title.push_str(join);
                }
            }
            title
        });

    let current_hotkey = gtk::Label::new(Some(window_title));
    current_hotkey.set_widget_name("path");

    fn selector(hotkey: &Hotkey) -> &str {
        hotkey.chain[0].repr.trim()
    }

    let grouped = group_by(event.config.clone(), selector);
    let chunks = grouped.chunks(10);
    let n_chunks = chunks.len() as i32;

    for (column, chunk) in chunks.enumerate() {
        let column = column as i32;
        let completion_grid = gtk::Grid::default();
        completion_grid.set_widget_name("completion-grid");
        main_grid.attach(&completion_grid, column * 2, 2, 1, 1);
        if column != n_chunks - 1 {
            let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
            main_grid.attach(&sep, 1 + column * 2, 2, 1, 1);
        }

        for (row, group) in chunk.iter().enumerate() {
            let row = row as i32;

            let (keys, desc) = if group.len() == 1 {
                // There is exactly one continuation -- Show the expanded command
                let hotkey = &group[0];
                let continuation = hotkey
                    .chain
                    .iter()
                    .map(|ch| ch.repr.trim())
                    .collect::<Vec<_>>()
                    .join(&format!(" {} ", arrow));
                let command = hotkey.description();
                let continuation = gtk::Label::new(Some(&continuation));
                continuation.set_widget_name("path");
                let command = gtk::Label::new(Some(&command));
                command.set_widget_name("command");
                (continuation, command)
            } else {
                // There are multiple continuations in this chain -- show each continuation
                let continuation = group[0].chain[0].repr.trim().to_string();
                let command = group
                    .iter()
                    .map(|g| g.chain.get(1))
                    .filter(|x| x.is_some())
                    .map(|s| s.unwrap().repr.trim())
                    .collect::<Vec<_>>()
                    .join(" | ");
                let command = command.to_string();
                let continuation = gtk::Label::new(Some(&continuation));
                continuation.set_widget_name("path");
                let command = gtk::Label::new(Some(&command));
                command.set_widget_name("path");
                (continuation, command)
            };
            keys.set_halign(gtk::Align::End);
            completion_grid.attach(&keys, 0, row, 1, 1);
            let join_symbol = if group[0].chain[0].lock_chain.is_locking() {
                lock
            } else {
                triangle
            };
            let triangle = gtk::Label::new(Some(join_symbol));
            completion_grid.attach(&triangle, 1, row, 1, 1);
            desc.set_halign(gtk::Align::Start);
            completion_grid.attach(&desc, 2, row, 1, 1);
        }
    }

    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    main_grid.attach(&sep, 0, 1, n_chunks * 2 - 1, 1);
    main_grid.attach(&current_hotkey, 0, 0, n_chunks * 2 - 1, 1);
    main_grid
}

fn build_ui(application: &gtk::Application) {
    let window = ApplicationWindow::builder()
        .application(application)
        .title("sxhkd-whichkey")
        .default_width(100)
        .default_height(40)
        .width_request(1)
        .height_request(1)
        .decorated(false)
        .can_focus(false)
        .deletable(false)
        .resizable(false)
        .window_position(gtk::WindowPosition::CenterAlways)
        .accept_focus(false)
        .type_hint(gdk::WindowTypeHint::Notification)
        .deletable(false)
        .skip_taskbar_hint(true)
        .mnemonics_visible(false)
        .build();

    // Connect the 'destroy' event to terminate the application
    window.connect_delete_event(|w, _| {
        w.hide();
        Inhibit(true)
    });

    let (sender, receiver) = MainContext::channel(glib::PRIORITY_DEFAULT);
    let _ = std::thread::spawn(move || {
        for evt in Subscriber::default() {
            if sender.send(evt).is_err() {
                // Break in case of send error
                return;
            }
        }
    });

    receiver.attach(None, move |evt| {
        match evt {
            Event::KeyEvent(ref evt) => {
                let grid = build_grid(evt);
                if let Some(c) = window.child() {
                    window.remove(&c);
                }
                window.set_child(Some(&grid));
                window.show_all();
            }
            Event::ChainEnded => {
                window.hide();
            }
            Event::CommandEvent(c) => println!("{:?}", c),
            _ => (),
        };
        Continue(true)
    });
}

fn main() -> glib::ExitCode {
    if std::env::var("DISPLAY").is_err() {
        std::env::set_var("DISPLAY", ":0");
        println!("DISPLAY is not set. Trying with DISPLAY=\":0\"");
    }
    let application = gtk::Application::new(Some("sxhkd.whichkey"), Default::default());
    application.connect_activate(|app| {
        // The CSS "magic" happens here.
        let provider = gtk::CssProvider::new();
        // Load the CSS file
        let style = include_bytes!("style.css");
        provider.load_from_data(style).expect("Failed to load CSS");
        // We give the CssProvided to the default screen so the CSS rules we added
        // can be applied to our window.
        gtk::StyleContext::add_provider_for_screen(
            &gdk::Screen::default().expect("Error initializing gtk css provider."),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        build_ui(app);
    });
    let empty: Vec<&str> = vec![];
    application.run_with_args(&empty)
}
