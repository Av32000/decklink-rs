//! CUDA-backed video buffer allocator for DeckLink capture.
//!
//! This module provides [`CudaAllocatorProvider`] which allocates DeckLink video
//! buffers in CUDA pinned (page-locked) host memory. Pinned memory allows the
//! DeckLink DMA engine to write directly into memory that is efficiently
//! accessible by the GPU, avoiding an extra host-to-device copy.
//!
//! # Usage
//!
//! ```no_run
//! use decklink::cuda::CudaAllocatorProvider;
//! use cudarc::driver::CudaContext;
//! use std::sync::Arc;
//!
//! let ctx = CudaContext::new(0).unwrap();
//! let provider = Arc::new(CudaAllocatorProvider::new(ctx));
//!
//! // Use with DecklinkInputDevice::enable_video_input_with_allocator
//! ```
//!
//! Requires the `cuda` feature.

use crate::allocator::{
    BufferSpec, VideoBuffer, VideoBufferAllocator, VideoBufferAllocatorProvider,
};
use crate::SdkError;
use cudarc::driver::CudaContext;
use std::ffi::c_void;
use std::sync::Arc;

/// A video buffer backed by CUDA pinned (page-locked) host memory.
///
/// DeckLink writes frame data here via DMA. The memory is pinned, so it can
/// also be accessed directly by the GPU without staging through pageable RAM.
pub struct CudaPinnedBuffer {
    /// Pointer to the pinned host memory allocation.
    ptr: *mut c_void,
    /// Size of the allocation in bytes.
    size: usize,
    /// Keep the CUDA context alive for the lifetime of the buffer.
    _ctx: Arc<CudaContext>,
}

// Safety: The pinned memory pointer is valid from any thread.
unsafe impl Send for CudaPinnedBuffer {}
unsafe impl Sync for CudaPinnedBuffer {}

impl CudaPinnedBuffer {
    /// Allocate a new buffer of `size` bytes in CUDA pinned host memory.
    ///
    /// Uses `CU_MEMHOSTALLOC_PORTABLE` so the memory is portable across CUDA
    /// contexts and readable/writable from both host and device sides (unlike
    /// `WRITECOMBINED` which penalises host reads).
    pub fn new(ctx: Arc<CudaContext>, size: usize) -> Result<Self, SdkError> {
        ctx.bind_to_thread().map_err(|_| SdkError::FAIL)?;
        let ptr = unsafe {
            cudarc::driver::result::malloc_host(size, cudarc::driver::sys::CU_MEMHOSTALLOC_PORTABLE)
        }
        .map_err(|_| SdkError::OUTOFMEMORY)?;
        if ptr.is_null() {
            return Err(SdkError::OUTOFMEMORY);
        }
        Ok(Self {
            ptr,
            size,
            _ctx: ctx,
        })
    }

    /// Get a raw pointer to the pinned memory.
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr as *mut u8
    }

    /// Get the size of the buffer in bytes.
    pub fn len(&self) -> usize {
        self.size
    }

    /// Returns true if the buffer has zero size.
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
}

impl Drop for CudaPinnedBuffer {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            let _ = self._ctx.bind_to_thread();
            unsafe {
                let _ = cudarc::driver::result::free_host(self.ptr);
            }
            self.ptr = std::ptr::null_mut();
        }
    }
}

impl VideoBuffer for CudaPinnedBuffer {
    fn get_bytes(&self) -> Result<*mut c_void, SdkError> {
        if self.ptr.is_null() {
            Err(SdkError::POINTER)
        } else {
            Ok(self.ptr)
        }
    }
}

/// A video buffer allocator that creates CUDA pinned host memory buffers.
struct CudaPinnedAllocator {
    ctx: Arc<CudaContext>,
    buffer_size: usize,
}

impl VideoBufferAllocator for CudaPinnedAllocator {
    fn allocate(&self) -> Result<Box<dyn VideoBuffer>, SdkError> {
        let buf = CudaPinnedBuffer::new(self.ctx.clone(), self.buffer_size)?;
        Ok(Box::new(buf))
    }
}

/// Allocator provider that creates CUDA pinned (page-locked) host memory
/// buffers for DeckLink video capture.
///
/// When used with [`DecklinkInputDevice::enable_video_input_with_allocator`],
/// incoming video frames are DMA'd directly into pinned memory. This pinned
/// memory can then be efficiently copied to GPU device memory using
/// `cuMemcpyHtoDAsync` or accessed directly via zero-copy if the GPU supports it.
///
/// # Example
///
/// ```no_run
/// use decklink::cuda::CudaAllocatorProvider;
/// use decklink::device::input::DecklinkInputDevice;
/// use cudarc::driver::CudaContext;
/// use std::sync::Arc;
///
/// let ctx = CudaContext::new(0).unwrap();
/// let provider = Arc::new(CudaAllocatorProvider::new(ctx));
/// // input_device.enable_video_input_with_allocator(mode, pixel_format, flags, provider)?;
/// ```
pub struct CudaAllocatorProvider {
    ctx: Arc<CudaContext>,
}

impl CudaAllocatorProvider {
    /// Create a new CUDA allocator provider using the given CUDA context.
    pub fn new(ctx: Arc<CudaContext>) -> Self {
        Self { ctx }
    }
}

impl VideoBufferAllocatorProvider for CudaAllocatorProvider {
    fn get_allocator(&self, spec: BufferSpec) -> Result<Arc<dyn VideoBufferAllocator>, SdkError> {
        Ok(Arc::new(CudaPinnedAllocator {
            ctx: self.ctx.clone(),
            buffer_size: spec.buffer_size as usize,
        }))
    }
}
