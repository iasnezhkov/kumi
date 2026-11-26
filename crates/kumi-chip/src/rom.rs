// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
pub const SF_INIT: usize = 0x0008_2356;
pub const SF_DEINIT: usize = 0x0008_236E;
pub const SF_ERASE: usize = 0x0008_213C;
pub const SF_WRITE: usize = 0x0008_2200;
pub const SF_READ: usize = 0x0008_225E;
pub const SF_JEDEC: usize = 0x0008_20C6;
pub const BROM_FUNC_QUAD_SLOT: usize = 0x0008_00A8;
pub const NORCTRL_QUAD_SPI_INIT: usize = 0x0008_1A82;
pub const USB_CLOCK_SOURCE: usize = 0x0008_272E;
pub const USB_PLL: usize = 0x0008_27EC;
pub const PIN_GROUP: usize = 0;
pub const USB_SOURCE_HZ: u32 = 240_000;

pub unsafe fn sf_init() {
    let f = unsafe { core::mem::transmute::<usize, extern "C" fn(usize)>(SF_INIT) };
    f(PIN_GROUP)
}

pub unsafe fn sf_deinit() {
    let f = unsafe { core::mem::transmute::<usize, extern "C" fn(usize)>(SF_DEINIT) };
    f(PIN_GROUP)
}

pub unsafe fn sf_read(offset: u32, buffer: &mut [u8]) {
    let f = unsafe { core::mem::transmute::<usize, extern "C" fn(u32, *mut u8, usize)>(SF_READ) };
    f(offset, buffer.as_mut_ptr(), buffer.len())
}

pub unsafe fn sf_write(offset: u32, data: &[u8]) {
    let f = unsafe { core::mem::transmute::<usize, extern "C" fn(u32, *const u8, usize)>(SF_WRITE) };
    f(offset, data.as_ptr(), data.len())
}

pub unsafe fn sf_erase(offset: u32, len: u32) {
    let f = unsafe { core::mem::transmute::<usize, extern "C" fn(u32, u32)>(SF_ERASE) };
    f(offset, len)
}

pub unsafe fn sf_jedec() -> u32 {
    let f = unsafe { core::mem::transmute::<usize, extern "C" fn() -> u32>(SF_JEDEC) };
    f()
}

pub unsafe fn usb_clock_source(hz: u32) -> i32 {
    let f = unsafe { core::mem::transmute::<usize, extern "C" fn(u32) -> i32>(USB_CLOCK_SOURCE) };
    f(hz)
}

pub unsafe fn usb_pll(source_ready: u32) -> i32 {
    let f = unsafe { core::mem::transmute::<usize, extern "C" fn(u32) -> i32>(USB_PLL) };
    f(source_ready)
}

pub unsafe fn has_quad_spi_init() -> bool {
    unsafe { core::ptr::read_volatile(BROM_FUNC_QUAD_SLOT as *const u32) == NORCTRL_QUAD_SPI_INIT as u32 }
}

pub unsafe fn norctrl_quad_spi_init(boot_sel: u32, flash_type: u32, freq_hz: u64) -> u32 {
    let f = unsafe { core::mem::transmute::<usize, extern "C" fn(u32, u32, u64) -> u32>(NORCTRL_QUAD_SPI_INIT) };
    f(boot_sel, flash_type, freq_hz)
}
