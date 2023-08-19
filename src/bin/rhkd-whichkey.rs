use std::hash::Hash;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;

use rhkd::parser::config::load_config;
use rhkd::parser::{self, Chord, Hotkey};
use rhkd::CliArguments;

use gtk::{
    gdk,
    glib::{self, MainContext},
    prelude::{
        ApplicationExt, ApplicationExtManual, BinExt, ContainerExt, CssProviderExt, GridExt,
        WidgetExt,
    },
    ApplicationWindow,
};
use rhkd::rhkc::ipc::{self, get_socket_path, IpcCommand, SubscribeCommand};
use rhkd::rhkd::hotkey_handler::HotkeyHandler;

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
            let mut title = String::new();
            let locking_source = &event.config[0];
            for (i, chord) in event.keys.iter().enumerate() {
                title.push_str(&chord.repr);
                let join = if locking_source.chain[i].is_locking() {
                    lock
                } else {
                    arrow
                };
                title.push_str(&format!(" {} ", join));
            }
            title
        });

    let current_hotkey = gtk::Label::new(Some(window_title));
    current_hotkey.set_widget_name("path");

    let grouped = group_by(event.config.clone(), |hk: &Hotkey| {
        hk.chain[event.current_index].repr.trim()
    });
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
                    .skip(event.current_index)
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
                let continuation = group[0].chain[event.current_index].repr.trim().to_string();
                let command = group
                    .iter()
                    .map(|g| g.chain.get(event.current_index + 1))
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
            let join_symbol = if group[0].chain[event.current_index].lock_chain.is_locking() {
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
        .title("rhkd-whichkey")
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
        glib::Propagation::Stop
    });

    let (sender, receiver) = MainContext::channel(glib::Priority::default());
    let _ = std::thread::spawn(move || {
        let args = CliArguments::default();
        let config_path = args.config_path.as_deref();
        let config = load_config(config_path).expect("Failed to load config.");
        let fifo = args.status_fifo.clone();
        let handler = rhkd::rhkd::hotkey_handler::HotkeyHandler::new(args.clone(), config);

        fn read_lines<R: Read>(
            reader: BufReader<R>,
            handler: &HotkeyHandler,
            sender: glib::Sender<Event>,
        ) {
            let hotkeys = handler.clone_hotkeys();
            for mut line in reader.lines().flatten() {
                let prefix = line.remove(0);
                let stroke = match prefix {
                    'B' => Stroke::BeginChain(line),
                    'E' => Stroke::EndChain(line),
                    'T' => Stroke::Timeout(line),
                    'H' => Stroke::Hotkey(line),
                    'C' => Stroke::Command(line),
                    'R' => Stroke::Reload,
                    x => {
                        eprintln!("Failed to parse line {}{}", x, line);
                        continue;
                    }
                };

                let err = match stroke {
                    Stroke::BeginChain(_) => sender.send(Event::ChainStarted),
                    Stroke::EndChain(_) => sender.send(Event::ChainEnded),
                    Stroke::Hotkey(ref h) => match parser::parse_chord_chain(h) {
                        Ok(chords) => {
                            let hotkeys = HotkeyHandler::find_hotkeys_for_chords(&hotkeys, &chords);
                            if hotkeys.is_empty() {
                                continue;
                            }
                            let event = Event::KeyEvent(KeyEvent {
                                config: hotkeys,
                                keys: chords.clone(),
                                current_index: chords.len(),
                            });
                            sender.send(event)
                        }
                        Err(e) => {
                            eprintln!("Failed to parse keys from {}: {}", h, e);
                            continue;
                        }
                    },
                    _ => {
                        continue;
                    }
                };
                if let Err(e) = err {
                    eprintln!("Error: {}", e);
                }
            }
        }

        loop {
            if let Some(ref fifo) = fifo {
                let f = std::fs::File::open(fifo).expect("Failed to open fifo");
                let reader = BufReader::new(f);
                println!("Fifo connected!");
                read_lines(reader, &handler, sender.clone());
            } else {
                use ipc::SubscribeEventMask;
                let mut socket =
                    UnixStream::connect(get_socket_path()).expect("Failed to connect to socket");
                let cmd: Vec<u8> = IpcCommand::Subscribe(SubscribeCommand {
                    events: vec![
                        SubscribeEventMask::Command,
                        SubscribeEventMask::Hotkey,
                        SubscribeEventMask::Chain,
                    ],
                })
                .into();
                socket
                    .write_all(&cmd)
                    .expect("Failed to communicate with socket");
                let reader = BufReader::new(socket);
                println!("Socket connected!");
                read_lines(reader, &handler, sender.clone());
            };
            std::thread::sleep(std::time::Duration::from_secs(1));
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
        glib::ControlFlow::Continue
    });
}

fn main() -> glib::ExitCode {
    if std::env::var("DISPLAY").is_err() {
        std::env::set_var("DISPLAY", ":0");
        println!("DISPLAY is not set. Trying with DISPLAY=\":0\"");
    }
    let application = gtk::Application::new(Some("rhkd.whichkey"), Default::default());
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

#[derive(Debug, Clone)]
pub enum Event {
    ChainStarted,
    ChainEnded,
    KeyEvent(KeyEvent),
}

#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub config: Vec<Hotkey>,
    pub keys: Vec<Chord>,
    pub current_index: usize,
}

enum Stroke {
    Hotkey(String),
    Command(String),
    BeginChain(String),
    EndChain(String),
    Timeout(String),
    Reload,
}
