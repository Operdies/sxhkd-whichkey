use sxhkd_whichkey::sxhkd::subscribe::{Event, KeyEvent, Subscriber};

use gtk::glib::MainContext;
use gtk::{gdk, glib, prelude::*, ApplicationWindow};

fn build_grid(event: &KeyEvent) -> gtk::Grid {
    fn vec_join<T>(v: Vec<T>, sep: T) -> Vec<T>
    where
        T: Clone,
    {
        let mut result = vec![];
        for item in v {
            result.push(item);
            result.push(sep.clone());
        }
        result.pop();
        result
    }
    let main_grid = gtk::Grid::builder().build();

    let triangle = "";
    let arrow = "";

    let current_hotkey = &event.keys.iter().map(|c| c.trim()).collect::<Vec<_>>();

    let current_hotkey = current_hotkey.join(&format!(" {} ", arrow));
    let current_hotkey = gtk::Label::new(Some(&current_hotkey));
    current_hotkey.set_widget_name("path");
    main_grid.attach(&current_hotkey, 0, 0, 1, 1);

    let completion_grid = gtk::Grid::default();
    completion_grid.set_widget_name("completion-grid");
    let max_completions = 20;
    let config = &event.config;
    let widest_remaining = config.iter().fold(0, |acc, elem| {
        let width = (elem.chain.len() * 2) - 1;
        if width > acc {
            width
        } else {
            acc
        }
    });

    for (row, hotkey) in config.iter().enumerate().take(max_completions) {
        let row = row as i32;
        let remaining_hotkey = hotkey
            .chain
            .iter()
            .map(|ch| ch.repr.trim())
            .collect::<Vec<_>>();

        let vj = vec_join(remaining_hotkey, arrow);
        for (i, ele) in vj.iter().enumerate() {
            let column = i as i32;
            let label = gtk::Label::new(Some(ele));
            label.set_widget_name("path");
            completion_grid.attach(&label, column, row, 1, 1);
        }
        let arrow = gtk::Label::new(Some(triangle));
        completion_grid.attach(&arrow, 1 + widest_remaining as i32, row, 1, 1);
        let cmd_label = gtk::Label::new(Some(&hotkey.description()));
        cmd_label.set_widget_name("command");
        cmd_label.set_halign(gtk::Align::Start);
        completion_grid.attach(&cmd_label, 2 + widest_remaining as i32, row, 1, 1);
    }

    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    main_grid.attach(&sep, 0, 1, 1, 1);
    main_grid.attach(&completion_grid, 0, 2, 1, 1);
    let not_shown = (config.len() as i32) - (max_completions as i32);
    if not_shown > 0 {
        let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
        main_grid.attach(&sep, 0, 3, 1, 1);
        let label = gtk::Label::new(Some(&format!("{} options not shown", not_shown)));
        label.set_widget_name("not-shown");
        main_grid.attach(&label, 0, 4, 1, 1);
    }
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
