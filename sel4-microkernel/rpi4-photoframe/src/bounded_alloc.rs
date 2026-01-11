//! # Bounded Bump Allocator
//!
//! A fixed-size heap allocator that prevents memory exhaustion attacks.
//!
//! ## Security Properties
//!
//! 1. **Fixed size**: Cannot allocate beyond the compile-time limit
//! 2. **No fragmentation**: Bump allocator has no fragmentation exploits
//! 3. **Resettable**: Clean slate for each image decode
//! 4. **Observable**: Track usage and detect over-allocation attempts
//!
//! ## Usage
//!
//! ```rust
//! // In your decoder PD:
//! #[global_allocator]
//! static ALLOCATOR: BoundedBumpAllocator<{8 * 1024 * 1024}> =
//!     BoundedBumpAllocator::new();
//!
//! // Reset between photos
//! ALLOCATOR.reset();
//!
//! // Check usage
//! let used = ALLOCATOR.used();
//! let peak = ALLOCATOR.peak();
//! ```

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};

/// Bounded bump allocator with a fixed-size memory pool.
///
/// Generic parameter `N` is the heap size in bytes.
pub struct BoundedBumpAllocator<const N: usize> {
    /// Fixed memory pool
    pool: UnsafeCell<[u8; N]>,
    /// Current allocation offset
    offset: AtomicUsize,
    /// Peak usage (high water mark)
    peak: AtomicUsize,
    /// Number of allocation failures
    failures: AtomicUsize,
    /// Whether any allocation has failed
    oom_occurred: AtomicBool,
}

// Safety: The allocator uses atomic operations for thread safety
unsafe impl<const N: usize> Sync for BoundedBumpAllocator<N> {}

impl<const N: usize> BoundedBumpAllocator<N> {
    /// Create a new bounded allocator.
    ///
    /// The pool is zero-initialized at compile time.
    pub const fn new() -> Self {
        Self {
            pool: UnsafeCell::new([0; N]),
            offset: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            failures: AtomicUsize::new(0),
            oom_occurred: AtomicBool::new(false),
        }
    }

    /// Reset the allocator, freeing all memory.
    ///
    /// Call this between photo decodes to reclaim memory.
    ///
    /// # Safety
    ///
    /// Caller must ensure no references to allocated memory exist.
    pub fn reset(&self) {
        self.offset.store(0, Ordering::Release);
        self.oom_occurred.store(false, Ordering::Release);
    }

    /// Get current memory usage in bytes.
    #[inline]
    pub fn used(&self) -> usize {
        self.offset.load(Ordering::Acquire)
    }

    /// Get peak memory usage in bytes.
    #[inline]
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::Acquire)
    }

    /// Get remaining available memory in bytes.
    #[inline]
    pub fn remaining(&self) -> usize {
        N.saturating_sub(self.used())
    }

    /// Get total capacity in bytes.
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Get number of failed allocation attempts.
    #[inline]
    pub fn failure_count(&self) -> usize {
        self.failures.load(Ordering::Acquire)
    }

    /// Check if any allocation has failed since last reset.
    #[inline]
    pub fn oom_occurred(&self) -> bool {
        self.oom_occurred.load(Ordering::Acquire)
    }

    /// Get usage as a percentage (0-100).
    #[inline]
    pub fn usage_percent(&self) -> u8 {
        ((self.used() as u64 * 100) / N as u64) as u8
    }
}

