use core::sync::atomic::{Ordering, fence};
use core::{hint::spin_loop, mem::size_of, ptr};

use spin::Mutex;

pub mod block {
    use const_default::ConstDefault;

    use super::*;

    const VIRTIO_MMIO_BASE: usize = 0x1000_1000;
    const QUEUE_SIZE: usize = 8;
    const SECTOR_SIZE: usize = 512;

    const MAGIC_VALUE: usize = 0x000;
    const VERSION: usize = 0x004;
    const DEVICE_ID: usize = 0x008;
    const DEVICE_FEATURES: usize = 0x010;
    const DEVICE_FEATURES_SEL: usize = 0x014;
    const DRIVER_FEATURES: usize = 0x020;
    const DRIVER_FEATURES_SEL: usize = 0x024;
    const QUEUE_SEL: usize = 0x030;
    const QUEUE_NUM_MAX: usize = 0x034;
    const QUEUE_NUM: usize = 0x038;
    const QUEUE_READY: usize = 0x044;
    const QUEUE_NOTIFY: usize = 0x050;
    const INTERRUPT_STATUS: usize = 0x060;
    const INTERRUPT_ACK: usize = 0x064;
    const STATUS: usize = 0x070;
    const QUEUE_DESC_LOW: usize = 0x080;
    const QUEUE_DESC_HIGH: usize = 0x084;
    const QUEUE_AVAIL_LOW: usize = 0x090;
    const QUEUE_AVAIL_HIGH: usize = 0x094;
    const QUEUE_USED_LOW: usize = 0x0a0;
    const QUEUE_USED_HIGH: usize = 0x0a4;
    const CONFIG_GENERATION: usize = 0x0fc;
    const CONFIG_OFFSET: usize = 0x100;

    const STATUS_ACKNOWLEDGE: u32 = 1;
    const STATUS_DRIVER: u32 = 2;
    const STATUS_FEATURES_OK: u32 = 8;
    const STATUS_DRIVER_OK: u32 = 4;

    const VIRTIO_F_VERSION_1_BIT: u32 = 0;

    static DEVICE: Mutex<Option<VirtIoBlock>> = Mutex::new(None);

    #[repr(C)]
    #[derive(ConstDefault, Clone, Copy)]
    struct VirtqDesc {
        addr: u64,
        len: u32,
        flags: u16,
        next: u16,
    }

    #[repr(C, align(2))]
    struct VirtqAvail {
        flags: u16,
        idx: u16,
        ring: [u16; QUEUE_SIZE],
    }

