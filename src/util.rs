use num_traits::FromPrimitive;
use std::ffi::CStr;

// TODO - refactor the error type to abstract away weird errors?
#[derive(Debug, FromPrimitive)]
#[allow(overflowing_literals)]
pub enum SdkError {
    FALSE = 0x0000_0001,
    UNEXPECTED = -0x0000_FFFF,
    NOTIMPL = -0x0000_0001,
    OUTOFMEMORY = -0x0000_0002,
    INVALIDARG = -0x0000_0003,
    NOINTERFACE = -0x0000_0004,
    POINTER = -0x0000_0005,
    HANDLE = -0x0000_0006,
    ABORT = -0x0000_0007,
    FAIL = -0x0000_0008,
    ACCESSDENIED = -0x0009,
}

impl SdkError {
    /// Return the raw HRESULT code for this error.
    pub(crate) fn code(&self) -> i32 {
        // The enum discriminant values are the HRESULT codes
        // We can safely transmute since the repr is i32-compatible
        match self {
            SdkError::FALSE => 0x0000_0001,
            SdkError::UNEXPECTED => -0x0000_FFFFi32,
            SdkError::NOTIMPL => -0x0000_0001i32,
            SdkError::OUTOFMEMORY => -0x0000_0002i32,
            SdkError::INVALIDARG => -0x0000_0003i32,
            SdkError::NOINTERFACE => -0x0000_0004i32,
            SdkError::POINTER => -0x0000_0005i32,
            SdkError::HANDLE => -0x0000_0006i32,
            SdkError::ABORT => -0x0000_0007i32,
            SdkError::FAIL => -0x0000_0008i32,
            SdkError::ACCESSDENIED => -0x0009i32,
        }
    }

    #[allow(overflowing_literals)]
    pub(crate) fn from(value: i32) -> SdkError {
        Self::from_i32(value).unwrap_or(SdkError::FALSE)
    }
    pub(crate) fn is_false(value: i32) -> bool {
        value == (SdkError::FALSE as i32)
    }
    pub(crate) fn is_ok(value: i32) -> bool {
        value == 0
    }

    pub(crate) fn result<T>(r: i32) -> Result<T, SdkError>
    where
        T: Default,
    {
        Self::result_or_else(r, Default::default)
    }
    pub(crate) fn result_or<T>(r: i32, def: T) -> Result<T, SdkError> {
        if Self::is_ok(r) {
            Ok(def)
        } else {
            Err(Self::from(r))
        }
    }
    pub(crate) fn result_or_else<T, F: FnOnce() -> T>(r: i32, ok: F) -> Result<T, SdkError> {
        if Self::is_ok(r) {
            Ok(ok())
        } else {
            Err(Self::from(r))
        }
    }
}

pub(crate) unsafe fn convert_c_string(ptr: *const ::std::os::raw::c_char) -> String {
    CStr::from_ptr(ptr).to_str().unwrap_or_default().to_string()
}

pub(crate) unsafe fn convert_and_release_c_string(ptr: *const ::std::os::raw::c_char) -> String {
    let str = convert_c_string(ptr);
    crate::sdk::cdecklink_free_string(ptr);
    str
}
