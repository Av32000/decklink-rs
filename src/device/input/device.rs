use crate::sdk;
use std::ptr::null_mut;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct DecklinkInputDevicePtr {
    pub(crate) dev: *mut crate::sdk::cdecklink_input_t,
    pub video_active: Arc<AtomicBool>,
}

unsafe impl Send for DecklinkInputDevicePtr {}
unsafe impl Sync for DecklinkInputDevicePtr {}

impl Drop for DecklinkInputDevicePtr {
    fn drop(&mut self) {
        if !self.dev.is_null() {
            unsafe { sdk::cdecklink_input_release(self.dev) };
            self.dev = null_mut();
        }
    }
}
