//! The return addresses on the stack, for crash reports.
//!
//! The standard library's `Backtrace` gives function names when the binary
//! has symbols, and nothing useful when it is stripped, as in the boot image.
//! Crash reports need the bare addresses instead, which a developer resolves
//! later with the debug file (`cargo xtask symbolize`). They come from the
//! unwinder that the standard library links in for panics, through its C
//! interface `_Unwind_Backtrace` (libgcc with glibc, LLVM's libunwind with
//! musl).

use std::ffi::{c_int, c_void};

/// The unwinder's view of one frame; only its address is used.
#[repr(C)]
struct UnwindContext {
    /// The type is opaque.
    _opaque: [u8; 0],
}

/// `_URC_NO_REASON`: go on to the next frame.
const NEXT: c_int = 0;

/// `_URC_END_OF_STACK`: stop.
const STOP: c_int = 5;

/// The callback `_Unwind_Backtrace` calls for each frame.
type Callback = extern "C" fn(*mut UnwindContext, *mut c_void) -> c_int;

unsafe extern "C" {
    fn _Unwind_Backtrace(callback: Callback, argument: *mut c_void) -> c_int;
    fn _Unwind_GetIP(context: *mut UnwindContext) -> usize;
}

/// The most frames a report holds; deeper stacks are cut, so a runaway
/// recursion still gives a report of reasonable size.
pub const MAX_FRAMES: usize = 100;

/// Records one frame's address in the `Vec<usize>` that `argument` points to.
extern "C" fn record(context: *mut UnwindContext, argument: *mut c_void) -> c_int {
    // SAFETY: `return_addresses` passes a pointer to its own vector, which
    // lives until `_Unwind_Backtrace` returns, and nothing else uses it
    // meanwhile.
    let frames = unsafe { &mut *argument.cast::<Vec<usize>>() };
    // SAFETY: the unwinder passes a context that is valid during the call.
    let address = unsafe { _Unwind_GetIP(context) };
    if address == 0 || frames.len() >= MAX_FRAMES {
        return STOP;
    }
    // The capacity was reserved, so this does not allocate: a crash report
    // may be written after the allocator failed.
    frames.push(address);
    NEXT
}

/// The return addresses of the calling thread's stack, innermost first, at
/// most [`MAX_FRAMES`]. The first is inside this function's caller.
pub fn return_addresses() -> Vec<usize> {
    let mut frames: Vec<usize> = Vec::with_capacity(MAX_FRAMES);
    // SAFETY: `record` is the only user of the pointer, during this call,
    // and it never unwinds (it cannot panic: no indexing, and the push stays
    // within the reserved capacity).
    unsafe {
        _Unwind_Backtrace(record, (&raw mut frames).cast());
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[inline(never)]
    fn nested(depth: usize) -> Vec<usize> {
        if depth == 0 {
            return_addresses()
        } else {
            let frames = nested(depth - 1);
            std::hint::black_box(frames)
        }
    }

    #[test]
    fn deeper_calls_have_more_frames() {
        let shallow = nested(0);
        let deep = nested(5);

        assert!(shallow.len() > 2, "{shallow:x?}");
        assert!(deep.len() >= shallow.len() + 5, "{shallow:x?} {deep:x?}");
        assert!(deep.iter().all(|address| *address != 0));
    }

    #[test]
    fn the_stack_is_cut_at_the_limit() {
        fn recurse(depth: usize) -> usize {
            if depth == 0 {
                return_addresses().len()
            } else {
                std::hint::black_box(recurse(depth - 1))
            }
        }

        assert_eq!(recurse(MAX_FRAMES + 20), MAX_FRAMES);
    }
}
