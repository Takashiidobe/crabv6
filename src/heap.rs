use linked_list_allocator::LockedHeap;

#[global_allocator]
static mut KERNEL_HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

static mut KERNEL_HEAP: [u8; 0x20000] = [0; 0x20000];

/// Initialize the heap allocator.
#[allow(static_mut_refs)]
pub unsafe fn init_kernel_heap() {
    let heap_start = unsafe { KERNEL_HEAP.as_mut_ptr() };
    let heap_size = unsafe { KERNEL_HEAP.len() };
    unsafe { KERNEL_HEAP_ALLOCATOR.lock().init(heap_start, heap_size) };
}
