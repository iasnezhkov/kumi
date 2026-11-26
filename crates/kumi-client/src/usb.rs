// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::io::{self, ErrorKind};

use kumi_icd::{USB_PRODUCT_ID, USB_VENDOR_ID, VENDOR_INTERFACE_CLASS, VENDOR_INTERFACE_PROTOCOL, VENDOR_INTERFACE_SUBCLASS};

use crate::bridge::DEVICE_DESCRIPTOR;

pub const OUT_TIMEOUT_MS: u32 = 3_000;
pub const BROM_VENDOR_ID: u16 = 0x0666;
pub const BROM_PRODUCT_ID: u16 = 0x0888;
pub const STOCK_VENDOR_ID: u16 = 0x0661;
pub const STOCK_PRODUCT_ID: u16 = 0x0881;
pub const BROM_PACKET: usize = 512;
pub const BROM_IN_EP: u8 = 0x81;
pub const BROM_OUT_EP: u8 = 0x02;
pub const BROM_TIMEOUT_MS: u32 = 1_500;
pub const BROM_ERASE_TIMEOUT_MS: u32 = 20_000;
const BROM_ERASE_COMMAND: u16 = 0x0120;

pub fn speed_name(speed: u8) -> &'static str {
    match speed {
        1 => "low",
        2 => "full",
        3 => "high",
        4 => "super",
        5 => "super+",
        _ => "unknown",
    }
}

pub fn device_mode(vendor: u16, product: u16) -> Option<&'static str> {
    match (vendor, product) {
        (USB_VENDOR_ID, USB_PRODUCT_ID) => Some("kumi"),
        (BROM_VENDOR_ID, BROM_PRODUCT_ID) => Some("brom"),
        (STOCK_VENDOR_ID, STOCK_PRODUCT_ID) => Some("stock"),
        _ => None,
    }
}

pub fn brom_reply_timeout_ms(packet: &[u8]) -> u32 {
    if packet.len() >= 6 && u16::from_le_bytes([packet[4], packet[5]]) == BROM_ERASE_COMMAND {
        BROM_ERASE_TIMEOUT_MS
    } else {
        BROM_TIMEOUT_MS
    }
}

pub fn is_brom_device_descriptor(descriptor: &[u8]) -> bool {
    descriptor.len() >= DEVICE_DESCRIPTOR
        && descriptor[0] == DEVICE_DESCRIPTOR as u8
        && descriptor[1] == 1
        && u16::from_le_bytes([descriptor[8], descriptor[9]]) == BROM_VENDOR_ID
        && u16::from_le_bytes([descriptor[10], descriptor[11]]) == BROM_PRODUCT_ID
}

