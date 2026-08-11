//! ESC/POS printing over the three transports POS hardware actually uses:
//! raw TCP (":9100" network printers), USB (libusb), and the Windows print
//! spooler (buffered bytes handed to winspool). Extracted from medusa-pos.
//!
//! The API is deliberately dumb: a target address and a list of ops. Receipt
//! layout, logos, and templating belong to the host application.

use std::time::Duration;

use escpos::driver::{Driver, NetworkDriver, UsbDriver};
use escpos::errors::PrinterError;
use escpos::printer::Printer;
use escpos::printer_options::PrinterOptions;
use escpos::utils::{
    BitImageOption, BitImageSize, CashDrawer, JustifyMode, Protocol, QRCodeCorrectionLevel,
    QRCodeModel, QRCodeOption,
};
use serde::{Deserialize, Serialize};

use crate::winprint;

/// Where the bytes go.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PrinterTarget {
    /// Raw TCP, default port 9100.
    #[serde(rename_all = "camelCase")]
    Network { host: String, #[serde(default = "default_port")] port: u16 },
    /// Direct USB via libusb.
    #[serde(rename_all = "camelCase")]
    Usb { vendor_id: u16, product_id: u16 },
    /// A printer known to the OS spooler (Windows; raw bytes).
    #[serde(rename_all = "camelCase")]
    System { name: String },
}

fn default_port() -> u16 {
    9100
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Left,
    Center,
    Right,
}

/// QR error correction level. Higher levels survive smudged thermal paper at
/// the cost of capacity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum QrEcc {
    L,
    M,
    Q,
    H,
}

impl From<QrEcc> for QRCodeCorrectionLevel {
    fn from(value: QrEcc) -> Self {
        match value {
            QrEcc::L => QRCodeCorrectionLevel::L,
            QrEcc::M => QRCodeCorrectionLevel::M,
            QrEcc::Q => QRCodeCorrectionLevel::Q,
            QrEcc::H => QRCodeCorrectionLevel::H,
        }
    }
}

/// One printer instruction. Hosts compose receipts/tickets out of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum PrintOp {
    /// A line of text (UTF-8 pass-through; Georgian etc. depends on printer firmware).
    #[serde(rename_all = "camelCase")]
    Text {
        text: String,
        #[serde(default)]
        bold: bool,
        #[serde(default)]
        align: Option<Align>,
        /// (width, height) multipliers 1..=8.
        #[serde(default)]
        size: Option<(u8, u8)>,
    },
    /// Raster image from a file path, optionally constrained to a max width in dots.
    #[serde(rename_all = "camelCase")]
    Image { path: String, #[serde(default)] max_width: Option<u32> },
    /// 2D code carrying a structured payload — hand-off tickets, payment links.
    ///
    /// Model 2 rather than escpos's Model 1 default: it is the model every
    /// phone camera and scanner app assumes, and Model 1 is a legacy encoding
    /// some readers no longer decode at all.
    #[serde(rename_all = "camelCase")]
    QrCode {
        data: String,
        /// Module size in dots, 1..=15 (escpos caps at 15). Defaults to 4;
        /// below 3 a 58mm head prints modules too fine for a phone to resolve.
        #[serde(default)]
        size: Option<u8>,
        /// Defaults to `M` — enough redundancy for thermal paper without
        /// spending a third of the capacity on it.
        #[serde(default)]
        correction: Option<QrEcc>,
    },
    /// Feed n lines.
    Feed { lines: u8 },
    /// Full cut.
    Cut,
    /// Kick the cash drawer (pin 2).
    DrawerKick,
    /// Raw ESC/POS bytes for anything the ops above don't cover.
    Raw { bytes: Vec<u8> },
}