unsafe impl<const N: usize> GlobalAlloc for BoundedBumpAllocator<N> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        // Retry loop for atomic compare-exchange
        loop {
            let current = self.offset.load(Ordering::Acquire);

            // Align up to required alignment
            let aligned = (current + align - 1) & !(align - 1);
            let new_offset = match aligned.checked_add(size) {
                Some(off) => off,
                None => {
                    // Integer overflow - reject
                    self.failures.fetch_add(1, Ordering::Relaxed);
                    self.oom_occurred.store(true, Ordering::Release);
                    return null_mut();
                }
            };

            // Check if allocation would exceed pool
            if new_offset > N {
                self.failures.fetch_add(1, Ordering::Relaxed);
                self.oom_occurred.store(true, Ordering::Release);
                return null_mut();
            }

            // Try to claim this space atomically
            match self.offset.compare_exchange_weak(
                current,
                new_offset,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    // Update peak if this is a new high
                    let _ = self.peak.fetch_max(new_offset, Ordering::Relaxed);

                    // Return pointer into pool
                    let pool_ptr = self.pool.get() as *mut u8;
                    return pool_ptr.add(aligned);
                }
                Err(_) => {
                    // Another thread modified offset, retry
                    continue;
                }
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator: individual deallocations are no-ops
        // Memory is reclaimed in bulk via reset()
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // Simple implementation: allocate new, copy, don't free old
        // (old memory stays allocated until reset)
        let new_layout = match Layout::from_size_align(new_size, layout.align()) {
            Ok(l) => l,
            Err(_) => return null_mut(),
        };

        let new_ptr = self.alloc(new_layout);
        if !new_ptr.is_null() {
            // Copy old data
            let copy_size = layout.size().min(new_size);
            core::ptr::copy_nonoverlapping(ptr, new_ptr, copy_size);
        }
        new_ptr
    }
}

// ============================================================================
// PRESET ALLOCATOR SIZES
// ============================================================================

/// 4 MB allocator - suitable for small photos (up to ~1000x1000)
pub type SmallAllocator = BoundedBumpAllocator<{ 4 * 1024 * 1024 }>;

/// 8 MB allocator - suitable for HD photos (up to 1920x1080)
pub type MediumAllocator = BoundedBumpAllocator<{ 8 * 1024 * 1024 }>;

/// 16 MB allocator - suitable for 4K photos (up to 3840x2160)
pub type LargeAllocator = BoundedBumpAllocator<{ 16 * 1024 * 1024 }>;

/// 32 MB allocator - suitable for large photos with complex decoding
pub type XLargeAllocator = BoundedBumpAllocator<{ 32 * 1024 * 1024 }>;

// ============================================================================
// STATISTICS FOR DEBUGGING
// ============================================================================

/// Allocator statistics snapshot
#[derive(Debug, Clone, Copy)]
pub struct AllocStats {
    pub used: usize,
    pub peak: usize,
    pub capacity: usize,
    pub failures: usize,
    pub oom_occurred: bool,
}

impl<const N: usize> BoundedBumpAllocator<N> {
    /// Get a snapshot of allocator statistics.
    pub fn stats(&self) -> AllocStats {
        AllocStats {
            used: self.used(),
            peak: self.peak(),
            capacity: N,
            failures: self.failure_count(),
            oom_occurred: self.oom_occurred(),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_allocation() {
        let alloc: BoundedBumpAllocator<1024> = BoundedBumpAllocator::new();

        unsafe {
            let layout = Layout::from_size_align(100, 8).unwrap();
            let ptr = alloc.alloc(layout);
            assert!(!ptr.is_null());
            assert!(alloc.used() >= 100);
        }
    }

    #[test]
    fn test_oom() {
        let alloc: BoundedBumpAllocator<256> = BoundedBumpAllocator::new();

        unsafe {
            // Try to allocate more than capacity
            let layout = Layout::from_size_align(512, 8).unwrap();
            let ptr = alloc.alloc(layout);
            assert!(ptr.is_null());
            assert!(alloc.oom_occurred());
            assert_eq!(alloc.failure_count(), 1);
        }
    }

    #[test]
    fn test_reset() {
        let alloc: BoundedBumpAllocator<1024> = BoundedBumpAllocator::new();

        unsafe {
            let layout = Layout::from_size_align(100, 8).unwrap();
            let _ = alloc.alloc(layout);
            assert!(alloc.used() > 0);

            alloc.reset();
            assert_eq!(alloc.used(), 0);
            assert!(!alloc.oom_occurred());
        }
    }

    #[test]
    fn test_alignment() {
        let alloc: BoundedBumpAllocator<1024> = BoundedBumpAllocator::new();

        unsafe {
            // Allocate with different alignments
            let l1 = Layout::from_size_align(1, 1).unwrap();
            let p1 = alloc.alloc(l1);
            assert!(!p1.is_null());

            let l2 = Layout::from_size_align(1, 64).unwrap();
            let p2 = alloc.alloc(l2);
            assert!(!p2.is_null());
            assert_eq!(p2 as usize % 64, 0);  // Should be 64-byte aligned
        }
    }
}
