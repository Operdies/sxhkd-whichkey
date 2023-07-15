use sxhkd_whichkey::sxhkd::subscribe::Subscriber;
use sxhkd_whichkey::sxhkd::subscribe::{Event, KeyEvent};

use gtk::glib::MainContext;
use gtk::{gdk, glib, prelude::*, ApplicationWindow};

fn build_ui(application: &gtk::Application) {
    let window = ApplicationWindow::new(application);

    window.set_title("sxhkd-whichkey");
    window.set_default_size(260, 40);
    window.set_decorated(false);
    window.set_can_focus(false);
    window.set_deletable(false);
    window.set_resizable(false);
    window.set_position(gtk::WindowPosition::CenterAlways);
    window.set_title("sxhkd-whichkey");
    window.set_accept_focus(false);
    window.set_type_hint(gdk::WindowTypeHint::PopupMenu);

    // Connect the 'destroy' event to terminate the application
    window.connect_delete_event(|_, _| {
        gtk::main_quit();
        Inhibit(false)
    });

    let label = gtk::Label::new(Some("Hello World"));
    window.set_child(Some(&label));

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
            Event::KeyEvent(KeyEvent { keys, config }) => {
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
                label.set_text(&fmt);
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
    let application = gtk::Application::new(Some("sxhkd.whichkey"), Default::default());
    application.connect_activate(build_ui);
    let empty: Vec<&str> = vec![];
    application.run_with_args(&empty)

    // let config = cmd::Config::parse();
    // // Initialize GTK
    // gtk::init().expect("Failed to initialize GTK.");
    //
    // // Create a new window
    // let window = Window::new(WindowType::Popup);
    // window.set_title("sxhkd-whichkey");
    //
    // // Remove window decorators
    // window.set_decorated(false);
    // window.set_position(gtk::WindowPosition::CenterAlways);
    //
    // // Prevent window from ever taking focus
    // window.set_accept_focus(false);
    // // window.set_type_hint(gdk::WindowTypeHint::Dock);
    //
    // // Connect the 'destroy' event to terminate the application
    // window.connect_delete_event(|_, _| {
    //     gtk::main_quit();
    //     Inhibit(false)
    // });
}