/// Execute a job against a target. One connection per job — restaurant/retail
/// print volume never justifies pooling, and reconnect-per-job survives
/// printer power cycles.
/// Send bytes to a printer and read whatever it sends back.
///
/// The transport was write-only until now, which meant a printer could not be
/// asked anything: not its model, not its serial, not whether it has paper.
/// ESC/POS answers all three (`GS I n`, `DLE EOT n`), and `escpos`'s `Driver`
/// trait already has `read` on both the network and USB drivers — so this is a
/// door that only needed opening.
///
/// Deliberately generic: it moves bytes and returns bytes. Which query to send
/// and how to read the reply is ESC/POS semantics and belongs in the host's
/// TypeScript, not in here (the caller's thin-core rule).
///
/// One read, one short timeout, and an empty vec when the printer says nothing
/// — silence is a legitimate answer from a head that does not implement a
/// query, and it must not look like a failure.
pub fn query(target: &PrinterTarget, payload: &[u8], timeout_ms: u64) -> Result<Vec<u8>, String> {
    let timeout = Some(Duration::from_millis(timeout_ms.clamp(100, 10_000)));
    let mut buf = [0u8; 128];

    fn exchange<D: Driver>(driver: D, payload: &[u8], buf: &mut [u8]) -> Result<usize, PrinterError> {
        driver.write(payload)?;
        driver.flush()?;
        driver.read(buf)
    }

    let read = match target {
        PrinterTarget::Network { host, port } => {
            let driver = map_err(NetworkDriver::open(host, *port, timeout))?;
            exchange(driver, payload, &mut buf)
        }
        PrinterTarget::Usb { vendor_id, product_id } => {
            let driver = map_err(UsbDriver::open(*vendor_id, *product_id, timeout, None))?;
            exchange(driver, payload, &mut buf)
        }
        // The spooler takes a job and reports on the job, never on the head.
        PrinterTarget::System { .. } => {
            return Err("this printer is reached through the OS spooler, which cannot carry a reply".into())
        }
    };

    match read {
        Ok(n) => Ok(buf[..n].to_vec()),
        // A timeout means "this head does not answer that", not "the print
        // path is broken" — the caller shows an empty answer, not an error.
        Err(e) => {
            log::info!("printer query returned nothing: {e}");
            Ok(Vec::new())
        }
    }
}

pub fn print_job(target: &PrinterTarget, ops: &[PrintOp]) -> Result<(), String> {
    match target {
        PrinterTarget::Network { host, port } => {
            let mut printer = map_err(network_printer(host, *port))?;
            run_ops(&mut printer, ops)
        }
        PrinterTarget::Usb { vendor_id, product_id } => {
            let mut printer = map_err(usb_printer(*vendor_id, *product_id))?;
            run_ops(&mut printer, ops)
        }
        PrinterTarget::System { name } => {
            let (mut printer, buffer) = buffer_printer();
            run_ops(&mut printer, ops)?;
            let bytes = std::mem::take(&mut *buffer.lock().expect("print buffer"));
            winprint::raw_print(name, &bytes)
        }
    }
}

/// Kick the drawer without printing anything. Network path writes the pulse
/// directly (no init sequence — some printers feed paper on init).
pub fn open_cash_drawer(target: &PrinterTarget) -> Result<(), String> {
    match target {
        PrinterTarget::Network { host, port } => {
            use std::io::Write;
            use std::net::{SocketAddr, TcpStream};
            let addr: SocketAddr = format!("{host}:{port}")
                .parse()
                .map_err(|e: std::net::AddrParseError| e.to_string())?;
            let mut stream =
                TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| e.to_string())?;
            // ESC p 0 25 255 — drawer kick pulse on pin 2
            stream.write_all(&[0x1B, 0x70, 0x00, 0x19, 0xFF]).map_err(|e| e.to_string())?;
            Ok(())
        }
        PrinterTarget::Usb { vendor_id, product_id } => {
            let mut printer = map_err(usb_printer(*vendor_id, *product_id))?;
            map_err(printer.init())?;
            map_err(printer.cash_drawer(CashDrawer::Pin2))?;
            Ok(())
        }
        PrinterTarget::System { name } => {
            let (mut printer, buffer) = buffer_printer();
            map_err(printer.init())?;
            map_err(printer.cash_drawer(CashDrawer::Pin2))?;
            let bytes = std::mem::take(&mut *buffer.lock().expect("print buffer"));
            winprint::raw_print(name, &bytes)
        }
    }
}

