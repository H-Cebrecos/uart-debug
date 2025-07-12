#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{Color32, ComboBox, EventFilter};
use eframe::{App, egui};
use rhai::Engine;
use serialport::{Parity, SerialPort, SerialPortInfo, StopBits};
use std::fmt::Debug;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{io::Read, thread};

#[derive(Debug, PartialEq)]
enum Mode {
    Terminal,
    Debug,
}

#[derive(Default)]
struct Window {
    id: usize,
    name: String,
    text: String,
}

enum WndOp {
    New(usize, String),
    WriteText(usize, String),
    Close(usize),
}

struct UartApp {
    mode: Mode,
    ports: Vec<SerialPortInfo>,
    selected_port: Option<usize>,
    baud_rate: u32,
    parity: Parity,
    stop_bits: StopBits,
    connected: bool,
    tx_buffer: String,
    rx_buffer: Arc<Mutex<String>>,
    port_handle: Option<Arc<Mutex<Box<dyn SerialPort>>>>,
    windows: Vec<Window>,
    window_chan: Option<Receiver<WndOp>>,
    script_ch: Option<Sender<PathBuf>>,
    //rhai_engine: Engine,
}

impl Default for UartApp {
    fn default() -> Self {
        Self {
            mode: Mode::Debug,
            ports: serialport::available_ports().unwrap_or_default(),
            selected_port: None,
            baud_rate: 115_200,
            parity: Parity::None,
            stop_bits: StopBits::One,
            connected: false,
            tx_buffer: String::new(),
            rx_buffer: Arc::new(Mutex::new(String::new())),
            port_handle: None,
            script_ch: None,
            windows: Vec::new(),
            window_chan: None,
            //rhai_engine: Engine::new(),
        }
    }
}
//Todo scripting rhai, midi script,
impl App for UartApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if let Some(ch) = &self.window_chan {
            let rslt = ch.try_recv();
            match rslt {
                Ok(WndOp::New(id, name)) => {
                    let wnd = Window {
                        id: id,
                        name: name,
                        text: String::from("hello"),
                    };
                    self.windows.push(wnd);
                    println!("new window");
                }
                Ok(WndOp::WriteText(id, text)) => {
                    if let Some(found) = self.windows.iter_mut().find(|wnd| wnd.id == id) {
                        found.text += &text;
                    };
                }
                _ => (),
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // First row with selection buttons (Port, Baud rate, Parity, Stop Bits)
            ui.horizontal(|ui| {
                ui.label("Port:");
                let port_names: Vec<String> =
                    self.ports.iter().map(|p| p.port_name.clone()).collect();
                ComboBox::from_id_salt("port_select")
                    .selected_text(
                        self.selected_port
                            .and_then(|i| port_names.get(i))
                            .unwrap_or(&"<Select>".to_string())
                            .clone(),
                    )
                    .show_ui(ui, |ui| {
                        for (i, name) in port_names.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_port, Some(i), name);
                        }
                    });

                ui.label("Baud:");
                ui.add(
                    egui::DragValue::new(&mut self.baud_rate)
                        .speed(100)
                        .range(1_200..=921_600),
                );

                ui.label("Parity:");
                ComboBox::from_id_salt("parity_select")
                    .selected_text(format!("{:?}", self.parity))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.parity, Parity::None, "None");
                        ui.selectable_value(&mut self.parity, Parity::Even, "Even");
                        ui.selectable_value(&mut self.parity, Parity::Odd, "Odd");
                    });

                ui.label("Stop Bits:");
                ComboBox::from_id_salt("stopbit_select")
                    .selected_text(format!("{:?}", self.stop_bits))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.stop_bits, StopBits::One, "1");
                        ui.selectable_value(&mut self.stop_bits, StopBits::Two, "2");
                    });

                if !self.connected {
                    if ui.button("Connect").clicked() {
                        if let Some(index) = self.selected_port {
                            let port_name = &self.ports[index].port_name;
                            match serialport::new(port_name, self.baud_rate)
                                .parity(self.parity)
                                .stop_bits(self.stop_bits)
                                .timeout(Duration::from_millis(100))
                                .open()
                            {
                                Ok(p) => {
                                    let arc_port = Arc::new(Mutex::new(p));
                                    let rx_buffer = Arc::clone(&self.rx_buffer);
                                    let port_clone = Arc::clone(&arc_port);
                                    thread::spawn(move || {
                                        let mut buf = [0u8; 128];
                                        loop {
                                            let mut port = port_clone.lock().unwrap();
                                            match port.read(&mut buf) {
                                                Ok(n) if n > 0 => {
                                                    let mut out = rx_buffer.lock().unwrap();
                                                    out.push_str(&String::from_utf8_lossy(
                                                        &buf[..n],
                                                    ));
                                                }
                                                Ok(_) => {
                                                    // No data, avoid hogging CPU
                                                    drop(port);
                                                    thread::sleep(Duration::from_millis(10));
                                                }
                                                Err(ref e)
                                                    if e.kind() == std::io::ErrorKind::TimedOut =>
                                                {
                                                    // Timeout — expected
                                                    drop(port);
                                                    thread::sleep(Duration::from_millis(10));
                                                }
                                                Err(_) => {
                                                    // Other errors — optional: break or log
                                                    break;
                                                }
                                            }
                                        }
                                    });
                                    self.port_handle = Some(arc_port);
                                    self.connected = true;
                                }
                                Err(e) => {
                                    eprintln!("Failed to open port: {}", e);
                                }
                            }
                        }
                    }
                } else {
                    if ui.button("Disconnect").clicked() {
                        self.connected = false;
                        self.port_handle = None;
                        
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Mode:");
                ComboBox::from_id_salt("mode_select")
                    .selected_text(format!("{:?}", self.mode))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.mode, Mode::Terminal, "Terminal");
                        ui.selectable_value(&mut self.mode, Mode::Debug, "Debug");
                    });
                ui.label("Operations");
                let _ = ComboBox::from_id_salt("op_sel").selected_text("ops");

                if ui.button("load script").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        println!("Selected file: {:?}", path);
                        // You can now use `path` (a `std::path::PathBuf`)
                        //let script= read_to_string(path).unwrap();
                        if let Some(ch) = &self.script_ch {
                            let _ = ch.send(path);
                        }
                    }
                }
                if ui.button("program device").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        if let Ok(file) = File::open(&path) {
                            let mut reader = BufReader::new(file);
                            let mut buffer = [0u8; 512];
                
                            loop {
                                match reader.read_exact(&mut buffer) {
                                    Ok(()) => {
                                        self.send_to_uart(&buffer);
                                        thread::sleep(Duration::from_millis(10)); // Wait between blocks
                                    }
                                    Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                                        // Partial final block is ignored; optional: pad & send
                                        break;
                                    }
                                    Err(e) => {
                                        eprintln!("Error reading UF2 file: {}", e);
                                        break;
                                    }
                                }
                            }
                        } else {
                            eprintln!("Failed to open UF2 file.");
                        }
                    }
                }
            });
            match self.mode {
                Mode::Debug => {
                    // Send section (Send field and Send button)
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.tx_buffer);
                        if ui.button("Send").clicked() {
                            self.send_to_uart(self.tx_buffer.as_bytes());
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        // Clear button (Placed at the bottom, minimal space)
                        if ui.button("Clear").clicked() {
                            let mut rx = self.rx_buffer.lock().unwrap();
                            rx.clear();
                        }
                    });

                    // Received section (ASCII and Hex views)
                    ui.add_sized(ui.available_size(), |ui: &mut egui::Ui| {
                        egui::Frame::default()
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        // ASCII view (Left side)
                                        egui::ScrollArea::vertical()
                                            //.max_height(f32::INFINITY)
                                            .auto_shrink(false)
                                            .max_width(ui.available_width() / 2.0)
                                            .id_salt("ascii_view")
                                            .show(ui, |ui| {
                                                let rx = self.rx_buffer.lock().unwrap();
                                                ui.monospace(rx.as_str());
                                            });
                                    });
                                    ui.separator();

                                    ui.vertical(|ui| {
                                        // Hex view (Right side)
                                        egui::ScrollArea::vertical()
                                            .auto_shrink(false)
                                            //.max_height(f32::INFINITY)
                                            .max_width(ui.available_width())
                                            .id_salt("hex_view")
                                            .show(ui, |ui| {
                                                let rx = self.rx_buffer.lock().unwrap();
                                                let hex: String = rx
                                                    .as_bytes()
                                                    .chunks(8)
                                                    .map(|chunk| {
                                                        let hex_part: String = chunk
                                                            .iter()
                                                            .map(|b| format!("{:02X} ", b))
                                                            .collect();
                                                        let ascii_part: String = chunk
                                                            .iter()
                                                            .map(|b| {
                                                                if b.is_ascii_graphic() {
                                                                    *b as char
                                                                } else {
                                                                    '.'
                                                                }
                                                            })
                                                            .collect();
                                                        format!(
                                                            "{:<24}  {}\n",
                                                            hex_part, ascii_part
                                                        )
                                                    })
                                                    .collect();
                                                ui.monospace(hex);
                                            });
                                    });
                                });
                            })
                            .response
                    });
                }
                Mode::Terminal => {
                    let rx = self.rx_buffer.lock().unwrap();
                    let mut rx_clone = rx.clone(); // TextEdit needs a mutable String
                    let id = ui.make_persistent_id("term");
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.add(
                                //todo: change to normal text so you can select, carefull with id, also change colors or something.
                                egui::TextEdit::multiline(&mut rx_clone)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text("Terminal output")
                                    .desired_rows(20)
                                    .desired_width(f32::INFINITY)
                                    .cursor_at_end(true)
                                    .lock_focus(true)
                                    .id(id)
                                    .code_editor()
                                    .interactive(false)
                                    .text_color_opt(Some(Color32::ORANGE)),
                            );
                        });
                    if !ui.ctx().memory_mut(|mem| mem.has_focus(id)) {
                        ui.ctx().memory_mut(|mem| mem.request_focus(id));
                    };
                    ui.ctx().memory_mut(|mem| {
                        mem.set_focus_lock_filter(
                            id,
                            EventFilter {
                                tab: false,
                                horizontal_arrows: false,
                                vertical_arrows: false,
                                escape: false,
                            },
                        )
                    });
                    ui.input(|i| {
                        for event in &i.events {
                            match event {
                                egui::Event::Text(text) => {
                                    // Send printable characters
                                    self.send_to_uart(text.as_bytes());
                                }
                                egui::Event::Key {
                                    key: egui::Key::Tab,
                                    pressed: true,
                                    ..
                                } => {
                                    // Send Tab explicitly
                                    self.send_to_uart(&[b'\t']);
                                }
                                egui::Event::Key {
                                    key: egui::Key::Enter,
                                    pressed: true,
                                    ..
                                } => {
                                    self.send_to_uart(&[b'\r', b'\n']);
                                }
                                egui::Event::Paste(text) => {
                                    self.send_to_uart(text.as_bytes());
                                }
                                _ => {}
                            }
                        }
                    });
                }
            }
        });

        if !self.windows.is_empty() {
            for wnd in &self.windows {
                egui::Window::new(&wnd.name).show(ctx, |ui| {
                    ui.monospace(&wnd.text);
                });
            }
        }

        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

