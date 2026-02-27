mod device;
pub mod enums;
mod video_callback;

use crate::allocator::{create_c_allocator_provider, VideoBufferAllocatorProvider};
use crate::device::input::device::DecklinkInputDevicePtr;
use crate::device::input::video_callback::{register_input_callback, InputCallbackWrapper};
use crate::display_mode::{
    iterate_display_modes, DecklinkDisplayMode, DecklinkDisplayModeId,
};
use crate::frame::DecklinkPixelFormat;
use crate::{sdk, SdkError};
use num_traits::FromPrimitive;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub use crate::device::input::enums::*;
pub use crate::device::input::video_callback::DeckLinkInputCallback;
use crate::device::DecklinkDeviceDisplayModes;

pub struct DecklinkInputDevice {
    ptr: Arc<DecklinkInputDevicePtr>,
    callback_wrapper: *mut InputCallbackWrapper,
    video_active: bool,
    /// C allocator provider pointer, released on drop.
    allocator_provider: *mut sdk::cdecklink_video_buffer_allocator_provider_t,
}

// Safety: The underlying C pointer is thread-safe for the operations we perform
unsafe impl Send for DecklinkInputDevice {}

impl DecklinkDeviceDisplayModes<enums::DecklinkVideoInputFlags> for DecklinkInputDevice {
    fn does_support_video_mode(
        &self,
        mode: DecklinkDisplayModeId,
        pixel_format: DecklinkPixelFormat,
        flags: enums::DecklinkVideoInputFlags,
    ) -> Result<(bool, Option<DecklinkDisplayModeId>), SdkError> {
        let mut supported = false;
        let mut display_mode_id: u32 = 0;
        let result = unsafe {
            sdk::cdecklink_input_does_support_video_mode(
                self.ptr.dev,
                sdk::_DecklinkVideoConnection_decklinkVideoConnectionUnspecified,
                mode as u32,
                pixel_format as u32,
                sdk::_DecklinkVideoInputConversionMode_decklinkNoVideoInputConversion,
                flags.bits(),
                &mut display_mode_id,
                &mut supported,
            )
        };
        SdkError::result_or_else(result, move || {
            let possible_mode = DecklinkDisplayModeId::from_u32(display_mode_id);
            (supported, possible_mode)
        })
    }

    fn display_modes(&self) -> Result<Vec<DecklinkDisplayMode>, SdkError> {
        unsafe {
            let mut it = null_mut();
            let ok = sdk::cdecklink_input_get_display_mode_iterator(self.ptr.dev, &mut it);
            if SdkError::is_ok(ok) {
                let v = iterate_display_modes(it);
                sdk::cdecklink_display_mode_iterator_release(it);
                v
            } else {
                Err(SdkError::from(ok))
            }
        }
    }
}

impl DecklinkInputDevice {
    pub(crate) fn from(ptr: *mut crate::sdk::cdecklink_input_t) -> DecklinkInputDevice {
        DecklinkInputDevice {
            ptr: Arc::new(DecklinkInputDevicePtr {
                dev: ptr,
                video_active: Arc::new(AtomicBool::new(false)),
            }),
            callback_wrapper: null_mut(),
            video_active: false,
            allocator_provider: null_mut(),
        }
    }

    /// Enable video input with the specified display mode, pixel format, and flags.
    /// A callback must be set before starting streams.
    pub fn enable_video_input(
        &mut self,
        mode: DecklinkDisplayModeId,
        pixel_format: DecklinkPixelFormat,
        flags: enums::DecklinkVideoInputFlags,
    ) -> Result<(), SdkError> {
        if self.ptr.video_active.swap(true, Ordering::Relaxed) {
            return Err(SdkError::ACCESSDENIED);
        }
        let result = unsafe {
            sdk::cdecklink_input_enable_video_input(
                self.ptr.dev,
                mode as u32,
                pixel_format as u32,
                flags.bits(),
            )
        };
        if !SdkError::is_ok(result) {
            self.ptr.video_active.store(false, Ordering::Relaxed);
            return Err(SdkError::from(result));
        }
        self.video_active = true;
        Ok(())
    }

    /// Disable video input.
    pub fn disable_video_input(&mut self) -> Result<(), SdkError> {
        let result = unsafe { sdk::cdecklink_input_disable_video_input(self.ptr.dev) };
        self.video_active = false;
        self.ptr.video_active.store(false, Ordering::Relaxed);

        // Release the allocator provider if one was set
        if !self.allocator_provider.is_null() {
            unsafe {
                sdk::cdecklink_video_buffer_allocator_provider_release(self.allocator_provider)
            };
            self.allocator_provider = null_mut();
        }

        SdkError::result(result)
    }