fn run_ops<D: Driver>(printer: &mut Printer<D>, ops: &[PrintOp]) -> Result<(), String> {
    map_err(printer.init())?;
    for op in ops {
        match op {
            PrintOp::Text { text, bold, align, size } => {
                if let Some(align) = align {
                    map_err(printer.justify(match align {
                        Align::Left => JustifyMode::LEFT,
                        Align::Center => JustifyMode::CENTER,
                        Align::Right => JustifyMode::RIGHT,
                    }))?;
                }
                if let Some((w, h)) = size {
                    map_err(printer.size(*w, *h))?;
                }
                map_err(printer.bold(*bold))?;
                map_err(printer.writeln(text))?;
                map_err(printer.bold(false))?;
                if size.is_some() {
                    map_err(printer.reset_size())?;
                }
                if align.is_some() {
                    map_err(printer.justify(JustifyMode::LEFT))?;
                }
            }
            PrintOp::Image { path, max_width } => {
                let option = BitImageOption::new(*max_width, None, BitImageSize::Normal)
                    .map_err(|e| e.to_string())?;
                map_err(printer.bit_image_option(path, option))?;
            }
            PrintOp::QrCode { data, size, correction } => {
                let option = QRCodeOption::new(
                    QRCodeModel::Model2,
                    size.unwrap_or(4).clamp(1, 15),
                    correction.map_or(QRCodeCorrectionLevel::M, Into::into),
                );
                map_err(printer.qrcode_option(data, option))?;
            }
            PrintOp::Feed { lines } => {
                map_err(printer.feeds(*lines))?;
            }
            PrintOp::Cut => {
                map_err(printer.print_cut())?;
            }
            PrintOp::DrawerKick => {
                map_err(printer.cash_drawer(CashDrawer::Pin2))?;
            }
            PrintOp::Raw { bytes } => {
                map_err(printer.custom(bytes))?;
            }
        }
    }
    map_err(printer.print())?;
    Ok(())
}

// ---- transports (extracted from medusa-pos) ----

/// Shared byte buffer for capturing ESC/POS output.
type SharedBuffer = std::sync::Arc<std::sync::Mutex<Vec<u8>>>;

/// In-memory driver that captures ESC/POS bytes for later dispatch
/// (e.g. via the Windows print spooler).
#[derive(Clone)]
struct BufferDriver {
    buf: SharedBuffer,
}

impl Driver for BufferDriver {
    fn name(&self) -> String {
        "BufferDriver".into()
    }
    fn write(&self, data: &[u8]) -> Result<(), PrinterError> {
        self.buf.lock().expect("print buffer").extend_from_slice(data);
        Ok(())
    }
    fn read(&self, _buf: &mut [u8]) -> Result<usize, PrinterError> {
        Ok(0)
    }
    fn flush(&self) -> Result<(), PrinterError> {
        Ok(())
    }
}

fn buffer_printer() -> (Printer<BufferDriver>, SharedBuffer) {
    let buf: SharedBuffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::with_capacity(4096)));
    let printer = Printer::new(
        BufferDriver { buf: buf.clone() },
        Protocol::default(),
        Some(PrinterOptions::default()),
    );
    (printer, buf)
}

fn network_printer(host: &str, port: u16) -> Result<Printer<NetworkDriver>, PrinterError> {
    let driver = NetworkDriver::open(host, port, Some(Duration::from_secs(5)))?;
    Ok(Printer::new(driver, Protocol::default(), Some(PrinterOptions::default())))
}

fn usb_printer(vendor_id: u16, product_id: u16) -> Result<Printer<UsbDriver>, PrinterError> {
    log::info!("USB print path: libusb via escpos::UsbDriver (rusb), VID {vendor_id:04x} PID {product_id:04x}");
    UsbDriver::open(vendor_id, product_id, Some(Duration::from_secs(8)), None)
        .map(|driver| Printer::new(driver, Protocol::default(), Some(PrinterOptions::default())))
        .inspect_err(|e| log::error!("USB printer open failed (VID {vendor_id:04x} PID {product_id:04x}): {e}"))
}

fn map_err<T>(result: Result<T, PrinterError>) -> Result<T, String> {
    result.map_err(|e| {
        let s = e.to_string();
        log::error!("Printer error: {s}");
        s
    })
}