    impl VirtqAvail {
        const fn new() -> Self {
            Self {
                flags: 0,
                idx: 0,
                ring: [0; QUEUE_SIZE],
            }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct VirtqUsedElem {
        id: u32,
        len: u32,
    }

    impl VirtqUsedElem {
        const fn new() -> Self {
            Self { id: 0, len: 0 }
        }
    }

    #[repr(C, align(4096))]
    struct VirtqUsed {
        flags: u16,
        idx: u16,
        ring: [VirtqUsedElem; QUEUE_SIZE],
    }

    impl VirtqUsed {
        const fn new() -> Self {
            Self {
                flags: 0,
                idx: 0,
                ring: [VirtqUsedElem::new(); QUEUE_SIZE],
            }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct VirtioBlkReqHeader {
        ty: u32,
        reserved: u32,
        sector: u64,
    }

    impl VirtioBlkReqHeader {
        const fn new() -> Self {
            Self {
                ty: 0,
                reserved: 0,
                sector: 0,
            }
        }
    }

    #[allow(dead_code)]
    enum RequestType {
        In = 0,
        Out = 1,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct VirtioBlockGeometry {
        cylinders: u16,
        heads: u8,
        sectors: u8,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct VirtioBlockTopology {
        physical_block_exp: u8,
        alignment_offset: u8,
        min_io_size: u16,
        opt_io_size: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct VirtioBlockConfig {
        capacity: u64,
        size_max: u32,
        seg_max: u32,
        geometry: VirtioBlockGeometry,
        blk_size: u32,
        topology: VirtioBlockTopology,
        writeback: u8,
        unused0: u8,
        unused1: u16,
        max_discard_sectors: u32,
        max_discard_seg: u32,
        discard_sector_alignment: u32,
        max_write_zeroes_sectors: u32,
        max_write_zeroes_seg: u32,
        write_zeroes_may_unmap: u8,
        unused2: [u8; 3],
        max_secure_erase_sectors: u32,
        max_secure_erase_seg: u32,
        secure_erase_sector_alignment: u32,
    }

    impl VirtioBlockConfig {
        fn sector_capacity(&self) -> u64 {
            self.capacity
        }

        fn block_size(&self) -> u32 {
            if self.blk_size == 0 {
                SECTOR_SIZE as u32
            } else {
                self.blk_size
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VirtioError {
        DeviceNotFound,
        UnsupportedDevice,
        LegacyOnly(u32),
        QueueUnavailable,
        DeviceRejectedFeatures,
        DeviceFailure,
    }

    #[derive(Clone, Copy)]
    pub struct VirtIoBlock {
        regs_base: usize,
        capacity_sectors: u64,
        queue_size: u16,
    }

    impl VirtIoBlock {
        pub fn total_blocks(&self) -> u32 {
            self.capacity_sectors.min(u32::MAX as u64) as u32
        }

        pub fn read_block(&self, index: u32, buf: &mut [u8]) {
            self.transfer(index, buf.as_mut_ptr(), buf.len(), RequestType::In);
        }

        pub fn write_block(&self, index: u32, buf: &[u8]) {
            self.transfer(index, buf.as_ptr() as *mut u8, buf.len(), RequestType::Out);
        }

        fn transfer(&self, index: u32, buffer: *mut u8, len: usize, request: RequestType) {
            assert!(len >= SECTOR_SIZE);
            assert!((index as u64) < self.capacity_sectors);

            let mut queue = QUEUE_STATE.lock();
            unsafe {
                let header_ptr = ptr::addr_of_mut!(REQUEST_HEADER);
                (*header_ptr).ty = match request {
                    RequestType::In => 0,
                    RequestType::Out => 1,
                };
                (*header_ptr).reserved = 0;
                (*header_ptr).sector = index as u64;
                ptr::write(ptr::addr_of_mut!(REQUEST_STATUS), 0xFF);

                let desc0 = ptr::addr_of_mut!(VIRTQ_DESC[0]);
                (*desc0).addr = ptr::addr_of!(REQUEST_HEADER) as u64;
                (*desc0).len = size_of::<VirtioBlkReqHeader>() as u32;
                (*desc0).flags = VIRTQ_DESC_F_NEXT;
                (*desc0).next = 1;

                let desc1 = ptr::addr_of_mut!(VIRTQ_DESC[1]);
                (*desc1).addr = buffer as u64;
                (*desc1).len = SECTOR_SIZE as u32;
                (*desc1).flags = VIRTQ_DESC_F_NEXT
                    | match request {
                        RequestType::In => VIRTQ_DESC_F_WRITE,
                        RequestType::Out => 0,
                    };
                (*desc1).next = 2;

                let desc2 = ptr::addr_of_mut!(VIRTQ_DESC[2]);
                (*desc2).addr = ptr::addr_of!(REQUEST_STATUS) as u64;
                (*desc2).len = 1;
                (*desc2).flags = VIRTQ_DESC_F_WRITE;
                (*desc2).next = 0;

                let avail_ptr = ptr::addr_of_mut!(VIRTQ_AVAIL);
                let slot = (queue.next_avail as usize) % (self.queue_size as usize);
                (*avail_ptr).ring[slot] = 0;
                fence(Ordering::Release);
                queue.next_avail = queue.next_avail.wrapping_add(1);
                (*avail_ptr).idx = queue.next_avail;

                fence(Ordering::SeqCst);
                write32(self.regs_base, QUEUE_NOTIFY, 0);

                let expected = queue.last_used.wrapping_add(1);
                loop {
                    fence(Ordering::Acquire);
                    if ptr::read_volatile(ptr::addr_of!(VIRTQ_USED.idx)) == expected {
                        break;
                    }
                    spin_loop();
                }
                queue.last_used = expected;

                let status = ptr::read_volatile(ptr::addr_of!(REQUEST_STATUS));
                if status != 0 {
                    panic!("virtio block request failed with status {}", status);
                }

                let interrupt_status = read32(self.regs_base, INTERRUPT_STATUS);
                if interrupt_status != 0 {
                    write32(self.regs_base, INTERRUPT_ACK, interrupt_status);
                }
            }
        }
    }

    const VIRTQ_DESC_F_NEXT: u16 = 1;
    const VIRTQ_DESC_F_WRITE: u16 = 2;

    struct VirtQueueState {
        next_avail: u16,
        last_used: u16,
    }

    impl VirtQueueState {
        const fn new() -> Self {
            Self {
                next_avail: 0,
                last_used: 0,
            }
        }
    }

    static mut VIRTQ_DESC: [VirtqDesc; QUEUE_SIZE] = [VirtqDesc::DEFAULT; QUEUE_SIZE];
    static mut VIRTQ_AVAIL: VirtqAvail = VirtqAvail::new();
    static mut VIRTQ_USED: VirtqUsed = VirtqUsed::new();
    static mut REQUEST_HEADER: VirtioBlkReqHeader = VirtioBlkReqHeader::new();
    static mut REQUEST_STATUS: u8 = 0;
    static QUEUE_STATE: Mutex<VirtQueueState> = Mutex::new(VirtQueueState::new());

    pub fn init() -> Result<VirtIoBlock, VirtioError> {
        let mut guard = DEVICE.lock();
        if let Some(device) = *guard {
            return Ok(device);
        }
        let device = unsafe { initialize()? };
        *guard = Some(device);
        Ok(device)
    }

    unsafe fn initialize() -> Result<VirtIoBlock, VirtioError> {
        if read32(VIRTIO_MMIO_BASE, MAGIC_VALUE) != 0x7472_6976 {
            return Err(VirtioError::DeviceNotFound);
        }
        let version = read32(VIRTIO_MMIO_BASE, VERSION);
        if version != 2 {
            return Err(VirtioError::LegacyOnly(version));
        }
        if read32(VIRTIO_MMIO_BASE, DEVICE_ID) != 2 {
            return Err(VirtioError::UnsupportedDevice);
        }

        write32(VIRTIO_MMIO_BASE, STATUS, 0);
        write32(VIRTIO_MMIO_BASE, STATUS, STATUS_ACKNOWLEDGE);
        write32(VIRTIO_MMIO_BASE, STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        write32(VIRTIO_MMIO_BASE, DEVICE_FEATURES_SEL, 0);
        let device_features_lo = read32(VIRTIO_MMIO_BASE, DEVICE_FEATURES);
        let driver_features_lo = device_features_lo & SUPPORTED_FEATURES_LO;
        write32(VIRTIO_MMIO_BASE, DRIVER_FEATURES_SEL, 0);
        write32(VIRTIO_MMIO_BASE, DRIVER_FEATURES, driver_features_lo);

        write32(VIRTIO_MMIO_BASE, DEVICE_FEATURES_SEL, 1);
        let device_features_hi = read32(VIRTIO_MMIO_BASE, DEVICE_FEATURES);
        let mut driver_features_hi = 0u32;
        if (device_features_hi & (1 << VIRTIO_F_VERSION_1_BIT)) != 0 {
            driver_features_hi |= 1 << VIRTIO_F_VERSION_1_BIT;
        }
        write32(VIRTIO_MMIO_BASE, DRIVER_FEATURES_SEL, 1);
        write32(VIRTIO_MMIO_BASE, DRIVER_FEATURES, driver_features_hi);

        write32(
            VIRTIO_MMIO_BASE,
            STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK,
        );
        if (read32(VIRTIO_MMIO_BASE, STATUS) & STATUS_FEATURES_OK) == 0 {
            return Err(VirtioError::DeviceRejectedFeatures);
        }

        write32(VIRTIO_MMIO_BASE, QUEUE_SEL, 0);
        let queue_max = read32(VIRTIO_MMIO_BASE, QUEUE_NUM_MAX);
        if queue_max == 0 {
            return Err(VirtioError::QueueUnavailable);
        }
        let queue_size = core::cmp::min(queue_max as usize, QUEUE_SIZE) as u16;
        write32(VIRTIO_MMIO_BASE, QUEUE_NUM, queue_size as u32);

        zero_queue_memory();

        let desc_addr = ptr::addr_of!(VIRTQ_DESC) as usize;
        let avail_addr = ptr::addr_of!(VIRTQ_AVAIL) as usize;
        let used_addr = ptr::addr_of!(VIRTQ_USED) as usize;

        write64(
            VIRTIO_MMIO_BASE,
            QUEUE_DESC_LOW,
            QUEUE_DESC_HIGH,
            desc_addr as u64,
        );
        write64(
            VIRTIO_MMIO_BASE,
            QUEUE_AVAIL_LOW,
            QUEUE_AVAIL_HIGH,
            avail_addr as u64,
        );
        write64(
            VIRTIO_MMIO_BASE,
            QUEUE_USED_LOW,
            QUEUE_USED_HIGH,
            used_addr as u64,
        );

        write32(VIRTIO_MMIO_BASE, QUEUE_READY, 1);

        let config_generation = read32(VIRTIO_MMIO_BASE, CONFIG_GENERATION);
        let config = read_config();
        let block_size = config.block_size();
        if block_size as usize != SECTOR_SIZE {
            panic!("unsupported block size: {}", block_size);
        }
        let capacity_sectors = config.sector_capacity();
        let config_generation_after = read32(VIRTIO_MMIO_BASE, CONFIG_GENERATION);
        if config_generation != config_generation_after {
            return Err(VirtioError::DeviceFailure);
        }

        write32(
            VIRTIO_MMIO_BASE,
            STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
        );

        Ok(VirtIoBlock {
            regs_base: VIRTIO_MMIO_BASE,
            capacity_sectors,
            queue_size,
        })
    }

    const SUPPORTED_FEATURES_LO: u32 = 0;

    fn zero_queue_memory() {
        unsafe {
            let desc_base = ptr::addr_of_mut!(VIRTQ_DESC) as *mut VirtqDesc;
            for i in 0..QUEUE_SIZE {
                ptr::write(desc_base.add(i), VirtqDesc::DEFAULT);
            }
            ptr::write(ptr::addr_of_mut!(VIRTQ_AVAIL), VirtqAvail::new());
            ptr::write(ptr::addr_of_mut!(VIRTQ_USED), VirtqUsed::new());
            ptr::write(ptr::addr_of_mut!(REQUEST_HEADER), VirtioBlkReqHeader::new());
            ptr::write(ptr::addr_of_mut!(REQUEST_STATUS), 0);
        }
        let mut state = QUEUE_STATE.lock();
        *state = VirtQueueState::new();
    }

    fn read_config() -> VirtioBlockConfig {
        unsafe {
            ptr::read_volatile((VIRTIO_MMIO_BASE + CONFIG_OFFSET) as *const VirtioBlockConfig)
        }
    }

    fn read32(base: usize, offset: usize) -> u32 {
        unsafe { ptr::read_volatile((base + offset) as *const u32) }
    }

    fn write32(base: usize, offset: usize, value: u32) {
        unsafe { ptr::write_volatile((base + offset) as *mut u32, value) };
    }

    fn write64(base: usize, low_offset: usize, high_offset: usize, value: u64) {
        write32(base, low_offset, value as u32);
        write32(base, high_offset, (value >> 32) as u32);
    }
}
