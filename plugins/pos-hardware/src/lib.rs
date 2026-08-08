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
pub mod usb;
pub mod winprint;

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

mod commands {
    use crate::{keyboard, printing, usb, winprint};

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
        ])
        .build()
}
