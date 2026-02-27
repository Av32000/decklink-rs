//! Custom video buffer allocator provider support.
//!
//! This module provides traits and helpers for creating custom video buffer allocator
//! providers that control where DeckLink writes incoming frame data. This is useful
//! for receiving frames directly into GPU memory (e.g. CUDA pinned or device memory).

use crate::{sdk, SdkError};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};

/// Trait for a custom video buffer that supplies its own memory.
///
/// Implementors provide a pointer to memory where DeckLink will read/write pixel data,
/// and receive notifications about access begin/end for synchronization.
pub trait VideoBuffer: Send + Sync {
    /// Return a raw pointer to the buffer memory.
    /// For DMA-capable devices, this should be a host-visible pointer
    /// (e.g. CUDA pinned memory returned by `cuMemAllocHost`).
    fn get_bytes(&self) -> Result<*mut c_void, SdkError>;

    /// Called before DeckLink accesses the buffer (DMA write or CPU read).
    /// Use this to prepare memory (e.g. map for DMA, pin pages).
    fn start_access(&self, _flags: u32) -> Result<(), SdkError> {
        Ok(())
    }

    /// Called after DeckLink finishes accessing the buffer.
    /// Use this to finalize (e.g. trigger async device-to-device copy, unmap).
    fn end_access(&self, _flags: u32) -> Result<(), SdkError> {
        Ok(())
    }
}

/// Trait for allocating video buffers of a fixed specification.
///
/// One allocator is created per unique (width, height, pixel_format, rowBytes) combination.
pub trait VideoBufferAllocator: Send + Sync {
    /// Allocate a new video buffer. DeckLink may call this multiple times to
    /// build a pool of buffers for pipeline buffering.
    fn allocate(&self) -> Result<Box<dyn VideoBuffer>, SdkError>;
}

/// Information about the buffer format requested by DeckLink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferSpec {
    /// Total buffer size in bytes.
    pub buffer_size: u32,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Bytes per row (stride).
    pub row_bytes: u32,
    /// Pixel format (as raw SDK value, cast to `DecklinkPixelFormat`).
    pub pixel_format: u32,
}

/// Trait for providing video buffer allocators to the DeckLink runtime.
///
/// When DeckLink needs buffers with a new specification, it calls `get_allocator`
/// with the buffer parameters. The returned allocator is used to create individual
/// buffers of that specification.
pub trait VideoBufferAllocatorProvider: Send + Sync {
    /// Return an allocator for the given buffer specification.
    /// The allocator may be cached internally — DeckLink will call this once
    /// per unique buffer spec and reuse the allocator.
    fn get_allocator(&self, spec: BufferSpec) -> Result<Arc<dyn VideoBufferAllocator>, SdkError>;
}

// ============================================================================
// C callback bridge — wires Rust traits to the C FFI function pointers
// ============================================================================

// ---- VideoBuffer C callback trampolines ----

struct VideoBufferContext {
    buffer: Box<dyn VideoBuffer>,
}

unsafe extern "C" fn video_buffer_get_bytes(
    context: *mut c_void,
    buffer: *mut *mut c_void,
) -> sdk::HRESULT {
    let ctx = &*(context as *const VideoBufferContext);
    match ctx.buffer.get_bytes() {
        Ok(ptr) => {
            *buffer = ptr;
            0 // S_OK
        }
        Err(e) => e.code(),
    }
}

