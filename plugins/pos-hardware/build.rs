// Every command in the invoke handler must appear here, or Tauri generates no
// permission for it and the call is rejected at runtime with nothing in the
// handler to explain why. `query_printer` was registered without one until
// 2026-08-11 — invisible for as long as it was, because the only host using it
// had defined its own copy of that command rather than reaching for this one.
const COMMANDS: &[&str] = &[
    "print_job",
    "open_cash_drawer",
    "query_printer",
    "list_usb_devices",
    "list_system_printers",
    "check_physical_keyboard",
    "toggle_virtual_keyboard",
    "list_serial_ports",
    "open_serial_scanner",
    "close_serial_scanner",
    "open_serial_scanners",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