pub fn parse_brom_config(config: &[u8]) -> Option<(u8, u8, u8)> {
    if config.len() < 9 || config[1] != 2 {
        return None;
    }
    let total = usize::from(u16::from_le_bytes([config[2], config[3]]));
    if total < 9 || total > config.len() {
        return None;
    }
    let mut index = 0;
    let mut selected = false;
    let mut found = false;
    let mut endpoints = 0u8;
    while index + 2 <= total {
        let length = usize::from(config[index]);
        let kind = config[index + 1];
        if length < 2 || index + length > total {
            return None;
        }
        if kind == 4 && length >= 9 {
            selected = config[index + 2] == 0 && config[index + 3] == 0 && config[index + 5] == 3;
            if selected {
                if found {
                    return None;
                }
                found = true;
            }
        } else if kind == 5 && length >= 7 && selected {
            let max = u16::from_le_bytes([config[index + 4], config[index + 5]]) & 0x7ff;
            if config[index + 3] & 3 != 3 || max == 0 || max > 1024 {
                return None;
            }
            match config[index + 2] {
                BROM_IN_EP => endpoints |= 1,
                BROM_OUT_EP => endpoints |= 2,
                _ => {}
            }
        }
        index += length;
    }
    (found && endpoints == 3).then_some((0, BROM_IN_EP, BROM_OUT_EP))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KumiEndpoints {
    pub interface: u8,
    pub in_ep: u8,
    pub out_ep: u8,
    pub in_max: u16,
    pub out_max: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbDeviceInfo {
    pub bus: u8,
    pub address: u8,
    pub vendor: u16,
    pub product: u16,
    pub speed: u8,
}

pub fn is_kumi_device_descriptor(descriptor: &[u8]) -> bool {
    descriptor.len() >= DEVICE_DESCRIPTOR
        && descriptor[0] == DEVICE_DESCRIPTOR as u8
        && descriptor[1] == 1
        && u16::from_le_bytes([descriptor[8], descriptor[9]]) == USB_VENDOR_ID
        && u16::from_le_bytes([descriptor[10], descriptor[11]]) == USB_PRODUCT_ID
}

pub fn parse_kumi_config(config: &[u8]) -> Option<KumiEndpoints> {
    if config.len() < 9 || config[1] != 2 {
        return None;
    }
    let total = usize::from(u16::from_le_bytes([config[2], config[3]]));
    if total < 9 || total > config.len() {
        return None;
    }
    let mut index = 0;
    let mut selected = false;
    let mut found = false;
    let mut interface = 0;
    let mut in_ep = 0;
    let mut out_ep = 0;
    let mut in_max = 0;
    let mut out_max = 0;
    while index + 2 <= total {
        let length = usize::from(config[index]);
        let kind = config[index + 1];
        if length < 2 || index + length > total {
            return None;
        }
        if kind == 4 && length >= 9 {
            selected = config[index + 3] == 0
                && config[index + 5] == VENDOR_INTERFACE_CLASS
                && config[index + 6] == VENDOR_INTERFACE_SUBCLASS
                && config[index + 7] == VENDOR_INTERFACE_PROTOCOL;
            if selected {
                if found {
                    return None;
                }
                found = true;
                interface = config[index + 2];
            }
        } else if kind == 5 && length >= 7 && selected {
            let address = config[index + 2];
            let max = u16::from_le_bytes([config[index + 4], config[index + 5]]) & 0x7ff;
            if config[index + 3] & 3 != 2 || max == 0 {
                return None;
            }
            if address & 0x80 != 0 {
                if in_ep != 0 {
                    return None;
                }
                in_ep = address;
                in_max = max;
            } else {
                if out_ep != 0 {
                    return None;
                }
                out_ep = address;
                out_max = max;
            }
        }
        index += length;
    }
    if found && in_ep != 0 && out_ep != 0 { Some(KumiEndpoints { interface, in_ep, out_ep, in_max, out_max }) } else { None }
}

#[cfg(all(unix, not(target_os = "android")))]
mod native {
    const CONTROL_TIMEOUT_MS: u32 = 1_000;
    const IN_POLL_MS: u32 = 100;
    use std::ffi::{CStr, CString, c_char, c_int, c_uchar, c_uint, c_void};
    use std::io::{self, ErrorKind};
    use std::ptr;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use kumi_icd::{USB_PRODUCT_ID, USB_VENDOR_ID};

    #[cfg(not(target_os = "macos"))]
    use super::{BROM_IN_EP, BROM_OUT_EP, BROM_TIMEOUT_MS, is_brom_device_descriptor, parse_brom_config};
    use super::{
        BROM_PACKET, BROM_PRODUCT_ID, BROM_VENDOR_ID, KumiEndpoints, OUT_TIMEOUT_MS, UsbDeviceInfo, brom_reply_timeout_ms,
        device_mode, is_kumi_device_descriptor, parse_kumi_config,
    };
    use crate::bridge::{DEVICE_DESCRIPTOR, MAX_FRAME, POLL, REPLY_TIMEOUT};
    use crate::wire::FrameLink;

    const RTLD_NOW: c_int = 2;
    const LIBUSB_ERROR_TIMEOUT: c_int = -7;

    #[repr(C)]
    #[derive(Default)]
    struct LibusbDeviceDescriptor {
        length: u8,
        descriptor_type: u8,
        bcd_usb: u16,
        device_class: u8,
        device_sub_class: u8,
        device_protocol: u8,
        max_packet_size0: u8,
        id_vendor: u16,
        id_product: u16,
        bcd_device: u16,
        manufacturer: u8,
        product: u8,
        serial_number: u8,
        num_configurations: u8,
    }

    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
    }

    type FnInit = unsafe extern "C" fn(*mut *mut c_void) -> c_int;
    type FnExit = unsafe extern "C" fn(*mut c_void);
    type FnGetDeviceList = unsafe extern "C" fn(*mut c_void, *mut *mut *mut c_void) -> isize;
    type FnFreeDeviceList = unsafe extern "C" fn(*mut *mut c_void, c_int);
    type FnGetDeviceDescriptor = unsafe extern "C" fn(*mut c_void, *mut LibusbDeviceDescriptor) -> c_int;
    type FnGetBusNumber = unsafe extern "C" fn(*mut c_void) -> u8;
    type FnGetDeviceAddress = unsafe extern "C" fn(*mut c_void) -> u8;
    type FnGetDeviceSpeed = unsafe extern "C" fn(*mut c_void) -> c_int;
    type FnOpen = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int;
    type FnClose = unsafe extern "C" fn(*mut c_void);
    type FnControlTransfer = unsafe extern "C" fn(*mut c_void, u8, u8, u16, u16, *mut c_uchar, u16, c_uint) -> c_int;
    type FnClaimInterface = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
    type FnReleaseInterface = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
    type FnBulkTransfer = unsafe extern "C" fn(*mut c_void, u8, *mut c_uchar, c_int, *mut c_int, c_uint) -> c_int;
    type FnErrorName = unsafe extern "C" fn(c_int) -> *const c_char;
    type FnAutoDetach = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;

    struct LibUsb {
        lib: *mut c_void,
        init: FnInit,
        exit: FnExit,
        get_device_list: FnGetDeviceList,
        free_device_list: FnFreeDeviceList,
        get_device_descriptor: FnGetDeviceDescriptor,
        get_bus_number: FnGetBusNumber,
        get_device_address: FnGetDeviceAddress,
        get_device_speed: FnGetDeviceSpeed,
        open: FnOpen,
        close: FnClose,
        control_transfer: FnControlTransfer,
        claim_interface: FnClaimInterface,
        release_interface: FnReleaseInterface,
        bulk_transfer: FnBulkTransfer,
        #[cfg(not(target_os = "macos"))]
        interrupt_transfer: FnBulkTransfer,
        error_name: FnErrorName,
        set_auto_detach: Option<FnAutoDetach>,
    }

    unsafe impl Send for LibUsb {}
    unsafe impl Sync for LibUsb {}

    impl Drop for LibUsb {
        fn drop(&mut self) {
            unsafe {
                dlclose(self.lib);
            }
        }
    }

    fn candidate_paths() -> Vec<String> {
        let mut paths = Vec::new();
        if let Ok(path) = std::env::var("KUMI_LIBUSB")
            && !path.is_empty()
        {
            paths.push(path);
        }
        #[cfg(target_os = "macos")]
        {
            paths.extend(
                [
                    "libusb-1.0.dylib",
                    "/opt/homebrew/opt/libusb/lib/libusb-1.0.dylib",
                    "/opt/homebrew/lib/libusb-1.0.dylib",
                    "/usr/local/opt/libusb/lib/libusb-1.0.dylib",
                    "/usr/local/lib/libusb-1.0.dylib",
                ]
                .into_iter()
                .map(String::from),
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            paths.extend(["libusb-1.0.so.0", "libusb-1.0.so"].into_iter().map(String::from));
        }
        paths
    }

    unsafe fn sym<T>(lib: *mut c_void, name: &str) -> io::Result<T> {
        let cname = CString::new(name).map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid symbol"))?;
        let ptr = unsafe { dlsym(lib, cname.as_ptr()) };
        if ptr.is_null() {
            return Err(io::Error::new(ErrorKind::NotFound, format!("missing libusb symbol {name}")));
        }
        Ok(unsafe { std::mem::transmute_copy(&ptr) })
    }

    impl LibUsb {
        fn load() -> io::Result<Arc<Self>> {
            let mut handle = ptr::null_mut();
            for candidate in candidate_paths() {
                if let Ok(cpath) = CString::new(candidate) {
                    let opened = unsafe { dlopen(cpath.as_ptr(), RTLD_NOW) };
                    if !opened.is_null() {
                        handle = opened;
                        break;
                    }
                }
            }
            if handle.is_null() {
                return Err(io::Error::new(ErrorKind::NotFound, "libusb-1.0 not found; install libusb or set KUMI_LIBUSB"));
            }
            let result = unsafe {
                Ok(Arc::new(Self {
                    lib: handle,
                    init: sym(handle, "libusb_init")?,
                    exit: sym(handle, "libusb_exit")?,
                    get_device_list: sym(handle, "libusb_get_device_list")?,
                    free_device_list: sym(handle, "libusb_free_device_list")?,
                    get_device_descriptor: sym(handle, "libusb_get_device_descriptor")?,
                    get_bus_number: sym(handle, "libusb_get_bus_number")?,
                    get_device_address: sym(handle, "libusb_get_device_address")?,
                    get_device_speed: sym(handle, "libusb_get_device_speed")?,
                    open: sym(handle, "libusb_open")?,
                    close: sym(handle, "libusb_close")?,
                    control_transfer: sym(handle, "libusb_control_transfer")?,
                    claim_interface: sym(handle, "libusb_claim_interface")?,
                    release_interface: sym(handle, "libusb_release_interface")?,
                    bulk_transfer: sym(handle, "libusb_bulk_transfer")?,
                    #[cfg(not(target_os = "macos"))]
                    interrupt_transfer: sym(handle, "libusb_interrupt_transfer")?,
                    error_name: sym(handle, "libusb_error_name")?,
                    set_auto_detach: sym(handle, "libusb_set_auto_detach_kernel_driver").ok(),
                }))
            };
            if result.is_err() {
                unsafe {
                    dlclose(handle);
                }
            }
            result
        }

        fn error(&self, context: &str, code: c_int) -> io::Error {
            let name = unsafe {
                let raw = (self.error_name)(code);
                if raw.is_null() { "LIBUSB_ERROR" } else { CStr::from_ptr(raw).to_str().unwrap_or("LIBUSB_ERROR") }
            };
            let kind = if code == LIBUSB_ERROR_TIMEOUT { ErrorKind::TimedOut } else { ErrorKind::Other };
            io::Error::new(kind, format!("{context}: {name} ({code})"))
        }
    }

    struct RawHandle {
        lib: Arc<LibUsb>,
        ctx: *mut c_void,
        dev: *mut c_void,
        interface: c_int,
    }

    unsafe impl Send for RawHandle {}
    unsafe impl Sync for RawHandle {}

    impl Drop for RawHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = (self.lib.release_interface)(self.dev, self.interface);
                (self.lib.close)(self.dev);
                (self.lib.exit)(self.ctx);
            }
        }
    }

    pub struct UsbLink {
        raw: Arc<RawHandle>,
        endpoints: KumiEndpoints,
        device: UsbDeviceInfo,
        identity: Vec<u8>,
        stop: Arc<AtomicBool>,
        reader: Option<JoinHandle<()>>,
        messages: Receiver<io::Result<Vec<u8>>>,
        timeout: Duration,
    }

    impl UsbLink {
        pub fn list_kumi() -> io::Result<Vec<UsbDeviceInfo>> {
            Ok(Self::list_case_devices()?
                .into_iter()
                .filter(|dev| dev.vendor == USB_VENDOR_ID && dev.product == USB_PRODUCT_ID)
                .collect())
        }

        pub fn list_case_devices() -> io::Result<Vec<UsbDeviceInfo>> {
            let lib = LibUsb::load()?;
            let mut ctx = ptr::null_mut();
            let rc = unsafe { (lib.init)(&mut ctx) };
            if rc != 0 {
                return Err(lib.error("libusb_init", rc));
            }
            let mut list = ptr::null_mut();
            let count = unsafe { (lib.get_device_list)(ctx, &mut list) };
            if count < 0 {
                unsafe { (lib.exit)(ctx) };
                return Err(lib.error("libusb_get_device_list", count as c_int));
            }
            let mut found = Vec::new();
            for i in 0..count as usize {
                let dev = unsafe { *list.add(i) };
                let mut desc = LibusbDeviceDescriptor::default();
                if unsafe { (lib.get_device_descriptor)(dev, &mut desc) } == 0
                    && device_mode(desc.id_vendor, desc.id_product).is_some()
                {
                    found.push(UsbDeviceInfo {
                        bus: unsafe { (lib.get_bus_number)(dev) },
                        address: unsafe { (lib.get_device_address)(dev) },
                        vendor: desc.id_vendor,
                        product: desc.id_product,
                        speed: unsafe { (lib.get_device_speed)(dev) }.max(0) as u8,
                    });
                }
            }
            unsafe {
                (lib.free_device_list)(list, 1);
                (lib.exit)(ctx);
            }
            Ok(found)
        }

        pub fn open_kumi() -> io::Result<Self> {
            Self::open_kumi_at(None)
        }

        pub fn open_kumi_at(selector: Option<(u8, u8)>) -> io::Result<Self> {
            let lib = LibUsb::load()?;
            let mut ctx = ptr::null_mut();
            let rc = unsafe { (lib.init)(&mut ctx) };
            if rc != 0 {
                return Err(lib.error("libusb_init", rc));
            }
            let mut list = ptr::null_mut();
            let count = unsafe { (lib.get_device_list)(ctx, &mut list) };
            if count < 0 {
                unsafe { (lib.exit)(ctx) };
                return Err(lib.error("libusb_get_device_list", count as c_int));
            }
            let mut matches = Vec::new();
            for i in 0..count as usize {
                let dev = unsafe { *list.add(i) };
                let mut desc = LibusbDeviceDescriptor::default();
                if unsafe { (lib.get_device_descriptor)(dev, &mut desc) } == 0
                    && desc.id_vendor == USB_VENDOR_ID
                    && desc.id_product == USB_PRODUCT_ID
                {
                    let info = UsbDeviceInfo {
                        bus: unsafe { (lib.get_bus_number)(dev) },
                        address: unsafe { (lib.get_device_address)(dev) },
                        vendor: desc.id_vendor,
                        product: desc.id_product,
                        speed: unsafe { (lib.get_device_speed)(dev) }.max(0) as u8,
                    };
                    if selector.is_none_or(|(bus, addr)| bus == info.bus && addr == info.address) {
                        matches.push((dev, info));
                    }
                }
            }
            if matches.is_empty() {
                unsafe {
                    (lib.free_device_list)(list, 1);
                    (lib.exit)(ctx);
                }
                return Err(io::Error::new(
                    ErrorKind::NotFound,
                    "no Kumi 1209:0001 USB device found (press a key on the case if it is asleep)",
                ));
            }
            if matches.len() > 1 {
                unsafe {
                    (lib.free_device_list)(list, 1);
                    (lib.exit)(ctx);
                }
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "more than one Kumi 1209:0001 USB device is attached; select one with BUS:ADDR",
                ));
            }
            let (dev_ptr, device) = matches[0];
            let mut handle = ptr::null_mut();
            let rc = unsafe { (lib.open)(dev_ptr, &mut handle) };
            unsafe { (lib.free_device_list)(list, 1) };
            if rc != 0 || handle.is_null() {
                unsafe { (lib.exit)(ctx) };
                return Err(lib.error("libusb_open", rc));
            }

            let setup = (|| -> io::Result<(Vec<u8>, KumiEndpoints)> {
                let mut dev_desc = [0u8; DEVICE_DESCRIPTOR];
                let n = unsafe {
                    (lib.control_transfer)(
                        handle,
                        0x80,
                        6,
                        0x0100,
                        0,
                        dev_desc.as_mut_ptr(),
                        DEVICE_DESCRIPTOR as u16,
                        CONTROL_TIMEOUT_MS,
                    )
                };
                if n != DEVICE_DESCRIPTOR as c_int || !is_kumi_device_descriptor(&dev_desc) {
                    return Err(io::Error::other("USB device did not return a Kumi 1209:0001 device descriptor"));
                }
                let mut cfg_head = [0u8; 9];
                let n =
                    unsafe { (lib.control_transfer)(handle, 0x80, 6, 0x0200, 0, cfg_head.as_mut_ptr(), 9, CONTROL_TIMEOUT_MS) };
                if n != 9 {
                    return Err(io::Error::other("failed to read USB configuration header"));
                }
                let total = usize::from(u16::from_le_bytes([cfg_head[2], cfg_head[3]]));
                if !(9..=4096).contains(&total) {
                    return Err(io::Error::other("invalid USB configuration descriptor size"));
                }
                let mut cfg = vec![0u8; total];
                let n = unsafe {
                    (lib.control_transfer)(handle, 0x80, 6, 0x0200, 0, cfg.as_mut_ptr(), total as u16, CONTROL_TIMEOUT_MS)
                };
                if n != total as c_int {
                    return Err(io::Error::other("failed to read full USB configuration descriptor"));
                }
                let endpoints = parse_kumi_config(&cfg)
                    .ok_or_else(|| io::Error::other("Kumi vendor interface FF/4B/01 with bulk IN/OUT not found"))?;
                if let Some(detach) = lib.set_auto_detach {
                    let _ = unsafe { detach(handle, 1) };
                }
                let claim = unsafe { (lib.claim_interface)(handle, c_int::from(endpoints.interface)) };
                if claim != 0 {
                    return Err(lib.error("libusb_claim_interface", claim));
                }
                let mut identity = Vec::with_capacity(DEVICE_DESCRIPTOR + total);
                identity.extend_from_slice(&dev_desc);
                identity.extend_from_slice(&cfg);
                Ok((identity, endpoints))
            })();

            let (identity, endpoints) = match setup {
                Ok(pair) => pair,
                Err(error) => {
                    unsafe {
                        (lib.close)(handle);
                        (lib.exit)(ctx);
                    }
                    return Err(error);
                }
            };

            let raw = Arc::new(RawHandle { lib, ctx, dev: handle, interface: c_int::from(endpoints.interface) });
            let stop = Arc::new(AtomicBool::new(false));
            let (sender, messages) = mpsc::channel();
            let reader_raw = Arc::clone(&raw);
            let reader_stop = Arc::clone(&stop);
            let in_ep = endpoints.in_ep;
            let reader = thread::spawn(move || {
                let mut chunk = vec![0u8; MAX_FRAME];
                let mut frame = Vec::new();
                while !reader_stop.load(Ordering::Relaxed) {
                    let mut transferred: c_int = 0;
                    let rc = unsafe {
                        (reader_raw.lib.bulk_transfer)(
                            reader_raw.dev,
                            in_ep,
                            chunk.as_mut_ptr(),
                            chunk.len() as c_int,
                            &mut transferred,
                            IN_POLL_MS,
                        )
                    };
                    if rc == LIBUSB_ERROR_TIMEOUT && transferred <= 0 {
                        continue;
                    }
                    if rc != 0 && rc != LIBUSB_ERROR_TIMEOUT {
                        if !reader_stop.load(Ordering::Relaxed) {
                            let _ = sender.send(Err(reader_raw.lib.error("USB IN", rc)));
                        }
                        break;
                    }
                    let count = transferred.max(0) as usize;
                    if frame.len() + count > MAX_FRAME {
                        let _ = sender
                            .send(Err(io::Error::new(ErrorKind::InvalidData, format!("device frame exceeds {MAX_FRAME} bytes"))));
                        break;
                    }
                    frame.extend_from_slice(&chunk[..count]);
                    if rc == 0 && count < chunk.len() {
                        if !frame.is_empty() && sender.send(Ok(std::mem::take(&mut frame))).is_err() {
                            break;
                        }
                        frame.clear();
                    }
                }
            });

            Ok(Self { raw, endpoints, device, identity, stop, reader: Some(reader), messages, timeout: REPLY_TIMEOUT })
        }

        pub fn identity(&self) -> &[u8] {
            &self.identity
        }

        pub fn device(&self) -> UsbDeviceInfo {
            self.device
        }

        pub fn endpoints(&self) -> KumiEndpoints {
            self.endpoints
        }

        pub fn set_timeout(&mut self, timeout: Duration) {
            self.timeout = timeout;
        }

        fn next(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
            match self.messages.recv_timeout(wait) {
                Ok(message) => message.map(Some),
                Err(RecvTimeoutError::Timeout) => Ok(None),
                Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(ErrorKind::UnexpectedEof, "the USB link closed")),
            }
        }
    }

    impl Drop for UsbLink {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
    }

    impl FrameLink for UsbLink {
        type Error = io::Error;

        fn send(&mut self, frame: &[u8]) -> io::Result<()> {
            if frame.is_empty() || frame.len() > MAX_FRAME {
                return Err(io::Error::new(ErrorKind::InvalidInput, "USB frames hold 1..4096 bytes"));
            }
            let mut transferred: c_int = 0;
            let rc = unsafe {
                (self.raw.lib.bulk_transfer)(
                    self.raw.dev,
                    self.endpoints.out_ep,
                    frame.as_ptr().cast_mut(),
                    frame.len() as c_int,
                    &mut transferred,
                    OUT_TIMEOUT_MS,
                )
            };
            if rc != 0 {
                return Err(self.raw.lib.error("USB OUT", rc));
            }
            if transferred as usize != frame.len() {
                return Err(io::Error::new(
                    ErrorKind::WriteZero,
                    format!("short USB OUT transfer: {transferred} of {} bytes", frame.len()),
                ));
            }
            if frame.len().is_multiple_of(usize::from(self.endpoints.out_max)) {
                let mut zero = 0;
                let rc = unsafe {
                    (self.raw.lib.bulk_transfer)(
                        self.raw.dev,
                        self.endpoints.out_ep,
                        ptr::null_mut(),
                        0,
                        &mut zero,
                        OUT_TIMEOUT_MS,
                    )
                };
                if rc != 0 {
                    return Err(self.raw.lib.error("USB OUT ZLP", rc));
                }
            }
            Ok(())
        }

        fn receive(&mut self) -> io::Result<Vec<u8>> {
            let wait = self.timeout;
            self.next(wait)?.ok_or_else(|| io::Error::new(ErrorKind::TimedOut, "no frame from the case"))
        }

        fn try_receive(&mut self) -> io::Result<Option<Vec<u8>>> {
            self.next(POLL)
        }
    }

    #[cfg(target_os = "macos")]
    mod mac_hid {
        use std::ffi::{CString, c_char, c_double, c_long, c_void};
        use std::io::{self, ErrorKind};
        use std::ptr;
        use std::time::{Duration, Instant};

        use super::{BROM_PACKET, BROM_PRODUCT_ID, BROM_VENDOR_ID, RTLD_NOW, brom_reply_timeout_ms, dlclose, dlopen, dlsym};

        type CFTypeRef = *mut c_void;
        type CFStringRef = *mut c_void;
        type CFSetRef = *mut c_void;
        type CFRunLoopRef = *mut c_void;
        type IOHIDManagerRef = *mut c_void;
        type IOHIDDeviceRef = *mut c_void;
        type IOReturn = i32;

        const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
        const K_CF_NUMBER_SINT32: c_long = 3;
        const K_IO_HID_REPORT_TYPE_OUTPUT: u32 = 1;

        type FnCFRetain = unsafe extern "C" fn(CFTypeRef) -> CFTypeRef;
        type FnCFRelease = unsafe extern "C" fn(CFTypeRef);
        type FnCFStringCreate = unsafe extern "C" fn(CFTypeRef, *const c_char, u32) -> CFStringRef;
        type FnCFNumberCreate = unsafe extern "C" fn(CFTypeRef, c_long, *const c_void) -> CFTypeRef;
        type FnCFNumberGetValue = unsafe extern "C" fn(CFTypeRef, c_long, *mut c_void) -> u8;
        type FnCFDictionaryCreate = unsafe extern "C" fn(
            CFTypeRef,
            *const CFTypeRef,
            *const CFTypeRef,
            c_long,
            *const c_void,
            *const c_void,
        ) -> CFTypeRef;
        type FnCFSetGetCount = unsafe extern "C" fn(CFSetRef) -> c_long;
        type FnCFSetGetValues = unsafe extern "C" fn(CFSetRef, *mut CFTypeRef);
        type FnCFRunLoopGetCurrent = unsafe extern "C" fn() -> CFRunLoopRef;
        type FnCFRunLoopRunInMode = unsafe extern "C" fn(CFStringRef, c_double, u8) -> i32;
        type FnCFRunLoopStop = unsafe extern "C" fn(CFRunLoopRef);

        type InputCallback = unsafe extern "C" fn(*mut c_void, IOReturn, *mut c_void, u32, u32, *mut u8, c_long);

        type FnHidManagerCreate = unsafe extern "C" fn(CFTypeRef, u32) -> IOHIDManagerRef;
        type FnHidManagerSetMatching = unsafe extern "C" fn(IOHIDManagerRef, CFTypeRef);
        type FnHidManagerOpen = unsafe extern "C" fn(IOHIDManagerRef, u32) -> IOReturn;
        type FnHidManagerClose = unsafe extern "C" fn(IOHIDManagerRef, u32) -> IOReturn;
        type FnHidManagerCopyDevices = unsafe extern "C" fn(IOHIDManagerRef) -> CFSetRef;
        type FnHidDeviceGetProperty = unsafe extern "C" fn(IOHIDDeviceRef, CFStringRef) -> CFTypeRef;
        type FnHidDeviceOpen = unsafe extern "C" fn(IOHIDDeviceRef, u32) -> IOReturn;
        type FnHidDeviceClose = unsafe extern "C" fn(IOHIDDeviceRef, u32) -> IOReturn;
        type FnHidDeviceSchedule = unsafe extern "C" fn(IOHIDDeviceRef, CFRunLoopRef, CFStringRef);
        type FnHidDeviceUnschedule = unsafe extern "C" fn(IOHIDDeviceRef, CFRunLoopRef, CFStringRef);
        type FnHidDeviceRegisterInput = unsafe extern "C" fn(IOHIDDeviceRef, *mut u8, c_long, InputCallback, *mut c_void);
        type FnHidDeviceSetReport = unsafe extern "C" fn(IOHIDDeviceRef, u32, c_long, *const u8, c_long) -> IOReturn;

        struct Api {
            cf: *mut c_void,
            iokit: *mut c_void,
            cf_retain: FnCFRetain,
            cf_release: FnCFRelease,
            cf_string_create: FnCFStringCreate,
            cf_number_create: FnCFNumberCreate,
            cf_number_get_value: FnCFNumberGetValue,
            cf_dictionary_create: FnCFDictionaryCreate,
            cf_key_callbacks: *const c_void,
            cf_value_callbacks: *const c_void,
            cf_set_get_count: FnCFSetGetCount,
            cf_set_get_values: FnCFSetGetValues,
            cf_run_loop_get_current: FnCFRunLoopGetCurrent,
            cf_run_loop_run_in_mode: FnCFRunLoopRunInMode,
            cf_run_loop_stop: FnCFRunLoopStop,
            hid_manager_create: FnHidManagerCreate,
            hid_manager_set_matching: FnHidManagerSetMatching,
            hid_manager_open: FnHidManagerOpen,
            hid_manager_close: FnHidManagerClose,
            hid_manager_copy_devices: FnHidManagerCopyDevices,
            hid_device_get_property: FnHidDeviceGetProperty,
            hid_device_open: FnHidDeviceOpen,
            hid_device_close: FnHidDeviceClose,
            hid_device_schedule: FnHidDeviceSchedule,
            hid_device_unschedule: FnHidDeviceUnschedule,
            hid_device_register_input: FnHidDeviceRegisterInput,
            hid_device_set_report: FnHidDeviceSetReport,
            run_loop_default_mode: CFStringRef,
        }

        impl Drop for Api {
            fn drop(&mut self) {
                unsafe {
                    if !self.iokit.is_null() {
                        dlclose(self.iokit);
                    }
                    if !self.cf.is_null() {
                        dlclose(self.cf);
                    }
                }
            }
        }

        unsafe fn load_sym<T>(lib: *mut c_void, name: &str) -> io::Result<T> {
            let cname = CString::new(name).map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid symbol"))?;
            let ptr = unsafe { dlsym(lib, cname.as_ptr()) };
            if ptr.is_null() {
                return Err(io::Error::new(ErrorKind::NotFound, format!("missing framework symbol {name}")));
            }
            Ok(unsafe { std::mem::transmute_copy(&ptr) })
        }

        impl Api {
            fn load() -> io::Result<Self> {
                let cf_path = CString::new("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation").unwrap();
                let io_path = CString::new("/System/Library/Frameworks/IOKit.framework/IOKit").unwrap();
                let cf = unsafe { dlopen(cf_path.as_ptr(), RTLD_NOW) };
                if cf.is_null() {
                    return Err(io::Error::new(ErrorKind::NotFound, "CoreFoundation framework unavailable"));
                }
                let iokit = unsafe { dlopen(io_path.as_ptr(), RTLD_NOW) };
                if iokit.is_null() {
                    unsafe { dlclose(cf) };
                    return Err(io::Error::new(ErrorKind::NotFound, "IOKit framework unavailable"));
                }
                let loaded = (|| -> io::Result<Self> {
                    unsafe {
                        let mode_sym: *const CFStringRef = load_sym(cf, "kCFRunLoopDefaultMode")?;
                        Ok(Self {
                            cf,
                            iokit,
                            cf_retain: load_sym(cf, "CFRetain")?,
                            cf_release: load_sym(cf, "CFRelease")?,
                            cf_string_create: load_sym(cf, "CFStringCreateWithCString")?,
                            cf_number_create: load_sym(cf, "CFNumberCreate")?,
                            cf_number_get_value: load_sym(cf, "CFNumberGetValue")?,
                            cf_dictionary_create: load_sym(cf, "CFDictionaryCreate")?,
                            cf_key_callbacks: load_sym(cf, "kCFTypeDictionaryKeyCallBacks")?,
                            cf_value_callbacks: load_sym(cf, "kCFTypeDictionaryValueCallBacks")?,
                            cf_set_get_count: load_sym(cf, "CFSetGetCount")?,
                            cf_set_get_values: load_sym(cf, "CFSetGetValues")?,
                            cf_run_loop_get_current: load_sym(cf, "CFRunLoopGetCurrent")?,
                            cf_run_loop_run_in_mode: load_sym(cf, "CFRunLoopRunInMode")?,
                            cf_run_loop_stop: load_sym(cf, "CFRunLoopStop")?,
                            hid_manager_create: load_sym(iokit, "IOHIDManagerCreate")?,
                            hid_manager_set_matching: load_sym(iokit, "IOHIDManagerSetDeviceMatching")?,
                            hid_manager_open: load_sym(iokit, "IOHIDManagerOpen")?,
                            hid_manager_close: load_sym(iokit, "IOHIDManagerClose")?,
                            hid_manager_copy_devices: load_sym(iokit, "IOHIDManagerCopyDevices")?,
                            hid_device_get_property: load_sym(iokit, "IOHIDDeviceGetProperty")?,
                            hid_device_open: load_sym(iokit, "IOHIDDeviceOpen")?,
                            hid_device_close: load_sym(iokit, "IOHIDDeviceClose")?,
                            hid_device_schedule: load_sym(iokit, "IOHIDDeviceScheduleWithRunLoop")?,
                            hid_device_unschedule: load_sym(iokit, "IOHIDDeviceUnscheduleFromRunLoop")?,
                            hid_device_register_input: load_sym(iokit, "IOHIDDeviceRegisterInputReportCallback")?,
                            hid_device_set_report: load_sym(iokit, "IOHIDDeviceSetReport")?,
                            run_loop_default_mode: *mode_sym,
                        })
                    }
                })();
                if loaded.is_err() {
                    unsafe {
                        dlclose(iokit);
                        dlclose(cf);
                    }
                }
                loaded
            }

            unsafe fn matching_vid_pid(&self, vendor: u16, product: u16) -> io::Result<CFTypeRef> {
                let k_vid = CString::new("VendorID").unwrap();
                let k_pid = CString::new("ProductID").unwrap();
                let cf_k_vid = unsafe { (self.cf_string_create)(ptr::null_mut(), k_vid.as_ptr(), K_CF_STRING_ENCODING_UTF8) };
                let cf_k_pid = unsafe { (self.cf_string_create)(ptr::null_mut(), k_pid.as_ptr(), K_CF_STRING_ENCODING_UTF8) };
                let vid = i32::from(vendor);
                let pid = i32::from(product);
                let cf_v_vid = unsafe { (self.cf_number_create)(ptr::null_mut(), K_CF_NUMBER_SINT32, (&raw const vid).cast()) };
                let cf_v_pid = unsafe { (self.cf_number_create)(ptr::null_mut(), K_CF_NUMBER_SINT32, (&raw const pid).cast()) };
                let keys = [cf_k_vid, cf_k_pid];
                let values = [cf_v_vid, cf_v_pid];
                let dict = unsafe {
                    (self.cf_dictionary_create)(
                        ptr::null_mut(),
                        keys.as_ptr(),
                        values.as_ptr(),
                        2,
                        self.cf_key_callbacks,
                        self.cf_value_callbacks,
                    )
                };
                unsafe {
                    if !cf_k_vid.is_null() {
                        (self.cf_release)(cf_k_vid);
                    }
                    if !cf_k_pid.is_null() {
                        (self.cf_release)(cf_k_pid);
                    }
                    if !cf_v_vid.is_null() {
                        (self.cf_release)(cf_v_vid);
                    }
                    if !cf_v_pid.is_null() {
                        (self.cf_release)(cf_v_pid);
                    }
                }
                if dict.is_null() {
                    return Err(io::Error::other("CFDictionaryCreate failed"));
                }
                Ok(dict)
            }

            unsafe fn prop_i32(&self, dev: IOHIDDeviceRef, key: &str) -> Option<i32> {
                let ckey = CString::new(key).ok()?;
                let cf_key = unsafe { (self.cf_string_create)(ptr::null_mut(), ckey.as_ptr(), K_CF_STRING_ENCODING_UTF8) };
                if cf_key.is_null() {
                    return None;
                }
                let prop = unsafe { (self.hid_device_get_property)(dev, cf_key) };
                unsafe { (self.cf_release)(cf_key) };
                if prop.is_null() {
                    return None;
                }
                let mut value: i32 = 0;
                let ok = unsafe { (self.cf_number_get_value)(prop, K_CF_NUMBER_SINT32, (&raw mut value).cast()) };
                (ok != 0).then_some(value)
            }
        }

        struct CallbackState {
            reply: Option<Vec<u8>>,
            run_loop: CFRunLoopRef,
            stop_fn: FnCFRunLoopStop,
        }

        unsafe extern "C" fn on_input_report(
            context: *mut c_void,
            _result: IOReturn,
            _sender: *mut c_void,
            _kind: u32,
            _report_id: u32,
            report: *mut u8,
            report_length: c_long,
        ) {
            if context.is_null() || report.is_null() || report_length <= 0 {
                return;
            }
            let state = unsafe { &mut *context.cast::<CallbackState>() };
            let slice = unsafe { std::slice::from_raw_parts(report, report_length as usize) };
            state.reply = Some(slice.to_vec());
            unsafe { (state.stop_fn)(state.run_loop) };
        }

        pub struct HidBrom {
            api: Api,
            manager: IOHIDManagerRef,
            device: IOHIDDeviceRef,
            run_loop: CFRunLoopRef,
            report_buf: Box<[u8; BROM_PACKET]>,
            state: Box<CallbackState>,
        }

        impl HidBrom {
            pub fn open() -> io::Result<Self> {
                let api = Api::load()?;
                let manager = unsafe { (api.hid_manager_create)(ptr::null_mut(), 0) };
                if manager.is_null() {
                    return Err(io::Error::other("IOHIDManagerCreate failed"));
                }
                let matching = unsafe { api.matching_vid_pid(BROM_VENDOR_ID, BROM_PRODUCT_ID) };
                let matching = match matching {
                    Ok(dict) => dict,
                    Err(error) => {
                        unsafe { (api.cf_release)(manager) };
                        return Err(error);
                    }
                };
                unsafe {
                    (api.hid_manager_set_matching)(manager, matching);
                    (api.cf_release)(matching);
                }
                let rc = unsafe { (api.hid_manager_open)(manager, 0) };
                if rc != 0 {
                    unsafe { (api.cf_release)(manager) };
                    return Err(io::Error::other(format!("IOHIDManagerOpen failed ({rc:#x})")));
                }
                let devices = unsafe { (api.hid_manager_copy_devices)(manager) };
                let mut matched = ptr::null_mut();
                if !devices.is_null() {
                    let count = unsafe { (api.cf_set_get_count)(devices) };
                    if count > 0 {
                        let mut ptrs = vec![ptr::null_mut(); count as usize];
                        unsafe { (api.cf_set_get_values)(devices, ptrs.as_mut_ptr()) };
                        for dev in ptrs {
                            let vid = unsafe { api.prop_i32(dev, "VendorID") };
                            let pid = unsafe { api.prop_i32(dev, "ProductID") };
                            if vid == Some(i32::from(BROM_VENDOR_ID)) && pid == Some(i32::from(BROM_PRODUCT_ID)) {
                                unsafe { (api.cf_retain)(dev) };
                                matched = dev;
                                break;
                            }
                        }
                    }
                    unsafe { (api.cf_release)(devices) };
                }
                if matched.is_null() {
                    unsafe {
                        let _ = (api.hid_manager_close)(manager, 0);
                        (api.cf_release)(manager);
                    }
                    return Err(io::Error::new(
                        ErrorKind::NotFound,
                        "no boot ROM 0666:0888 USB device found (put the case in service mode first)",
                    ));
                }
                let rc = unsafe { (api.hid_device_open)(matched, 0) };
                if rc != 0 {
                    unsafe {
                        (api.cf_release)(matched);
                        let _ = (api.hid_manager_close)(manager, 0);
                        (api.cf_release)(manager);
                    }
                    return Err(io::Error::other(format!("IOHIDDeviceOpen failed ({rc:#x})")));
                }
                let run_loop = unsafe { (api.cf_run_loop_get_current)() };
                let mut report_buf = Box::new([0u8; BROM_PACKET]);
                let mut state = Box::new(CallbackState { reply: None, run_loop, stop_fn: api.cf_run_loop_stop });
                unsafe {
                    (api.hid_device_register_input)(
                        matched,
                        report_buf.as_mut_ptr(),
                        BROM_PACKET as c_long,
                        on_input_report,
                        (&raw mut *state).cast(),
                    );
                    (api.hid_device_schedule)(matched, run_loop, api.run_loop_default_mode);
                }
                Ok(Self { api, manager, device: matched, run_loop, report_buf, state })
            }

            pub fn transfer(&mut self, packet: &[u8]) -> io::Result<Vec<u8>> {
                if packet.len() > BROM_PACKET {
                    return Err(io::Error::new(ErrorKind::InvalidInput, "boot ROM packet exceeds 512 bytes"));
                }
                let mut request = [0u8; BROM_PACKET];
                request[..packet.len()].copy_from_slice(packet);
                self.state.reply = None;
                let rc = unsafe {
                    (self.api.hid_device_set_report)(
                        self.device,
                        K_IO_HID_REPORT_TYPE_OUTPUT,
                        0,
                        request.as_ptr(),
                        BROM_PACKET as c_long,
                    )
                };
                if rc != 0 {
                    return Err(io::Error::other(format!("IOHIDDeviceSetReport failed ({rc:#x})")));
                }
                let deadline = Instant::now() + Duration::from_millis(u64::from(brom_reply_timeout_ms(&request)));
                while self.state.reply.is_none() {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(io::Error::new(ErrorKind::TimedOut, "no reply from the boot ROM over USB HID"));
                    }
                    let slice = remaining.min(Duration::from_millis(50)).as_secs_f64();
                    unsafe {
                        (self.api.cf_run_loop_run_in_mode)(self.api.run_loop_default_mode, slice, 1);
                    }
                }
                let _ = self.report_buf.len();
                Ok(self.state.reply.take().unwrap())
            }
        }

        impl Drop for HidBrom {
            fn drop(&mut self) {
                unsafe {
                    (self.api.hid_device_unschedule)(self.device, self.run_loop, self.api.run_loop_default_mode);
                    let _ = (self.api.hid_device_close)(self.device, 0);
                    (self.api.cf_release)(self.device);
                    let _ = (self.api.hid_manager_close)(self.manager, 0);
                    (self.api.cf_release)(self.manager);
                }
            }
        }
    }

    pub struct BromUsbTransport {
        #[cfg(target_os = "macos")]
        inner: mac_hid::HidBrom,
        #[cfg(not(target_os = "macos"))]
        raw: RawHandle,
    }

    impl BromUsbTransport {
        #[cfg(target_os = "macos")]
        pub fn open_at(_selector: Option<(u8, u8)>) -> io::Result<Self> {
            Ok(Self { inner: mac_hid::HidBrom::open()? })
        }

        #[cfg(not(target_os = "macos"))]
        pub fn open_at(selector: Option<(u8, u8)>) -> io::Result<Self> {
            let lib = LibUsb::load()?;
            let mut ctx = ptr::null_mut();
            let rc = unsafe { (lib.init)(&mut ctx) };
            if rc != 0 {
                return Err(lib.error("libusb_init", rc));
            }
            let mut list = ptr::null_mut();
            let count = unsafe { (lib.get_device_list)(ctx, &mut list) };
            if count < 0 {
                unsafe { (lib.exit)(ctx) };
                return Err(lib.error("libusb_get_device_list", count as c_int));
            }
            let mut matches = Vec::new();
            for i in 0..count as usize {
                let dev = unsafe { *list.add(i) };
                let mut desc = LibusbDeviceDescriptor::default();
                if unsafe { (lib.get_device_descriptor)(dev, &mut desc) } == 0
                    && desc.id_vendor == BROM_VENDOR_ID
                    && desc.id_product == BROM_PRODUCT_ID
                {
                    let bus = unsafe { (lib.get_bus_number)(dev) };
                    let addr = unsafe { (lib.get_device_address)(dev) };
                    if selector.is_none_or(|(b, a)| b == bus && a == addr) {
                        matches.push(dev);
                    }
                }
            }
            if matches.is_empty() {
                unsafe {
                    (lib.free_device_list)(list, 1);
                    (lib.exit)(ctx);
                }
                return Err(io::Error::new(
                    ErrorKind::NotFound,
                    "no boot ROM 0666:0888 USB device found (put the case in service mode first)",
                ));
            }
            if matches.len() > 1 {
                unsafe {
                    (lib.free_device_list)(list, 1);
                    (lib.exit)(ctx);
                }
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "more than one boot ROM 0666:0888 device is attached; select one with BUS:ADDR",
                ));
            }
            let mut handle = ptr::null_mut();
            let rc = unsafe { (lib.open)(matches[0], &mut handle) };
            unsafe { (lib.free_device_list)(list, 1) };
            if rc != 0 || handle.is_null() {
                unsafe { (lib.exit)(ctx) };
                return Err(lib.error("libusb_open", rc));
            }
            let setup = (|| -> io::Result<u8> {
                let mut dev_desc = [0u8; DEVICE_DESCRIPTOR];
                let n = unsafe {
                    (lib.control_transfer)(
                        handle,
                        0x80,
                        6,
                        0x0100,
                        0,
                        dev_desc.as_mut_ptr(),
                        DEVICE_DESCRIPTOR as u16,
                        CONTROL_TIMEOUT_MS,
                    )
                };
                if n != DEVICE_DESCRIPTOR as c_int || !is_brom_device_descriptor(&dev_desc) {
                    return Err(io::Error::other("USB device did not return a boot ROM 0666:0888 descriptor"));
                }
                let mut cfg_head = [0u8; 9];
                let n =
                    unsafe { (lib.control_transfer)(handle, 0x80, 6, 0x0200, 0, cfg_head.as_mut_ptr(), 9, CONTROL_TIMEOUT_MS) };
                if n != 9 {
                    return Err(io::Error::other("failed to read boot ROM configuration header"));
                }
                let total = usize::from(u16::from_le_bytes([cfg_head[2], cfg_head[3]]));
                if !(9..=4096).contains(&total) {
                    return Err(io::Error::other("invalid boot ROM configuration descriptor size"));
                }
                let mut cfg = vec![0u8; total];
                let n = unsafe {
                    (lib.control_transfer)(handle, 0x80, 6, 0x0200, 0, cfg.as_mut_ptr(), total as u16, CONTROL_TIMEOUT_MS)
                };
                if n != total as c_int {
                    return Err(io::Error::other("failed to read full boot ROM configuration descriptor"));
                }
                let (iface, _, _) = parse_brom_config(&cfg)
                    .ok_or_else(|| io::Error::other("boot ROM HID interface 0 with interrupt 81/02 endpoints not found"))?;
                if let Some(detach) = lib.set_auto_detach {
                    let _ = unsafe { detach(handle, 1) };
                }
                let claim = unsafe { (lib.claim_interface)(handle, c_int::from(iface)) };
                if claim != 0 {
                    return Err(lib.error("libusb_claim_interface", claim));
                }
                Ok(iface)
            })();
            match setup {
                Ok(iface) => Ok(Self { raw: RawHandle { lib, ctx, dev: handle, interface: c_int::from(iface) } }),
                Err(error) => {
                    unsafe {
                        (lib.close)(handle);
                        (lib.exit)(ctx);
                    }
                    Err(error)
                }
            }
        }

        #[cfg(target_os = "macos")]
        pub fn transfer(&mut self, packet: &[u8]) -> io::Result<Vec<u8>> {
            self.inner.transfer(packet)
        }

        #[cfg(not(target_os = "macos"))]
        pub fn transfer(&mut self, packet: &[u8]) -> io::Result<Vec<u8>> {
            if packet.len() > BROM_PACKET {
                return Err(io::Error::new(ErrorKind::InvalidInput, "boot ROM packet exceeds 512 bytes"));
            }
            let mut request = [0u8; BROM_PACKET];
            request[..packet.len()].copy_from_slice(packet);
            let mut sent: c_int = 0;
            let rc = unsafe {
                (self.raw.lib.interrupt_transfer)(
                    self.raw.dev,
                    BROM_OUT_EP,
                    request.as_mut_ptr(),
                    BROM_PACKET as c_int,
                    &mut sent,
                    BROM_TIMEOUT_MS,
                )
            };
            if rc != 0 {
                return Err(self.raw.lib.error("USB BROM OUT", rc));
            }
            if sent as usize != BROM_PACKET {
                return Err(io::Error::new(ErrorKind::WriteZero, format!("short boot ROM OUT: {sent} bytes")));
            }
            let mut reply = vec![0u8; BROM_PACKET];
            let mut got: c_int = 0;
            let timeout = brom_reply_timeout_ms(&request);
            let rc = unsafe {
                (self.raw.lib.interrupt_transfer)(
                    self.raw.dev,
                    BROM_IN_EP,
                    reply.as_mut_ptr(),
                    BROM_PACKET as c_int,
                    &mut got,
                    timeout,
                )
            };
            if rc != 0 {
                return Err(self.raw.lib.error("USB BROM IN", rc));
            }
            reply.truncate(got.max(0) as usize);
            Ok(reply)
        }
    }
}