unsafe extern "C" fn video_buffer_start_access(
    context: *mut c_void,
    flags: sdk::DecklinkBufferAccessFlags,
) -> sdk::HRESULT {
    let ctx = &*(context as *const VideoBufferContext);
    match ctx.buffer.start_access(flags) {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

unsafe extern "C" fn video_buffer_end_access(
    context: *mut c_void,
    flags: sdk::DecklinkBufferAccessFlags,
) -> sdk::HRESULT {
    let ctx = &*(context as *const VideoBufferContext);
    match ctx.buffer.end_access(flags) {
        Ok(()) => 0,
        Err(e) => e.code(),
    }
}

unsafe extern "C" fn video_buffer_release(context: *mut c_void) {
    let _ = Box::from_raw(context as *mut VideoBufferContext);
}

/// Create a C `cdecklink_video_buffer_t` backed by a Rust `VideoBuffer`.
fn create_c_video_buffer(
    buffer: Box<dyn VideoBuffer>,
) -> Result<*mut sdk::cdecklink_video_buffer_t, SdkError> {
    let ctx = Box::into_raw(Box::new(VideoBufferContext { buffer }));
    let mut out: *mut sdk::cdecklink_video_buffer_t = null_mut();

    let result = unsafe {
        sdk::cdecklink_custom_video_buffer_create(
            ctx as *mut c_void,
            Some(video_buffer_get_bytes),
            Some(video_buffer_start_access),
            Some(video_buffer_end_access),
            Some(video_buffer_release),
            &mut out,
        )
    };

    if SdkError::is_ok(result) {
        Ok(out)
    } else {
        unsafe { drop(Box::from_raw(ctx)) };
        Err(SdkError::from(result))
    }
}

// ---- VideoBufferAllocator C callback trampolines ----

struct AllocatorContext {
    allocator: Arc<dyn VideoBufferAllocator>,
}

unsafe extern "C" fn allocator_allocate(
    context: *mut c_void,
    allocated_buffer: *mut *mut sdk::cdecklink_video_buffer_t,
) -> sdk::HRESULT {
    let ctx = &*(context as *const AllocatorContext);
    match ctx.allocator.allocate() {
        Ok(buffer) => match create_c_video_buffer(buffer) {
            Ok(c_buf) => {
                *allocated_buffer = c_buf;
                0 // S_OK
            }
            Err(e) => e.code(),
        },
        Err(e) => e.code(),
    }
}

unsafe extern "C" fn allocator_release(context: *mut c_void) {
    let _ = Box::from_raw(context as *mut AllocatorContext);
}

/// Create a C `cdecklink_video_buffer_allocator_t` backed by a Rust allocator.
fn create_c_allocator(
    allocator: Arc<dyn VideoBufferAllocator>,
) -> Result<*mut sdk::cdecklink_video_buffer_allocator_t, SdkError> {
    let ctx = Box::into_raw(Box::new(AllocatorContext { allocator }));
    let mut out: *mut sdk::cdecklink_video_buffer_allocator_t = null_mut();

    let result = unsafe {
        sdk::cdecklink_custom_video_buffer_allocator_create(
            ctx as *mut c_void,
            Some(allocator_allocate),
            Some(allocator_release),
            &mut out,
        )
    };

    if SdkError::is_ok(result) {
        Ok(out)
    } else {
        unsafe { drop(Box::from_raw(ctx)) };
        Err(SdkError::from(result))
    }
}

// ---- AllocatorProvider C callback trampolines ----

/// Internal context passed to C as the provider's opaque context pointer.
/// Owned by the C side — freed when the C provider is released.
struct ProviderContext {
    provider: Arc<dyn VideoBufferAllocatorProvider>,
    /// Cache of C allocator objects keyed by buffer spec, so we return the same
    /// C allocator pointer for repeated calls with the same spec.
    allocator_cache: Mutex<HashMap<BufferSpec, *mut sdk::cdecklink_video_buffer_allocator_t>>,
}

unsafe extern "C" fn provider_get_allocator(
    context: *mut c_void,
    buffer_size: u32,
    width: u32,
    height: u32,
    row_bytes: u32,
    pixel_format: sdk::DecklinkPixelFormat,
    allocator: *mut *mut sdk::cdecklink_video_buffer_allocator_t,
) -> sdk::HRESULT {
    let pctx = &*(context as *const ProviderContext);
    let spec = BufferSpec {
        buffer_size,
        width,
        height,
        row_bytes,
        pixel_format,
    };

    // Check cache first
    {
        let cache = pctx.allocator_cache.lock().unwrap();
        if let Some(&c_alloc) = cache.get(&spec) {
            // AddRef since DeckLink will take ownership of this reference
            sdk::cdecklink_video_buffer_allocator_add_ref(c_alloc);
            *allocator = c_alloc;
            return 0;
        }
    }

    // Ask the Rust provider for a new allocator
    match pctx.provider.get_allocator(spec) {
        Ok(rust_allocator) => match create_c_allocator(rust_allocator) {
            Ok(c_alloc) => {
                // AddRef for the cache
                sdk::cdecklink_video_buffer_allocator_add_ref(c_alloc);
                pctx.allocator_cache.lock().unwrap().insert(spec, c_alloc);
                *allocator = c_alloc;
                0
            }
            Err(e) => e.code(),
        },
        Err(e) => e.code(),
    }
}

unsafe extern "C" fn provider_release(context: *mut c_void) {
    let pctx = Box::from_raw(context as *mut ProviderContext);
    // Release all cached C allocator objects
    let cache = pctx.allocator_cache.lock().unwrap();
    for (_, c_alloc) in cache.iter() {
        if !c_alloc.is_null() {
            sdk::cdecklink_video_buffer_allocator_release(*c_alloc);
        }
    }
}

/// Create a C allocator provider object from a Rust `VideoBufferAllocatorProvider`.
///
/// Returns the raw C provider pointer. The caller is responsible for releasing it
/// via `cdecklink_video_buffer_allocator_provider_release` when done.
/// The internal bridge context is owned by the C object and freed when released.
pub(crate) fn create_c_allocator_provider(
    provider: Arc<dyn VideoBufferAllocatorProvider>,
) -> Result<*mut sdk::cdecklink_video_buffer_allocator_provider_t, SdkError> {
    let pctx = Box::into_raw(Box::new(ProviderContext {
        provider,
        allocator_cache: Mutex::new(HashMap::new()),
    }));

    let mut c_provider: *mut sdk::cdecklink_video_buffer_allocator_provider_t = null_mut();

    let result = unsafe {
        sdk::cdecklink_custom_video_buffer_allocator_provider_create(
            pctx as *mut c_void,
            Some(provider_get_allocator),
            Some(provider_release),
            &mut c_provider,
        )
    };

    if SdkError::is_ok(result) {
        Ok(c_provider)
    } else {
        unsafe { drop(Box::from_raw(pctx)) };
        Err(SdkError::from(result))
    }
}
