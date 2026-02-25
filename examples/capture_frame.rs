extern crate decklink;
#[macro_use]
extern crate text_io;

use decklink::device::input::{
    DeckLinkInputCallback, DecklinkDetectedVideoInputFormatFlags, DecklinkVideoInputFlags,
    DecklinkVideoInputFormatChangedEvents,
};
use decklink::device::DecklinkDeviceDisplayModes;
use decklink::device::{get_devices, DecklinkDevice};
use decklink::display_mode::{DecklinkDisplayMode, DecklinkDisplayModeId};
use decklink::frame::{DecklinkFrameBase, DecklinkPixelFormat, DecklinkVideoFrame};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// Callback handler that captures a frame after skipping initial black frames.
struct FrameCapture {
    frame_data: Mutex<Option<Vec<u8>>>,
    frame_info: Mutex<Option<FrameInfo>>,
    frame_ready: Condvar,
    captured: AtomicBool,
    frames_seen: std::sync::atomic::AtomicU32,
}

struct FrameInfo {
    width: usize,
    height: usize,
    row_bytes: usize,
    pixel_format: DecklinkPixelFormat,
}

impl DeckLinkInputCallback for FrameCapture {
    fn video_input_format_changed(
        &self,
        events: DecklinkVideoInputFormatChangedEvents,
        new_display_mode: DecklinkDisplayModeId,
        detected_signal_flags: DecklinkDetectedVideoInputFormatFlags,
    ) {
        println!(
            "Input format changed: events={:?}, mode={:?}, flags={:?}",
            events, new_display_mode, detected_signal_flags
        );
    }

    fn video_input_frame_arrived(&self, video_frame: Option<DecklinkVideoFrame>) -> bool {
        // Only capture once
        if self.captured.load(Ordering::Relaxed) {
            return true;
        }

        if let Some(frame) = video_frame {
            let count = self.frames_seen.fetch_add(1, Ordering::Relaxed);
            let width = frame.width();
            let height = frame.height();
            let row_bytes = frame.row_bytes();
            let pixel_format = frame.pixel_format();

            println!(
                "Frame #{}: {}x{}, row_bytes={}, format={:?}",
                count + 1,
                width,
                height,
                row_bytes,
                pixel_format
            );

            // Skip first 30 frames to let the signal stabilize
            if count < 60 {
                return true;
            }

            match frame.bytes_to_vec() {
                Ok(data) => {
                    self.captured.store(true, Ordering::Relaxed);

                    *self.frame_info.lock().unwrap() = Some(FrameInfo {
                        width,
                        height,
                        row_bytes,
                        pixel_format,
                    });
                    *self.frame_data.lock().unwrap() = Some(data);
                    self.frame_ready.notify_all();
                }
                Err(e) => {
                    eprintln!("Failed to read frame bytes: {:?}", e);
                }
            }
        } else {
            println!("Frame arrived with no video data (no input signal?)");
        }

        true
    }
}

fn select_input_and_format() -> Option<(
    DecklinkDevice,
    decklink::device::input::DecklinkInputDevice,
    DecklinkDisplayMode,
)> {
    let device = {
        let mut devices = get_devices().expect("Failed to list devices");
        if devices.is_empty() {
            println!("No Blackmagic Design devices were found.");
            return None;
        }

        println!("Found {} device(s):", devices.len());
        for i in 0..devices.len() {
            println!(
                "  {}: {}",
                i,
                devices[i]
                    .display_name()
                    .unwrap_or_else(|| "Unknown".to_string())
            );
        }

        print!("Select device index: ");
        let index: usize = read!();
        if index >= devices.len() {
            println!("Invalid device index");
            return None;
        }

        devices.swap_remove(index)
    };

    println!(
        "Selected: {}\n",
        device
            .display_name()
            .unwrap_or_else(|| "Unknown".to_string())
    );

    let input = match device.input() {
        None => {
            println!("Failed to get device input interface");
            return None;
        }
        Some(i) => i,
    };

    let mode = {
        let mut supported_modes = input
            .display_modes()
            .expect("Failed to list input display modes");

        println!("Available input modes:");
        for i in 0..supported_modes.len() {
            let m = &supported_modes[i];
            let framerate = m
                .framerate()
                .map(|(d, s)| {
                    if d > 0 {
                        format!("{:.2} fps", s as f64 / d as f64)
                    } else {
                        "? fps".to_string()
                    }
                })
                .unwrap_or_else(|| "? fps".to_string());
            println!(
                "  {}: {} ({}x{}, {})",
                i,
                m.name().unwrap_or_else(|| "Unknown".to_string()),
                m.width(),
                m.height(),
                framerate,
            );
        }

        print!("Select mode index: ");
        let index: usize = read!();
        if index >= supported_modes.len() {
            println!("Invalid mode index");
            return None;
        }

        supported_modes.swap_remove(index)
    };

    Some((device, input, mode))
}