    /// Enable video input with a custom allocator provider.
    ///
    /// The allocator provider controls where DeckLink writes incoming frame data.
    /// This is useful for receiving frames directly into GPU memory (e.g. CUDA).
    ///
    /// The provider will be asked to create allocators for specific buffer
    /// specifications, and those allocators will be used to allocate individual
    /// video buffers where DeckLink DMAs frame data.
    ///
    /// A callback must be set before starting streams.
    pub fn enable_video_input_with_allocator(
        &mut self,
        mode: DecklinkDisplayModeId,
        pixel_format: DecklinkPixelFormat,
        flags: enums::DecklinkVideoInputFlags,
        provider: Arc<dyn VideoBufferAllocatorProvider>,
    ) -> Result<(), SdkError> {
        if self.ptr.video_active.swap(true, Ordering::Relaxed) {
            return Err(SdkError::ACCESSDENIED);
        }

        // Create the C allocator provider from the Rust trait object
        let c_provider = create_c_allocator_provider(provider)?;

        let result = unsafe {
            sdk::cdecklink_input_enable_video_input_with_allocator_provider(
                self.ptr.dev,
                mode as u32,
                pixel_format as u32,
                flags.bits(),
                c_provider,
            )
        };

        if !SdkError::is_ok(result) {
            // Release the C provider on failure
            unsafe { sdk::cdecklink_video_buffer_allocator_provider_release(c_provider) };
            self.ptr.video_active.store(false, Ordering::Relaxed);
            return Err(SdkError::from(result));
        }

        // Store the provider so we release it on drop/disable
        self.allocator_provider = c_provider;
        self.video_active = true;
        Ok(())
    }

    /// Enable audio input with the specified sample rate, sample type, and channel count.
    pub fn enable_audio_input(
        &self,
        sample_rate: enums::DecklinkAudioSampleRate,
        sample_type: enums::DecklinkAudioSampleType,
        channel_count: u32,
    ) -> Result<(), SdkError> {
        let result = unsafe {
            sdk::cdecklink_input_enable_audio_input(
                self.ptr.dev,
                sample_rate as u32,
                sample_type as u32,
                channel_count,
            )
        };
        SdkError::result(result)
    }

    /// Disable audio input.
    pub fn disable_audio_input(&self) -> Result<(), SdkError> {
        let result = unsafe { sdk::cdecklink_input_disable_audio_input(self.ptr.dev) };
        SdkError::result(result)
    }

    /// Set the input callback handler. Must be called before `start_streams`.
    pub fn set_callback(
        &mut self,
        handler: Option<Arc<dyn DeckLinkInputCallback>>,
    ) -> Result<(), SdkError> {
        // Register the internal C callback wrapper if not already done
        if self.callback_wrapper.is_null() {
            self.callback_wrapper = register_input_callback(&self.ptr)?;
        }

        unsafe {
            let wrapper = &(*self.callback_wrapper);
            *wrapper.handler.write().unwrap() = handler;
        }
        Ok(())
    }

    /// Start capturing streams (video and/or audio).
    pub fn start_streams(&self) -> Result<(), SdkError> {
        let result = unsafe { sdk::cdecklink_input_start_streams(self.ptr.dev) };
        SdkError::result(result)
    }

    /// Stop capturing streams.
    pub fn stop_streams(&self) -> Result<(), SdkError> {
        let result = unsafe { sdk::cdecklink_input_stop_streams(self.ptr.dev) };
        SdkError::result(result)
    }

    /// Pause capturing streams.
    pub fn pause_streams(&self) -> Result<(), SdkError> {
        let result = unsafe { sdk::cdecklink_input_pause_streams(self.ptr.dev) };
        SdkError::result(result)
    }

    /// Flush all buffered frames.
    pub fn flush_streams(&self) -> Result<(), SdkError> {
        let result = unsafe { sdk::cdecklink_input_flush_streams(self.ptr.dev) };
        SdkError::result(result)
    }

    /// Get the number of available video frames in the buffer.
    pub fn available_video_frame_count(&self) -> Result<u32, SdkError> {
        let mut count = 0u32;
        let result = unsafe {
            sdk::cdecklink_input_get_available_video_frame_count(self.ptr.dev, &mut count)
        };
        SdkError::result_or(result, count)
    }

    /// Get the number of available audio sample frames in the buffer.
    pub fn available_audio_sample_frame_count(&self) -> Result<u32, SdkError> {
        let mut count = 0u32;
        let result = unsafe {
            sdk::cdecklink_input_get_available_audio_sample_frame_count(self.ptr.dev, &mut count)
        };
        SdkError::result_or(result, count)
    }
}

impl Drop for DecklinkInputDevice {
    fn drop(&mut self) {
        unsafe {
            if self.video_active {
                let _ = sdk::cdecklink_input_stop_streams(self.ptr.dev);
                let _ = sdk::cdecklink_input_disable_video_input(self.ptr.dev);
                self.ptr.video_active.store(false, Ordering::Relaxed);
            }

            // Clear the callback to release the C++ side reference
            if !self.callback_wrapper.is_null() {
                // Set a null callback to ensure no more callbacks fire
                sdk::cdecklink_input_set_callback(
                    self.ptr.dev,
                    null_mut(),
                    None,
                    None,
                );
                drop(Box::from_raw(self.callback_wrapper));
                self.callback_wrapper = null_mut();
            }

            // Release the allocator provider if one was set
            if !self.allocator_provider.is_null() {
                sdk::cdecklink_video_buffer_allocator_provider_release(self.allocator_provider);
                self.allocator_provider = null_mut();
            }
        }
    }
}
