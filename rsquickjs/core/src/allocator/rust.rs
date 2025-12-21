use std::{alloc::Layout, mem, ptr};

use super::Allocator;

/// The largest value QuickJS will allocate is a u64;
/// So all allocated memory must have the same alignment is this largest size.
const ALLOC_ALIGN: usize = mem::align_of::<u64>();

#[derive(Copy, Clone)]
#[repr(transparent)]
struct Header {
    size: usize,
}

const fn max(a: usize, b: usize) -> usize {
    if a < b {
        b
    } else {
        a
    }
}

/// Head needs to be at least alloc aligned so all that values after the header are aligned.
const HEADER_SIZE: usize = max(mem::size_of::<Header>(), ALLOC_ALIGN);

#[inline]
fn round_size(size: usize) -> usize {
    size.div_ceil(ALLOC_ALIGN) * ALLOC_ALIGN
}

/// The allocator which uses Rust global allocator
pub struct RustAllocator;

unsafe impl Allocator for RustAllocator {
    fn calloc(&mut self, count: usize, size: usize) -> *mut u8 {
        if count == 0 || size == 0 {
            return ptr::null_mut();
        }

        let total_size = count.checked_mul(size).expect("overflow");
        let total_size = round_size(total_size);

        let alloc_size = HEADER_SIZE + total_size;

        let layout = if let Ok(layout) = Layout::from_size_align(alloc_size, ALLOC_ALIGN) {
            layout
        } else {
            return ptr::null_mut();
        };

        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };

        if ptr.is_null() {
            return ptr::null_mut();
        }

        unsafe {
            ptr.cast::<Header>().write(Header { size: total_size });
            ptr.add(HEADER_SIZE)
        }
    }

    fn alloc(&mut self, size: usize) -> *mut u8 {
        let size = round_size(size);
        let alloc_size = size + HEADER_SIZE;

        let layout = if let Ok(layout) = Layout::from_size_align(alloc_size, ALLOC_ALIGN) {
            layout
        } else {
            return ptr::null_mut();
        };

        let ptr = unsafe { std::alloc::alloc(layout) };

        if ptr.is_null() {
            return ptr::null_mut();
        }

        unsafe {
            ptr.cast::<Header>().write(Header { size });
            ptr.add(HEADER_SIZE)
        }
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        let ptr = ptr.sub(HEADER_SIZE);
        let alloc_size = ptr.cast::<Header>().read().size + HEADER_SIZE;
        let layout = Layout::from_size_align_unchecked(alloc_size, ALLOC_ALIGN);

        std::alloc::dealloc(ptr, layout);
    }

    unsafe fn realloc(&mut self, ptr: *mut u8, new_size: usize) -> *mut u8 {
        let new_size = round_size(new_size);

        let ptr = ptr.sub(HEADER_SIZE);
        let alloc_size = ptr.cast::<Header>().read().size + HEADER_SIZE;

        let layout = Layout::from_size_align_unchecked(alloc_size, ALLOC_ALIGN);

        let new_alloc_size = new_size + HEADER_SIZE;

        let ptr = std::alloc::realloc(ptr, layout, new_alloc_size);

        if ptr.is_null() {
            return ptr::null_mut();
        }

        ptr.cast::<Header>().write(Header { size: new_size });
        ptr.add(HEADER_SIZE)
    }

    unsafe fn usable_size(ptr: *mut u8) -> usize {
        let ptr = ptr.sub(HEADER_SIZE);
        ptr.cast::<Header>().read().size
    }
}

#[cfg(test)]
mod test {
    use super::RustAllocator;
    use crate::{allocator::Allocator, AsyncContext, AsyncRuntime};
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[allow(dead_code)]
    static ALLOC_SIZE: AtomicUsize = AtomicUsize::new(0);
    #[allow(dead_code)]
    struct TestAllocator;
    impl Drop for TestAllocator {
        fn drop(&mut self) {
            let size = ALLOC_SIZE.load(Ordering::Acquire);
            assert_eq!(
                size, 0,
                "Memory leak detected: {} bytes still allocated",
                size
            );
        }
    }

    unsafe impl Allocator for TestAllocator {
        fn alloc(&mut self, size: usize) -> *mut u8 {
            unsafe {
                let res = RustAllocator.alloc(size);
                let old = ALLOC_SIZE.fetch_add(RustAllocator::usable_size(res), Ordering::AcqRel);
                println!(
                    "ALLOC: requested {}, got {}, total allocated: {}",
                    size,
                    RustAllocator::usable_size(res),
                    old + RustAllocator::usable_size(res)
                );
                res
            }
        }

        fn calloc(&mut self, count: usize, size: usize) -> *mut u8 {
            unsafe {
                let res = RustAllocator.calloc(count, size);
                let old = ALLOC_SIZE.fetch_add(RustAllocator::usable_size(res), Ordering::AcqRel);
                println!(
                    "CALLOC: requested {}, got {}, total allocated: {}",
                    count * size,
                    RustAllocator::usable_size(res),
                    old + RustAllocator::usable_size(res)
                );
                res
            }
        }

        unsafe fn dealloc(&mut self, ptr: *mut u8) {
            let old = ALLOC_SIZE.fetch_sub(RustAllocator::usable_size(ptr), Ordering::AcqRel);
            println!(
                "DEALLOC: freed {}, total allocated: {}",
                RustAllocator::usable_size(ptr),
                old - RustAllocator::usable_size(ptr)
            );
            RustAllocator.dealloc(ptr);
        }

        unsafe fn realloc(&mut self, ptr: *mut u8, new_size: usize) -> *mut u8 {
            if !ptr.is_null() {
                let old = ALLOC_SIZE.fetch_sub(RustAllocator::usable_size(ptr), Ordering::AcqRel);
                println!(
                    "REALLOC: freed {}, total allocated: {}",
                    RustAllocator::usable_size(ptr),
                    old - RustAllocator::usable_size(ptr)
                );
            }

            let res = RustAllocator.realloc(ptr, new_size);
            if !res.is_null() {
                let old = ALLOC_SIZE.fetch_add(RustAllocator::usable_size(res), Ordering::AcqRel);
                println!(
                    "REALLOC: allocated {}, total allocated: {}",
                    RustAllocator::usable_size(res),
                    old + RustAllocator::usable_size(res)
                );
            }
            res
        }

        unsafe fn usable_size(ptr: *mut u8) -> usize
        where
            Self: Sized,
        {
            RustAllocator::usable_size(ptr)
        }
    }

    #[tokio::test]
    async fn test_gc_working_correctly() {
        let rt = AsyncRuntime::new_with_alloc(TestAllocator).unwrap();
        let context = AsyncContext::full(&rt).await.unwrap();

        let before = ALLOC_SIZE.load(Ordering::Acquire);

        context.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                for(let i = 0;i < 100_000;i++){
                    // create recursive structure.
                    const a = () => {
                        if(a){
                            return true
                        }
                        return false
                    };
                }
            "#,
            )
            .unwrap();
        });

        let after = ALLOC_SIZE.load(Ordering::Acquire);
        // every object takes atleast a single byte.
        // So the gc must have collected atleast some of the recursive objects if the difference is
        // smaller then number of objects created.
        assert!(after.saturating_sub(before) < 100_000)
    }
}
