//! POS peripheral layer for Tauri apps: ESC/POS printing over network / USB /
//! Windows spooler, cash-drawer kick, USB printer enumeration, and
//! physical/virtual keyboard handling.
//!
//! Use it either way:
//! - as a **Tauri plugin**: `app.plugin(tauri_plugin_pos_hardware::init())` and
//!   grant the `pos-hardware:default` permission;
//! - as a **library**: call [`printing::print_job`], [`keyboard::has_physical_keyboard`]
//!   etc. from your own commands and keep your app's command surface.

pub mod keyboard;
pub mod printing;
pub mod serial;
pub mod usb;
pub mod winprint;

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

mod commands {
    use crate::{keyboard, printing, serial, usb, winprint};
    use tauri::{AppHandle, Runtime, State};

    #[tauri::command]
    pub async fn print_job(target: printing::PrinterTarget, ops: Vec<printing::PrintOp>) -> Result<(), String> {
        printing::print_job(&target, &ops)
    }

    #[tauri::command]
    pub async fn open_cash_drawer(target: printing::PrinterTarget) -> Result<(), String> {
        printing::open_cash_drawer(&target)
    }

    /// Ask the printer something and return its raw reply (identity, status).
    /// Bytes in, bytes out — the ESC/POS meaning lives in the host.
    #[tauri::command]
    pub async fn query_printer(
        target: printing::PrinterTarget,
        payload: Vec<u8>,
        timeout_ms: u64,
    ) -> Result<Vec<u8>, String> {
        printing::query(&target, &payload, timeout_ms)
    }

    #[tauri::command]
    pub fn list_usb_devices() -> Result<Vec<usb::UsbDeviceInfo>, String> {
        usb::list_usb_devices()
    }

    #[tauri::command]
    pub fn list_system_printers() -> Result<Vec<winprint::SystemPrinterInfo>, String> {
        winprint::list_system_printers()
    }

    #[tauri::command]
    pub fn check_physical_keyboard() -> bool {
        keyboard::has_physical_keyboard()
    }

    #[tauri::command]
    pub fn toggle_virtual_keyboard() {
        keyboard::toggle_virtual_keyboard();
    }

    /* ------------------------------------------------ serial (Virtual COM) */

    #[tauri::command]
    pub fn list_serial_ports() -> Result<Vec<serial::SerialPortInfo>, String> {
        serial::list_serial_ports()
    }

    /// Attach a reader to `path`; framed scans arrive on `pos-hardware://serial-scan`.
    #[tauri::command]
    pub fn open_serial_scanner<R: Runtime>(
        app: AppHandle<R>,
        state: State<'_, serial::SerialState>,
        path: String,
        baud: u32,
        idle_ms: u64,
    ) -> Result<(), String> {
        serial::open_scanner(app, &state, path, baud, idle_ms)
    }

    #[tauri::command]
    pub fn close_serial_scanner(
        state: State<'_, serial::SerialState>,
        path: String,
    ) -> Result<(), String> {
        serial::close_scanner(&state, &path)
    }

    #[tauri::command]
    pub fn open_serial_scanners(state: State<'_, serial::SerialState>) -> Result<Vec<String>, String> {
        serial::open_scanners(&state)
    }
}

/// Initialize the plugin (name: `pos-hardware`).
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("pos-hardware")
        .invoke_handler(tauri::generate_handler![
            commands::print_job,
            commands::open_cash_drawer,
            commands::query_printer,
            commands::list_usb_devices,
            commands::list_system_printers,
            commands::check_physical_keyboard,
            commands::toggle_virtual_keyboard,
            commands::list_serial_ports,
            commands::open_serial_scanner,
            commands::close_serial_scanner,
            commands::open_serial_scanners,
        ])
        // Readers are per-port and long-lived, so the plugin owns the registry
        // rather than each caller keeping its own and racing for the port.
        .setup(|app, _api| {
            app.manage(serial::SerialState::default());
            Ok(())
        })
        .build()
}
