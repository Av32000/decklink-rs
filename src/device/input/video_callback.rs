use crate::device::input::device::DecklinkInputDevicePtr;
use crate::device::input::enums::{
    DecklinkDetectedVideoInputFormatFlags, DecklinkVideoInputFormatChangedEvents,
};
use crate::display_mode::DecklinkDisplayModeId;
use crate::frame::DecklinkVideoFrame;
use crate::{sdk, SdkError};
use num_traits::FromPrimitive;
use std::sync::{Arc, RwLock};

pub(crate) fn free_callback_wrapper(wrapper: *mut InputCallbackWrapper) {
    unsafe {
        drop(Box::from_raw(wrapper));
    }
}

pub fn register_input_callback(
    ptr: &Arc<DecklinkInputDevicePtr>,
) -> Result<*mut InputCallbackWrapper, SdkError> {
    let callback_wrapper = Box::into_raw(Box::new(InputCallbackWrapper {
        handler: RwLock::new(None),
    }));

    let result = unsafe {
        sdk::cdecklink_input_set_callback(
            ptr.dev,
            callback_wrapper as *mut std::ffi::c_void,
            Some(video_input_format_changed_callback),
            Some(video_input_frame_arrived_callback),
        )
    };

    match SdkError::result_or(result, callback_wrapper) {
        Err(e) => {
            free_callback_wrapper(callback_wrapper);
            Err(e)
        }
        Ok(v) => Ok(v),
    }
}

/// Trait for receiving input callbacks from the DeckLink device.
pub trait DeckLinkInputCallback: Send + Sync {
    /// Called when the video input format changes (e.g. resolution, field dominance, colorspace).
    fn video_input_format_changed(
        &self,
        events: DecklinkVideoInputFormatChangedEvents,
        new_display_mode: DecklinkDisplayModeId,
        detected_signal_flags: DecklinkDetectedVideoInputFormatFlags,
    );

    /// Called when a new video frame arrives from the input.
    /// Return `true` to indicate success.
    fn video_input_frame_arrived(&self, video_frame: Option<DecklinkVideoFrame>) -> bool;
}

pub struct InputCallbackWrapper {
    pub handler: RwLock<Option<Arc<dyn DeckLinkInputCallback>>>,
}

extern "C" fn video_input_format_changed_callback(
    context: *mut ::std::os::raw::c_void,
    notification_events: sdk::DecklinkVideoInputFormatChangedEvents,
    new_display_mode: *mut sdk::cdecklink_display_mode_t,
    detected_signal_flags: sdk::DecklinkDetectedVideoInputFormatFlags,
) -> sdk::HRESULT {
    let wrapper: &InputCallbackWrapper = unsafe { &*(context as *const _) };

    if let Some(handler) = &*wrapper.handler.read().unwrap() {
        let events = DecklinkVideoInputFormatChangedEvents::from_bits_truncate(notification_events);
        let mode_id = if new_display_mode.is_null() {
            DecklinkDisplayModeId::Unknown
        } else {
            let raw = unsafe { sdk::cdecklink_display_mode_get_display_mode(new_display_mode) };
            DecklinkDisplayModeId::from_u32(raw).unwrap_or(DecklinkDisplayModeId::Unknown)
        };
        let flags =
            DecklinkDetectedVideoInputFormatFlags::from_bits_truncate(detected_signal_flags);

        handler.video_input_format_changed(events, mode_id, flags);
    }

    0 // S_OK
}

extern "C" fn video_input_frame_arrived_callback(
    context: *mut ::std::os::raw::c_void,
    video_frame: *mut sdk::cdecklink_video_input_frame_t,
    _audio_packet: *mut sdk::cdecklink_audio_input_packet_t,
) -> sdk::HRESULT {
    let wrapper: &InputCallbackWrapper = unsafe { &*(context as *const _) };

    let mut result = true;
    if let Some(handler) = &*wrapper.handler.read().unwrap() {
        let frame = if video_frame.is_null() {
            None
        } else {
            // Convert the input frame to a generic video frame for reading pixel data
            let video_frame_ptr =
                unsafe { sdk::cdecklink_video_input_frame_to_video_frame(video_frame) };
            if video_frame_ptr.is_null() {
                None
            } else {
                Some(unsafe { DecklinkVideoFrame::from(video_frame_ptr) })
            }
        };

        result = handler.video_input_frame_arrived(frame);
    }

    if result {
        0 // S_OK
    } else {
        1 // S_FALSE
    }
}