#[cfg(not(all(unix, not(target_os = "android"))))]
mod native {
    use std::io::{self, ErrorKind};
    use std::time::Duration;

    use super::{KumiEndpoints, UsbDeviceInfo};
    use crate::wire::FrameLink;

    pub struct UsbLink;
    pub struct BromUsbTransport;

    impl UsbLink {
        pub fn list_kumi() -> io::Result<Vec<UsbDeviceInfo>> {
            Err(io::Error::new(ErrorKind::Unsupported, "direct USB transport is unavailable on this platform"))
        }

        pub fn list_case_devices() -> io::Result<Vec<UsbDeviceInfo>> {
            Err(io::Error::new(ErrorKind::Unsupported, "direct USB transport is unavailable on this platform"))
        }

        pub fn open_kumi() -> io::Result<Self> {
            Err(io::Error::new(ErrorKind::Unsupported, "direct USB transport is unavailable on this platform"))
        }

        pub fn open_kumi_at(_selector: Option<(u8, u8)>) -> io::Result<Self> {
            Err(io::Error::new(ErrorKind::Unsupported, "direct USB transport is unavailable on this platform"))
        }

        pub fn identity(&self) -> &[u8] {
            &[]
        }

        pub fn device(&self) -> UsbDeviceInfo {
            UsbDeviceInfo { bus: 0, address: 0, vendor: 0, product: 0, speed: 0 }
        }