impl UartApp {
    fn new(tx: Sender<PathBuf>, wnd_rx: Receiver<WndOp>) -> Self {
        let mut new = UartApp::default();
        new.script_ch = Some(tx);
        new.window_chan = Some(wnd_rx);

        new
    }

    fn send_to_uart(&self, data: &[u8]) {
        if let Some(ref port) = self.port_handle {
            let port = Arc::clone(port);
            let data = data.to_vec();
            std::thread::spawn(move || {
                let mut port = port.lock().unwrap();
                let _ = port.write_all(&data);
            });
        }
    }
}

use std::sync::atomic::{AtomicUsize, Ordering};
fn main() -> eframe::Result<()> {
//todo: get the uart receive stuff outside of the graphics logic and treat it the same as a script. with is receive buffer copy and the send channel. you have a chatgpt started with the way to handle the buffer copies.
    let options = eframe::NativeOptions::default();
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let (wnd_tx, wnd_rx) = mpsc::channel::<WndOp>();
    let app = UartApp::new(tx, wnd_rx);
    let next_id = Arc::new(AtomicUsize::new(0)); // <- unique ID generator
    let clone_tx = move || wnd_tx.clone();

    thread::spawn(move || {
        let next_id = Arc::clone(&next_id);
        while let Ok(script) = rx.recv() {
            let tx = clone_tx();
            let tx1 = clone_tx();
            let tx2 = clone_tx();
            let next_id = Arc::clone(&next_id);
            thread::spawn(move || {
                let mut engine = Engine::new();

                engine.register_fn("new_window", move |name: String| -> usize {
                    //create_wnd(name);
                    let id = next_id.fetch_add(1, Ordering::Relaxed);
                    let _ = tx.send(WndOp::New(id, name));
                    id
                });
                engine.register_fn("write_wnd", move |id: usize, text: String| {
                    //create_wnd(name);
                    let _ = tx1.send(WndOp::WriteText(id, text));
                });
                if let Err(e) = engine.run_file(script) {
                    eprintln!("Rhai Error: {}", e);
                }
            });
        }
    });

    eframe::run_native(
        "UART Debug Tool",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
