//! Example: Capture video frames from a DeckLink device into CUDA pinned memory.
//!
//! Requires: `cargo run --example cuda_capture --features cuda`
//!
//! This demonstrates using the CUDA allocator provider to receive DeckLink
//! input frames directly into CUDA pinned (page-locked) host memory, which
//! can then be efficiently transferred to GPU device memory.

extern crate cudarc;
extern crate decklink;
#[macro_use]
extern crate text_io;

use decklink::allocator::VideoBufferAllocatorProvider;
use decklink::cuda::CudaAllocatorProvider;
use decklink::device::input::{
    DeckLinkInputCallback, DecklinkDetectedVideoInputFormatFlags, DecklinkVideoInputFlags,
    DecklinkVideoInputFormatChangedEvents,
};
use decklink::device::DecklinkDeviceDisplayModes;
use decklink::device::{get_devices, DecklinkDevice};
use decklink::display_mode::{DecklinkDisplayMode, DecklinkDisplayModeId};
use decklink::frame::{DecklinkFrameBase, DecklinkPixelFormat, DecklinkVideoFrame};

use cudarc::driver::CudaContext;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// Callback handler that captures frames arriving in CUDA pinned memory.
struct CudaFrameCapture {
    frame_count: AtomicU32,
    max_frames: u32,
    done: AtomicBool,
    notify: Condvar,
    lock: Mutex<()>,
}

impl CudaFrameCapture {
    fn new(max_frames: u32) -> Self {
        Self {
            frame_count: AtomicU32::new(0),
            max_frames,
            done: AtomicBool::new(false),
            notify: Condvar::new(),
            lock: Mutex::new(()),
        }
    }
}

impl DeckLinkInputCallback for CudaFrameCapture {
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
        if self.done.load(Ordering::Relaxed) {
            return true;
        }

        if let Some(frame) = video_frame {
            let count = self.frame_count.fetch_add(1, Ordering::Relaxed) + 1;
            println!(
                "Frame #{}: {}x{}, row_bytes={}, format={:?} (in CUDA pinned memory)",
                count,
                frame.width(),
                frame.height(),
                frame.row_bytes(),
                frame.pixel_format(),
            );

            // The frame data is already in CUDA pinned memory!
            // You can now efficiently copy it to GPU device memory using:
            //   cuMemcpyHtoDAsync(device_ptr, host_ptr, size, stream)
            // Or access it directly via zero-copy if supported.

            if count >= self.max_frames {
                self.done.store(true, Ordering::Relaxed);
                let _lock = self.lock.lock().unwrap();
                self.notify.notify_all();
            }
        }

        true
    }
}

fn select_device(devices: &[DecklinkDevice]) -> usize {
    println!("\nAvailable DeckLink devices:");
    for (i, dev) in devices.iter().enumerate() {
        let name = dev.display_name().unwrap_or_else(|| "Unknown".to_string());
        println!("  [{}] {}", i, name);
    }
    print!("\nSelect device index: ");
    let idx: usize = read!();
    idx
}

fn select_display_mode(modes: &[DecklinkDisplayMode]) -> usize {
    println!("\nAvailable display modes:");
    for (i, mode) in modes.iter().enumerate() {
        let name = mode.name().unwrap_or_else(|| "Unknown".to_string());
        println!(
            "  [{}] {} ({}x{}, {:?})",
            i,
            name,
            mode.width(),
            mode.height(),
            mode.mode(),
        );
    }
    print!("\nSelect display mode index: ");
    let idx: usize = read!();
    idx
}

fn main() {
    // Initialize CUDA
    let ctx = CudaContext::new(0).expect("Failed to initialize CUDA context 0");
    println!("CUDA context initialized");

    // Create the CUDA allocator provider
    let cuda_provider: Arc<dyn VideoBufferAllocatorProvider> =
        Arc::new(CudaAllocatorProvider::new(ctx));

    // Get DeckLink devices
    let devices = get_devices().expect("Failed to enumerate DeckLink devices");
    if devices.is_empty() {
        eprintln!("No DeckLink devices found.");
        return;
    }

    let dev_idx = select_device(&devices);
    let device = &devices[dev_idx];

    // Get input device
    let mut input = device.input().expect("Failed to get input device");

    // List display modes
    let modes = input.display_modes().expect("Failed to get display modes");
    let mode_idx = select_display_mode(&modes);
    let selected_mode = modes[mode_idx].mode();

    let pixel_format = DecklinkPixelFormat::Format8BitYUV;

    // Enable video input with CUDA allocator provider
    input
        .enable_video_input_with_allocator(
            selected_mode,
            pixel_format,
            DecklinkVideoInputFlags::empty(),
            cuda_provider,
        )
        .expect("Failed to enable video input with CUDA allocator");

    println!("\nVideo input enabled with CUDA pinned memory allocator");

    // Set up callback
    let capture = Arc::new(CudaFrameCapture::new(30)); // Capture 30 frames
    input
        .set_callback(Some(capture.clone()))
        .expect("Failed to set callback");

    // Start streaming
    input.start_streams().expect("Failed to start streams");
    println!("Capturing 30 frames into CUDA pinned memory...\n");

    // Wait for frames
    let lock = capture.lock.lock().unwrap();
    let _lock = capture
        .notify
        .wait_while(lock, |_| !capture.done.load(Ordering::Relaxed))
        .unwrap();

    // Stop
    input.stop_streams().expect("Failed to stop streams");
    input
        .disable_video_input()
        .expect("Failed to disable video input");

    let total = capture.frame_count.load(Ordering::Relaxed);
    println!("\nDone! Captured {} frames into CUDA pinned memory.", total);
}