        pub fn endpoints(&self) -> KumiEndpoints {
            KumiEndpoints { interface: 0, in_ep: 0, out_ep: 0, in_max: 0, out_max: 0 }
        }

        pub fn set_timeout(&mut self, _timeout: Duration) {}
    }

    impl FrameLink for UsbLink {
        type Error = io::Error;

        fn send(&mut self, _frame: &[u8]) -> io::Result<()> {
            Err(io::Error::new(ErrorKind::Unsupported, "direct USB transport is unavailable on this platform"))
        }

        fn receive(&mut self) -> io::Result<Vec<u8>> {
            Err(io::Error::new(ErrorKind::Unsupported, "direct USB transport is unavailable on this platform"))
        }

        fn try_receive(&mut self) -> io::Result<Option<Vec<u8>>> {
            Err(io::Error::new(ErrorKind::Unsupported, "direct USB transport is unavailable on this platform"))
        }
    }

    impl BromUsbTransport {
        pub fn open_at(_selector: Option<(u8, u8)>) -> io::Result<Self> {
            Err(io::Error::new(ErrorKind::Unsupported, "direct USB boot ROM transport is unavailable on this platform"))
        }

        pub fn transfer(&mut self, _packet: &[u8]) -> io::Result<Vec<u8>> {
            Err(io::Error::new(ErrorKind::Unsupported, "direct USB boot ROM transport is unavailable on this platform"))
        }
    }
}