/// Write raw frame data as a simple PPM image file (P6 format).
/// This works for 8-bit BGRA by converting to RGB.
fn write_ppm(
    path: &str,
    width: usize,
    height: usize,
    row_bytes: usize,
    pixel_format: DecklinkPixelFormat,
    data: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;

    let mut file = std::fs::File::create(path)?;

    // PPM header
    write!(file, "P6\n{} {}\n255\n", width, height)?;

    match pixel_format {
        DecklinkPixelFormat::Format8BitBGRA => {
            // BGRA -> RGB
            for y in 0..height {
                let row_start = y * row_bytes;
                for x in 0..width {
                    let offset = row_start + x * 4;
                    if offset + 2 < data.len() {
                        let b = data[offset];
                        let g = data[offset + 1];
                        let r = data[offset + 2];
                        file.write_all(&[r, g, b])?;
                    } else {
                        file.write_all(&[0, 0, 0])?;
                    }
                }
            }
        }
        DecklinkPixelFormat::Format8BitARGB => {
            // ARGB -> RGB
            for y in 0..height {
                let row_start = y * row_bytes;
                for x in 0..width {
                    let offset = row_start + x * 4;
                    if offset + 3 < data.len() {
                        let r = data[offset + 1];
                        let g = data[offset + 2];
                        let b = data[offset + 3];
                        file.write_all(&[r, g, b])?;
                    } else {
                        file.write_all(&[0, 0, 0])?;
                    }
                }
            }
        }
        DecklinkPixelFormat::Format8BitYUV => {
            // UYVY (8-bit YUV 4:2:2) -> RGB
            // Each 4 bytes: U Y0 V Y1 -> two pixels
            for y in 0..height {
                let row_start = y * row_bytes;
                let mut x = 0;
                while x < width {
                    let offset = row_start + (x / 2) * 4;
                    if offset + 3 < data.len() {
                        let u = data[offset] as f64 - 128.0;
                        let y0 = data[offset + 1] as f64;
                        let v = data[offset + 2] as f64 - 128.0;
                        let y1 = data[offset + 3] as f64;

                        // Pixel 0
                        let r = (y0 + 1.402 * v).clamp(0.0, 255.0) as u8;
                        let g = (y0 - 0.344136 * u - 0.714136 * v).clamp(0.0, 255.0) as u8;
                        let b = (y0 + 1.772 * u).clamp(0.0, 255.0) as u8;
                        file.write_all(&[r, g, b])?;

                        // Pixel 1
                        if x + 1 < width {
                            let r = (y1 + 1.402 * v).clamp(0.0, 255.0) as u8;
                            let g = (y1 - 0.344136 * u - 0.714136 * v).clamp(0.0, 255.0) as u8;
                            let b = (y1 + 1.772 * u).clamp(0.0, 255.0) as u8;
                            file.write_all(&[r, g, b])?;
                        }
                    } else {
                        file.write_all(&[0, 0, 0])?;
                        if x + 1 < width {
                            file.write_all(&[0, 0, 0])?;
                        }
                    }
                    x += 2;
                }
            }
        }
        _ => {
            // For other formats, just write raw data as grayscale-ish (best effort)
            eprintln!(
                "Warning: pixel format {:?} not fully supported for PPM conversion, writing raw bytes",
                pixel_format
            );
            for y in 0..height {
                let row_start = y * row_bytes;
                for x in 0..width {
                    let offset = row_start + x * 3;
                    if offset + 2 < data.len() {
                        file.write_all(&data[offset..offset + 3])?;
                    } else {
                        file.write_all(&[0, 0, 0])?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn main() {
    if let Ok(version) = decklink::api_version() {
        println!("DeckLink driver version: {}", version);
    } else {
        println!("Failed to get DeckLink driver version. Are the drivers installed?");
        return;
    }

    let (_device, mut input, mode) = match select_input_and_format() {
        Some(v) => v,
        None => return,
    };

    let pixel_format = DecklinkPixelFormat::Format8BitYUV;

    println!(
        "\nConfiguring capture: {} ({}x{}) with {:?}",
        mode.name().unwrap_or_else(|| "Unknown".to_string()),
        mode.width(),
        mode.height(),
        pixel_format,
    );

    // Enable video input
    input
        .enable_video_input(mode.mode(), pixel_format, DecklinkVideoInputFlags::empty())
        .expect("Failed to enable video input");

    // Create capture callback
    let capture = Arc::new(FrameCapture {
        frame_data: Mutex::new(None),
        frame_info: Mutex::new(None),
        frame_ready: Condvar::new(),
        captured: AtomicBool::new(false),
        frames_seen: std::sync::atomic::AtomicU32::new(0),
    });

    // Set callback
    input
        .set_callback(Some(capture.clone()))
        .expect("Failed to set input callback");

    // Start capture
    input.start_streams().expect("Failed to start streams");

    println!("Waiting for a frame... (make sure a signal is connected)");

    // Wait for a frame (with 10 second timeout)
    {
        let mut data = capture.frame_data.lock().unwrap();
        let timeout = std::time::Duration::from_secs(10);
        let result = capture
            .frame_ready
            .wait_timeout_while(data, timeout, |d| d.is_none())
            .unwrap();
        data = result.0;

        if data.is_none() {
            println!("Timeout: No frame received within 10 seconds.");
            println!("Make sure a video source is connected to the DeckLink device.");
            input.stop_streams().ok();
            return;
        }
    }

    // Stop capture
    input.stop_streams().expect("Failed to stop streams");

    // Save the captured frame
    let frame_data = capture.frame_data.lock().unwrap().take();
    let frame_info = capture.frame_info.lock().unwrap().take();

    if let (Some(data), Some(info)) = (frame_data, frame_info) {
        let raw_path = "captured_frame.raw";
        let ppm_path = "captured_frame.ppm";

        // Write raw bytes
        std::fs::write(raw_path, &data).expect("Failed to write raw frame file");
        println!(
            "Raw frame saved to {} ({} bytes, {}x{}, {:?})",
            raw_path,
            data.len(),
            info.width,
            info.height,
            info.pixel_format
        );

        // Write PPM image
        match write_ppm(
            ppm_path,
            info.width,
            info.height,
            info.row_bytes,
            info.pixel_format,
            &data,
        ) {
            Ok(_) => println!("PPM image saved to {}", ppm_path),
            Err(e) => eprintln!("Failed to write PPM: {}", e),
        }
    } else {
        println!("No frame data available");
    }
}
