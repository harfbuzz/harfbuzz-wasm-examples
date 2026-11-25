// The allocator of the module.
//
// HarfBuzz runs the module in an interpreter and limits the instructions
// a shape() call may run.  The standard library's allocator for this
// target (dlmalloc) costs about 230 of them for each allocation and its
// release.  Here blocks of up to 64 KiB are kept in free lists by size,
// which costs about 20.  The lists are cut from chunks that come from
// the standard allocator, and bigger blocks go to it directly.
//
// The lists keep what was released for as long as the instance of the
// module lives (in HarfBuzz, as long as the font's face), as the standard
// allocator keeps its free memory: nothing is given back, and nothing is
// forgotten between calls.  A released block only serves requests of its
// own size again, and a size is rounded up to a power of two, so the
// module can hold more memory than before: 35 MiB instead of 26 MiB after
// shaping 12,748 characters at once, the most that was seen.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;
use std::ptr::null_mut;

// Blocks are powers of two, from 16 bytes (list 0) to 64 KiB.
const MIN_SHIFT: u32 = 4;
const MAX_SHIFT: u32 = 16;
const LISTS: usize = (MAX_SHIFT - MIN_SHIFT + 1) as usize;
// Every block is aligned to its smallest size.
const ALIGN: usize = 1 << MIN_SHIFT;
// What the standard allocator is asked for at a time, to cut blocks from.
const CHUNK: usize = 1 << 18;

// Blocks are kept by address, not by the pointer that was released: that
// one is only good for the bytes its owner asked for, which can be fewer
// than the block has.  An address turned into a pointer again is good
// for the whole chunk it lies in.
struct Lists {
    // The address of the first free block of each size, or 0.  A free
    // block starts with the address of the next one.
    free: [usize; LISTS],
    // The part of the current chunk that has not been cut up.
    next: usize,
    left: usize,
}

pub struct BySize(UnsafeCell<Lists>);

// The module has one thread.
unsafe impl Sync for BySize {}

#[global_allocator]
static ALLOCATOR: BySize = BySize(UnsafeCell::new(Lists {
    free: [0; LISTS],
    next: 0,
    left: 0,
}));

// The list that serves this layout, if one does.
#[inline(always)]
fn list_for(layout: Layout) -> Option<usize> {
    let size = layout.size();
    if size > 1 << MAX_SHIFT || layout.align() > ALIGN {
        return None;
    }
    // The exponent of the power of two at or above the size is the
    // width of size - 1 in bits.
    let size = if size < ALIGN { ALIGN } else { size };
    Some((usize::BITS - (size - 1).leading_zeros() - MIN_SHIFT) as usize)
}

impl Lists {
    // Cuts a block for an empty list out of the current chunk.
    #[cold]
    #[inline(never)]
    unsafe fn cut(&mut self, list: usize) -> *mut u8 {
        let size = 1usize << (list as u32 + MIN_SHIFT);
        if self.left < size {
            // Too little is left of this chunk.  Put it on the lists,
            // the biggest blocks first, and take a new chunk.
            while self.left >= ALIGN {
                let shift = (usize::BITS - 1 - self.left.leading_zeros()).min(MAX_SHIFT);
                let block = self.next;
                self.next += 1 << shift;
                self.left -= 1 << shift;
                self.release(block, (shift - MIN_SHIFT) as usize);
            }
            let chunk = System.alloc(Layout::from_size_align_unchecked(CHUNK, ALIGN));
            if chunk.is_null() {
                self.left = 0;
                return null_mut();
            }
            self.next = chunk as usize;
            self.left = CHUNK;
        }
        let block = self.next;
        self.next += size;
        self.left -= size;
        block as *mut u8
    }

    #[inline(always)]
    unsafe fn release(&mut self, block: usize, list: usize) {
        *(block as *mut usize) = self.free[list];
        self.free[list] = block;
    }
}

unsafe impl GlobalAlloc for BySize {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match list_for(layout) {
            Some(list) => {
                let lists = &mut *self.0.get();
                let block = lists.free[list];
                if block == 0 {
                    return lists.cut(list);
                }
                lists.free[list] = *(block as *const usize);
                block as *mut u8
            }
            None => System.alloc(layout),
        }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        match list_for(layout) {
            Some(list) => (*self.0.get()).release(ptr as usize, list),
            None => System.dealloc(ptr, layout),
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
        match (list_for(layout), list_for(new_layout)) {
            // The block it has is the block it would get.
            (Some(old), Some(new)) if old == new => ptr as usize as *mut u8,
            (None, None) => System.realloc(ptr, layout, new_size),
            _ => {
                let new = self.alloc(new_layout);
                if !new.is_null() {
                    let size = if layout.size() < new_size { layout.size() } else { new_size };
                    std::ptr::copy_nonoverlapping(ptr, new, size);
                    self.dealloc(ptr, layout);
                }
                new
            }
        }
    }
}