pub use native::{BromUsbTransport, UsbLink};

pub fn parse_selector(text: &str) -> io::Result<(u8, u8)> {
    let (bus, addr) = text
        .split_once(':')
        .or_else(|| text.split_once('-'))
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "USB selector must look like BUS:ADDR"))?;
    let bus = bus.parse::<u8>().map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid USB bus number"))?;
    let addr = addr.parse::<u8>().map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid USB device address"))?;
    Ok((bus, addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device_desc(vid: u16, pid: u16) -> Vec<u8> {
        let mut out = vec![18, 1, 0, 2, 0, 0, 0, 64];
        out.extend_from_slice(&vid.to_le_bytes());
        out.extend_from_slice(&pid.to_le_bytes());
        out.extend_from_slice(&[0x10, 0x00, 1, 2, 0, 1]);
        out
    }

    fn config_with_kumi(max_packet: u16) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&[9, 4, 0, 0, 1, 3, 1, 1, 0]);
        body.extend_from_slice(&[7, 5, 0x81, 3, 16, 0, 1]);
        body.extend_from_slice(&[9, 4, 1, 0, 2, 0xff, 0x4b, 0x01, 0]);
        let [lo, hi] = max_packet.to_le_bytes();
        body.extend_from_slice(&[7, 5, 0x82, 2, lo, hi, 0]);
        body.extend_from_slice(&[7, 5, 0x03, 2, lo, hi, 0]);
        let total = (9 + body.len()) as u16;
        let [tlo, thi] = total.to_le_bytes();
        let mut out = vec![9, 2, tlo, thi, 2, 1, 0, 0x80, 50];
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn device_descriptor_checks_kumi_ids() {
        assert!(is_kumi_device_descriptor(&device_desc(USB_VENDOR_ID, USB_PRODUCT_ID)));
        assert!(!is_kumi_device_descriptor(&device_desc(0x0661, 0x0881)));
        assert!(!is_kumi_device_descriptor(&device_desc(USB_VENDOR_ID, USB_PRODUCT_ID)[..17]));
    }

    #[test]
    fn config_parser_finds_high_and_full_speed_vendor_endpoints() {
        for max in [64, 512] {
            let eps = parse_kumi_config(&config_with_kumi(max)).unwrap();
            assert_eq!(eps, KumiEndpoints { interface: 1, in_ep: 0x82, out_ep: 0x03, in_max: max, out_max: max });
        }
    }

    #[test]
    fn config_parser_rejects_missing_or_wrong_interface() {
        let mut wrong_class = config_with_kumi(512);
        wrong_class[9 + 16 + 5] = 0xfe;
        assert_eq!(parse_kumi_config(&wrong_class), None);

        let mut wrong_type = config_with_kumi(512);
        wrong_type[9 + 16 + 9 + 3] = 3;
        assert_eq!(parse_kumi_config(&wrong_type), None);

        let mut truncated = config_with_kumi(512);
        truncated.pop();
        assert_eq!(parse_kumi_config(&truncated), None);
    }

    #[test]
    fn selector_parses_bus_and_address() {
        assert_eq!(parse_selector("1:12").unwrap(), (1, 12));
        assert_eq!(parse_selector("002-007").unwrap(), (2, 7));
        assert!(parse_selector("12").is_err());
        assert!(parse_selector("256:1").is_err());
    }

    #[test]
    fn brom_descriptor_and_config_match_captured_service_mode() {
        let dev = [0x12, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x40, 0x66, 0x06, 0x88, 0x08, 0x00, 0x01, 0x01, 0x02, 0x03, 0x01];
        assert!(is_brom_device_descriptor(&dev));
        assert!(!is_brom_device_descriptor(&device_desc(USB_VENDOR_ID, USB_PRODUCT_ID)));
        let cfg = [
            0x09, 0x02, 0x29, 0x00, 0x01, 0x01, 0x00, 0xa0, 0x32, 0x09, 0x04, 0x00, 0x00, 0x02, 0x03, 0x00, 0x00, 0x00, 0x09,
            0x21, 0x11, 0x01, 0x21, 0x01, 0x22, 0x25, 0x00, 0x07, 0x05, 0x81, 0x03, 0x00, 0x02, 0x03, 0x07, 0x05, 0x02, 0x03,
            0x00, 0x02, 0x03,
        ];
        assert_eq!(parse_brom_config(&cfg), Some((0, BROM_IN_EP, BROM_OUT_EP)));
        let mut bad = cfg;
        bad[29] = 0x82;
        assert_eq!(parse_brom_config(&bad), None);
        assert_eq!(brom_reply_timeout_ms(&[b'B', b'U', b'X', b'X', 0x20, 0x01]), BROM_ERASE_TIMEOUT_MS);
        assert_eq!(brom_reply_timeout_ms(&[b'B', b'U', b'X', b'X', 0x23, 0x01]), BROM_TIMEOUT_MS);
        assert_eq!(device_mode(USB_VENDOR_ID, USB_PRODUCT_ID), Some("kumi"));
        assert_eq!(device_mode(BROM_VENDOR_ID, BROM_PRODUCT_ID), Some("brom"));
        assert_eq!(device_mode(STOCK_VENDOR_ID, STOCK_PRODUCT_ID), Some("stock"));
        assert_eq!(device_mode(0x1234, 0x5678), None);
        assert_eq!(speed_name(3), "high");
    }
}
