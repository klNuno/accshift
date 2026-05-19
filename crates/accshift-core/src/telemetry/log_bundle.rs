use crate::context::AppContext;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Builds an in-memory zip of the app log files (`app.log` and
/// `app.previous.log`).
///
/// Logs are already sanitized at write time (user paths and emails redacted),
/// so no extra processing is needed here.
///
/// Returns the zip bytes, ready to be sent to `/logs`.
pub fn build(ctx: &dyn AppContext) -> Result<Vec<u8>, String> {
    let current = crate::logging::log_file_path(ctx)?;
    let previous = current.with_file_name("app.previous.log");

    let mut buf = Vec::with_capacity(16 * 1024);
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buf));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        if current.exists() {
            zip.start_file("app.log", options)
                .map_err(|e| format!("zip start_file app.log: {e}"))?;
            let data = std::fs::read(&current).map_err(|e| format!("read app.log: {e}"))?;
            zip.write_all(&data)
                .map_err(|e| format!("zip write app.log: {e}"))?;
        }

        if previous.exists() {
            zip.start_file("app.previous.log", options)
                .map_err(|e| format!("zip start_file app.previous.log: {e}"))?;
            let data =
                std::fs::read(&previous).map_err(|e| format!("read app.previous.log: {e}"))?;
            zip.write_all(&data)
                .map_err(|e| format!("zip write app.previous.log: {e}"))?;
        }

        zip.finish().map_err(|e| format!("zip finish: {e}"))?;
    }
    Ok(buf)
}
