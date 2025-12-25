use linked_list_allocator::LockedHeap;

#[global_allocator]
static mut KERNEL_HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

// Increased heap size to 2MB for multitasking support
// Each process needs 128KB memory snapshot, plus overhead for process structs, pipes, etc.
static mut KERNEL_HEAP: [u8; 0x200000] = [0; 0x200000];

/// Initialize the heap allocator.
#[allow(static_mut_refs)]
pub unsafe fn init_kernel_heap() {
    let heap_start = unsafe { KERNEL_HEAP.as_mut_ptr() };
    let heap_size = unsafe { KERNEL_HEAP.len() };
    unsafe { KERNEL_HEAP_ALLOCATOR.lock().init(heap_start, heap_size) };
}
