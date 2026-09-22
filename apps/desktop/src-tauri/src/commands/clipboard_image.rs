//! Native clipboard image read for the composer paste path.
//!
//! WebKitGTK hides clipboard images from the webview paste event, so the Linux
//! adapter reads them through GTK. macOS and Windows webviews expose pasted
//! images as files on the event itself and never need this fallback.
//!
//! Clipboard content is user data: it is returned to the caller and never
//! logged or recorded in diagnostics.

use tauri::{AppHandle, ipc::Response};

/// Answers with the clipboard image encoded as PNG, or an empty body when the
/// clipboard holds no image. The raw body keeps the bytes out of JSON.
#[tauri::command]
pub async fn read_clipboard_image_png(app: AppHandle) -> Result<Response, String> {
    Ok(Response::new(
        read_clipboard_image(&app).await.unwrap_or_default(),
    ))
}

#[cfg(target_os = "linux")]
const CLIPBOARD_IMAGE_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(target_os = "linux")]
async fn read_clipboard_image(app: &AppHandle) -> Option<Vec<u8>> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    // GTK is main-thread only; the clipboard answers through its main loop.
    app.run_on_main_thread(move || {
        gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD).request_image(move |_, pixbuf| {
            let _ = sender.send(pixbuf.and_then(encode_png));
        });
    })
    .ok()?;
    tokio::time::timeout(CLIPBOARD_IMAGE_READ_TIMEOUT, receiver)
        .await
        .ok()?
        .ok()?
}

#[cfg(target_os = "linux")]
fn encode_png(pixbuf: &gtk::gdk_pixbuf::Pixbuf) -> Option<Vec<u8>> {
    pixbuf.save_to_bufferv("png", &[]).ok()
}

#[cfg(not(target_os = "linux"))]
async fn read_clipboard_image(_app: &AppHandle) -> Option<Vec<u8>> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use gtk::gdk_pixbuf::{Colorspace, Pixbuf};

    use super::encode_png;

    #[test]
    fn clipboard_pixbuf_is_encoded_as_png_with_its_dimensions() {
        let pixbuf = Pixbuf::new(Colorspace::Rgb, true, 8, 3, 2).expect("pixbuf allocation");
        pixbuf.fill(0xff00_00ff);

        let png = encode_png(&pixbuf).expect("png encoding");

        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // IHDR width and height are the big-endian words at bytes 16..24.
        assert_eq!(&png[16..24], &[0, 0, 0, 3, 0, 0, 0, 2]);
    }
}
