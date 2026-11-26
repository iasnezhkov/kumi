// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use core::cell::Cell;
use core::future::poll_fn;
use core::ptr::{read_volatile, write_volatile};
use core::task::Poll;
use embassy_usb_driver::{
    Bus as BusTrait, ControlPipe as ControlTrait, Direction, Driver as DriverTrait, Endpoint as EndpointTrait, EndpointAddress,
    EndpointAllocError, EndpointError, EndpointIn, EndpointInfo, EndpointOut, EndpointType, Event, Unsupported,
};

const BASE: usize = 0x200d_0000;
const POWER: usize = BASE + 0x01;
const POWER_RESUME: u8 = 0x04;
const POWER_HSMODE: u8 = 0x10;
const POWER_HSENAB: u8 = 0x20;
const POWER_SOFTCONN: u8 = 0x40;
const FIFO_START: u16 = 64;
const FIFO_LIMIT: u16 = 4096;
const INTRTX: usize = BASE + 0x02;
const INTRRX: usize = BASE + 0x04;
const INTRTXE: usize = BASE + 0x06;
const INTRRXE: usize = BASE + 0x08;
const INTRUSBE: usize = BASE + 0x0b;
const WAKE_INTRTXE: u16 = 0x0007;
const WAKE_INTRRXE: u16 = 0x0008;
const WAKE_INTRUSBE: u8 = 0x07;
static mut EP0_WAIT_STATUS: bool = false;
const INDEX: usize = BASE + 0x0e;
const CSR: usize = BASE + 0x12;
const RXCSR: usize = BASE + 0x16;
const COUNT: usize = BASE + 0x18;
const TX_MODE: u16 = 0x2000;
const TX_RESET: u16 = TX_MODE | 0x48;
const RX_RESET: u16 = 0x90;
const ISO: u16 = 0x4000;
const PACKET_READY: u16 = 1;
pub const OVERRUN: u16 = 0x04;
pub const DAMAGED: u16 = 0x08;
pub const UNDERRUN: u16 = 0x04;
const FIFO_DOUBLE: u8 = 0x10;

fn read8(address: usize) -> u8 {
    unsafe { read_volatile(address as *const u8) }
}

fn write8(address: usize, value: u8) {
    unsafe { write_volatile(address as *mut u8, value) }
}

fn read16(address: usize) -> u16 {
    unsafe { read_volatile(address as *const u16) }
}

fn write16(address: usize, value: u16) {
    unsafe { write_volatile(address as *mut u16, value) }
}

fn read32(address: usize) -> u32 {
    unsafe { read_volatile(address as *const u32) }
}

fn write32(address: usize, value: u32) {
    unsafe { write_volatile(address as *mut u32, value) }
}

fn select(ep: usize) {
    write8(INDEX, ep as u8);
}

#[unsafe(link_section = ".text.hot")]
#[inline(never)]
fn fifo_read(ep: usize, buf: &mut [u8], count: usize) {
    let fifo = BASE + 0x20 + 4 * ep;
    let mut i = 0;
    let keep = count.min(buf.len());
    if (buf.as_mut_ptr() as usize).is_multiple_of(4) {
        let words = keep & !3;
        let dst = buf.as_mut_ptr().cast::<u32>();
        while i < words {
            unsafe { dst.add(i >> 2).write(read32(fifo)) };
            i += 4;
        }
    } else {
        while i + 4 <= keep {
            buf[i..i + 4].copy_from_slice(&read32(fifo).to_le_bytes());
            i += 4;
        }
    }
    while i + 2 <= keep {
        buf[i..i + 2].copy_from_slice(&read16(fifo).to_le_bytes());
        i += 2;
    }
    while i < keep {
        buf[i] = read8(fifo);
        i += 1;
    }
    while i + 4 <= count {
        let _ = read32(fifo);
        i += 4;
    }
    while i < count {
        let _ = read8(fifo);
        i += 1;
    }
}

#[inline(never)]
fn fifo_write(ep: usize, data: &[u8]) {
    let fifo = BASE + 0x20 + 4 * ep;
    let mut i = 0;
    while i + 4 <= data.len() {
        write32(fifo, u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]));
        i += 4;
    }
    while i + 2 <= data.len() {
        write16(fifo, u16::from_le_bytes([data[i], data[i + 1]]));
        i += 2;
    }
    while i < data.len() {
        write8(fifo, data[i]);
        i += 1;
    }
}

fn slot(address: EndpointAddress) -> usize {
    address.index() + if address.is_in() { 8 } else { 0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    Full,
    High,
}

impl Speed {
    pub const fn bulk_packet(self) -> u16 {
        self.max_packet(EndpointType::Bulk)
    }

    pub const fn max_packet(self, ep_type: EndpointType) -> u16 {
        match (self, ep_type) {
            (Speed::High, EndpointType::Bulk) => 512,
            (Speed::High, EndpointType::Isochronous) => 1024,
            (Speed::Full, EndpointType::Isochronous) => 1023,
            _ => 64,
        }
    }
}

fn doubled(address: EndpointAddress, ep_type: EndpointType) -> bool {
    matches!(ep_type, EndpointType::Isochronous) && !address.is_in()
}

const fn reset_csr(is_in: bool, iso: bool) -> u16 {
    let iso = if iso { ISO } else { 0 };
    if is_in { TX_RESET | iso } else { RX_RESET | iso }
}

fn fifo_class(max_packet_size: u16) -> u16 {
    max_packet_size.max(8).next_power_of_two()
}

fn fifo_size_code(class: u16) -> u8 {
    (class.trailing_zeros() - 3) as u8
}

pub struct State {
    high_speed: Cell<bool>,
    enabled: Cell<u16>,
    iso: Cell<u16>,
    epoch: Cell<u32>,
    control_epoch: Cell<u32>,
    endpoint_epoch: [Cell<u32>; 16],
}

impl State {
    pub const fn new() -> Self {
        Self {
            high_speed: Cell::new(false),
            enabled: Cell::new(0),
            iso: Cell::new(0),
            epoch: Cell::new(0),
            control_epoch: Cell::new(0),
            endpoint_epoch: [const { Cell::new(0) }; 16],
        }
    }

    pub fn high_speed(&self) -> bool {
        self.high_speed.get()
    }

    pub fn epoch(&self) -> u32 {
        self.epoch.get()
    }

    fn iso(&self, address: EndpointAddress) -> bool {
        self.iso.get() & (1 << slot(address)) != 0
    }

    pub fn cancel_endpoint(&self, address: EndpointAddress) {
        let epoch = &self.endpoint_epoch[slot(address)];
        epoch.set(epoch.get().wrapping_add(1));
        select(address.index());
        let iso = self.iso(address);
        let reg = if address.is_in() { CSR } else { RXCSR };
        let value = reset_csr(address.is_in(), iso);
        write16(reg, value);
        if !address.is_in() && iso {
            write16(reg, value);
        }
    }

    pub fn set_enabled(&self, address: EndpointAddress, enabled: bool) {
        let bit = 1 << slot(address);
        self.enabled.set(if enabled { self.enabled.get() | bit } else { self.enabled.get() & !bit });
        self.cancel_endpoint(address);
        if !enabled {
            self.epoch.set(self.epoch.get().wrapping_add(1));
        }
    }
}

pub struct Driver<'d> {
    state: &'d State,
    speed: Speed,
    allocated: [Option<EndpointInfo>; 16],
    fifo: [u16; 16],
    next_fifo: u16,
}

impl<'d> Driver<'d> {
    pub fn new(state: &'d State, speed: Speed) -> Self {
        Self { state, speed, allocated: [None; 16], fifo: [0; 16], next_fifo: FIFO_START }
    }

    fn number_free(&self, ep: usize) -> bool {
        self.allocated[ep].is_none() && self.allocated[ep + 8].is_none()
    }

    fn alloc(
        &mut self,
        dir: Direction,
        ep_type: EndpointType,
        address: Option<EndpointAddress>,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Endpoint<'d>, EndpointAllocError> {
        if !matches!(ep_type, EndpointType::Bulk | EndpointType::Interrupt | EndpointType::Isochronous)
            || !(1..=self.speed.max_packet(ep_type)).contains(&max_packet_size)
        {
            return Err(EndpointAllocError);
        }
        let address = match address {
            Some(a) if a.direction() == dir && (1..=7).contains(&a.index()) => a,
            Some(_) => return Err(EndpointAllocError),
            None => (1..=7)
                .map(|ep| EndpointAddress::from_parts(ep, dir))
                .find(|a| self.number_free(a.index()))
                .ok_or(EndpointAllocError)?,
        };
        if !self.number_free(address.index()) {
            return Err(EndpointAllocError);
        }
        let class = fifo_class(max_packet_size) << u16::from(doubled(address, ep_type));
        let end = self.next_fifo.checked_add(class).filter(|end| *end <= FIFO_LIMIT).ok_or(EndpointAllocError)?;
        if matches!(ep_type, EndpointType::Isochronous) {
            self.state.iso.set(self.state.iso.get() | 1 << slot(address));
        }
        self.fifo[slot(address)] = self.next_fifo;
        self.next_fifo = end;
        let info = EndpointInfo { addr: address, ep_type, max_packet_size, interval_ms };
        self.allocated[slot(address)] = Some(info);
        Ok(Endpoint { state: self.state, info })
    }
}

impl<'d> DriverTrait<'d> for Driver<'d> {
    type EndpointOut = Endpoint<'d>;
    type EndpointIn = Endpoint<'d>;
    type ControlPipe = Control<'d>;
    type Bus = Bus<'d>;

    fn alloc_endpoint_out(
        &mut self,
        ep_type: EndpointType,
        ep_addr: Option<EndpointAddress>,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointOut, EndpointAllocError> {
        self.alloc(Direction::Out, ep_type, ep_addr, max_packet_size, interval_ms)
    }

    fn alloc_endpoint_in(
        &mut self,
        ep_type: EndpointType,
        ep_addr: Option<EndpointAddress>,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointIn, EndpointAllocError> {
        self.alloc(Direction::In, ep_type, ep_addr, max_packet_size, interval_ms)
    }

    fn start(self, control_max_packet_size: u16) -> (Self::Bus, Self::ControlPipe) {
        assert_eq!(control_max_packet_size, 64);
        write8(POWER, read8(POWER) & !(POWER_HSENAB | POWER_SOFTCONN));
        write16(BASE + 6, 0);
        write16(BASE + 8, 0);
        write8(BASE + 0x0b, 0);
        (
            Bus { state: self.state, speed: self.speed, allocated: self.allocated, fifo: self.fifo, powered: false },
            Control { state: self.state, speed: self.speed, epoch: 0, unserviced: false },
        )
    }
}

pub struct Bus<'d> {
    state: &'d State,
    speed: Speed,
    allocated: [Option<EndpointInfo>; 16],
    fifo: [u16; 16],
    powered: bool,
}

impl Bus<'_> {
    fn reset(&mut self) {
        self.state.enabled.set(0);
        self.state.epoch.set(self.state.epoch.get().wrapping_add(1));
        self.state.control_epoch.set(self.state.control_epoch.get().wrapping_add(1));
        unsafe { write_volatile(&raw mut EP0_WAIT_STATUS, false) };
        write8(BASE, 0);
        write16(INTRTXE, WAKE_INTRTXE);
        write16(INTRRXE, WAKE_INTRRXE);
        write8(INTRUSBE, WAKE_INTRUSBE);
        self.state.high_speed.set(read8(POWER) & POWER_HSMODE != 0);
        for info in self.allocated.iter().flatten() {
            let ep = info.addr.index();
            select(ep);
            let offset = if info.addr.is_in() { 0 } else { 1 };
            let double = if doubled(info.addr, info.ep_type) { FIFO_DOUBLE } else { 0 };
            write8(BASE + 0x62 + offset, fifo_size_code(fifo_class(info.max_packet_size)) | double);
            write16(BASE + 0x64 + 2 * offset, self.fifo[slot(info.addr)] / 8);
            write16(BASE + if info.addr.is_in() { 0x10 } else { 0x14 }, info.max_packet_size);
            let iso = matches!(info.ep_type, EndpointType::Isochronous);
            let reg = if info.addr.is_in() { CSR } else { RXCSR };
            let value = reset_csr(info.addr.is_in(), iso);
            write16(reg, value);
            if doubled(info.addr, info.ep_type) {
                write16(reg, value);
            }
        }
    }
}

impl BusTrait for Bus<'_> {
    async fn enable(&mut self) {
        let high_speed = if self.speed == Speed::High { POWER_HSENAB } else { 0 };
        write8(POWER, (read8(POWER) & !POWER_HSENAB) | POWER_SOFTCONN | high_speed);
        write16(INTRTXE, WAKE_INTRTXE);
        write16(INTRRXE, WAKE_INTRRXE);
        write8(INTRUSBE, WAKE_INTRUSBE);
    }

    async fn disable(&mut self) {
        write8(POWER, read8(POWER) & !POWER_SOFTCONN);
        self.reset();
    }

    async fn poll(&mut self) -> Event {
        if !self.powered {
            self.powered = true;
            return Event::PowerDetected;
        }
        poll_fn(|_| {
            let flags = read8(BASE + 0x0a);
            if flags & 4 != 0 {
                self.reset();
                Poll::Ready(Event::Reset)
            } else if flags & 1 != 0 {
                Poll::Ready(Event::Suspend)
            } else if flags & 2 != 0 {
                Poll::Ready(Event::Resume)
            } else {
                Poll::Pending
            }
        })
        .await
    }

    fn endpoint_set_enabled(&mut self, address: EndpointAddress, enabled: bool) {
        self.state.set_enabled(address, enabled);
    }

    fn endpoint_set_stalled(&mut self, address: EndpointAddress, stalled: bool) {
        select(address.index());
        let (csr, bit, clear) = if address.is_in() { (CSR, 0x10, 0x40) } else { (RXCSR, 0x20, 0x80) };
        write16(csr, if stalled { read16(csr) | bit } else { (read16(csr) & !(bit | (bit << 1))) | clear });
    }

    fn endpoint_is_stalled(&mut self, address: EndpointAddress) -> bool {
        select(address.index());
        read16(if address.is_in() { CSR } else { RXCSR }) & if address.is_in() { 0x30 } else { 0x60 } != 0
    }

    async fn remote_wakeup(&mut self) -> Result<(), Unsupported> {
        trigger_remote_wakeup();
        Ok(())
    }
}

#[unsafe(link_section = ".text.hot")]
pub fn prepare_idle_wake() -> bool {
    if unsafe { read_volatile(&raw const EP0_WAIT_STATUS) } {
        return true;
    }
    write16(INTRTXE, WAKE_INTRTXE);
    write16(INTRRXE, WAKE_INTRRXE);
    write8(INTRUSBE, WAKE_INTRUSBE);
    let tx = read16(INTRTX);
    let rx = read16(INTRRX);
    if (tx | rx) != 0 {
        return true;
    }
    select(0);
    if read16(CSR) & (PACKET_READY | 0x10) != 0 {
        return true;
    }
    select(3);
    read16(RXCSR) & PACKET_READY != 0
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn trigger_remote_wakeup() {
    let power = read8(POWER);
    write8(POWER, power | POWER_RESUME);
    crate::board::kick_watchdog();
    crate::board::delay_us_at_mhz(10_000, crate::board::cpu_mhz());
    crate::board::kick_watchdog();
    write8(POWER, read8(POWER) & !POWER_RESUME);
}

pub struct Endpoint<'d> {
    state: &'d State,
    info: EndpointInfo,
}

impl Endpoint<'_> {
    fn enabled(&self) -> bool {
        self.state.enabled.get() & (1 << slot(self.info.addr)) != 0
    }

    fn tx_mode(&self) -> u16 {
        TX_MODE | if self.state.iso(self.info.addr) { ISO } else { 0 }
    }

    #[unsafe(link_section = ".text.hot")]
    pub fn readable(&self) -> bool {
        if !self.enabled() {
            return false;
        }
        select(self.info.addr.index());
        read16(RXCSR) & (PACKET_READY | OVERRUN) != 0
    }

    #[unsafe(link_section = ".text.hot")]
    pub fn receive(&mut self, buf: &mut [u8]) -> Option<(usize, u16)> {
        if !self.enabled() {
            return None;
        }
        select(self.info.addr.index());
        let csr = read16(RXCSR);
        let flags = csr & (OVERRUN | DAMAGED);
        if csr & PACKET_READY == 0 {
            if flags & OVERRUN != 0 {
                write16(RXCSR, csr & !OVERRUN);
                return Some((0, OVERRUN));
            }
            return None;
        }
        let count = read16(COUNT) as usize;
        fifo_read(self.info.addr.index(), buf, count);
        write16(RXCSR, csr & !(PACKET_READY | OVERRUN));
        Some(if count <= buf.len() { (count, flags) } else { (0, flags | DAMAGED) })
    }

    #[unsafe(link_section = ".text.hot")]
    pub fn writable(&mut self) -> bool {
        if !self.enabled() {
            return false;
        }
        select(self.info.addr.index());
        read16(CSR) & (PACKET_READY | 0x30) == 0
    }

    #[unsafe(link_section = ".text.hot")]
    pub fn offer(&mut self, data: &[u8]) -> Option<u16> {
        if !self.writable() || data.len() > usize::from(self.info.max_packet_size) {
            return None;
        }
        let csr = read16(CSR);
        fifo_write(self.info.addr.index(), data);
        write16(CSR, self.tx_mode() | PACKET_READY);
        Some(csr & UNDERRUN)
    }
}

impl EndpointTrait for Endpoint<'_> {
    fn info(&self) -> &EndpointInfo {
        &self.info
    }

    async fn wait_enabled(&mut self) {
        poll_fn(|_| if self.enabled() { Poll::Ready(()) } else { Poll::Pending }).await
    }
}

impl EndpointOut for Endpoint<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, EndpointError> {
        let epoch = self.state.epoch.get();
        let endpoint_epoch = self.state.endpoint_epoch[slot(self.info.addr)].get();
        poll_fn(|_| {
            if !self.enabled()
                || self.state.epoch.get() != epoch
                || self.state.endpoint_epoch[slot(self.info.addr)].get() != endpoint_epoch
            {
                return Poll::Ready(Err(EndpointError::Disabled));
            }
            select(self.info.addr.index());
            if read16(RXCSR) & 1 == 0 {
                return Poll::Pending;
            }
            let count = read16(COUNT) as usize;
            fifo_read(self.info.addr.index(), buf, count);
            write16(RXCSR, read16(RXCSR) & !1);
            Poll::Ready(if count <= buf.len() { Ok(count) } else { Err(EndpointError::BufferOverflow) })
        })
        .await
    }
}

impl EndpointIn for Endpoint<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<(), EndpointError> {
        if buf.len() > self.info.max_packet_size as usize {
            return Err(EndpointError::BufferOverflow);
        }
        let epoch = self.state.epoch.get();
        let endpoint_epoch = self.state.endpoint_epoch[slot(self.info.addr)].get();
        let mut sent = false;
        poll_fn(|_| {
            if !self.enabled()
                || self.state.epoch.get() != epoch
                || self.state.endpoint_epoch[slot(self.info.addr)].get() != endpoint_epoch
            {
                return Poll::Ready(Err(EndpointError::Disabled));
            }
            select(self.info.addr.index());
            let csr = read16(CSR);
            if csr & 1 != 0 || csr & 0x30 != 0 {
                return Poll::Pending;
            }
            if sent {
                return Poll::Ready(Ok(()));
            }
            fifo_write(self.info.addr.index(), buf);
            write16(CSR, self.tx_mode() | 1);
            sent = true;
            Poll::Pending
        })
        .await
    }
}

pub struct Control<'d> {
    state: &'d State,
    speed: Speed,
    epoch: u32,
    unserviced: bool,
}

impl Control<'_> {
    fn cancelled(&self) -> bool {
        self.state.control_epoch.get() != self.epoch || read16(CSR) & 0x10 != 0
    }

    fn finish(&mut self, flags: u16) {
        select(0);
        if !self.cancelled() {
            write16(CSR, if self.unserviced { 0x40 } else { 0 } | flags);
        }
        self.unserviced = false;
    }

    async fn wait_tx(&self) -> Result<(), EndpointError> {
        poll_fn(|_| {
            select(0);
            if self.cancelled() {
                Poll::Ready(Err(EndpointError::Disabled))
            } else if read16(CSR) & 2 == 0 {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

impl ControlTrait for Control<'_> {
    fn max_packet_size(&self) -> usize {
        64
    }

    async fn setup(&mut self) -> [u8; 8] {
        let setup = poll_fn(|_| {
            select(0);
            let csr = read16(CSR);
            if csr & 0x10 != 0 {
                write16(CSR, 0x80);
            }
            if csr & 4 != 0 {
                write16(CSR, 0);
            }
            if csr & 1 == 0 {
                return Poll::Pending;
            }
            let count = read16(COUNT) as usize;
            let mut setup = [0u8; 8];
            fifo_read(0, &mut setup, count);
            if count != 8 {
                write16(CSR, 0x40);
                return Poll::Pending;
            }
            if setup[0] == 0x80 && setup[1] == 6 && (setup[3] == 7 || (setup[3] == 6 && self.speed == Speed::Full)) {
                write16(CSR, 0x60);
                return Poll::Pending;
            }
            if setup[6] != 0 || setup[7] != 0 {
                write16(CSR, 0x40);
            }
            Poll::Ready(setup)
        })
        .await;
        self.unserviced = setup[6] == 0 && setup[7] == 0;
        self.epoch = self.state.control_epoch.get();
        setup
    }

    async fn data_out(&mut self, buf: &mut [u8], _: bool, last: bool) -> Result<usize, EndpointError> {
        let result = poll_fn(|_| {
            select(0);
            if self.cancelled() {
                return Poll::Ready(Err(EndpointError::Disabled));
            }
            if read16(CSR) & 1 == 0 {
                return Poll::Pending;
            }
            let count = read16(COUNT) as usize;
            fifo_read(0, buf, count);
            if !last {
                write16(CSR, 0x40);
            }
            Poll::Ready(if count <= buf.len() { Ok(count) } else { Err(EndpointError::BufferOverflow) })
        })
        .await;
        self.unserviced = last && result.is_ok();
        result
    }

    async fn data_in(&mut self, data: &[u8], _: bool, last: bool) -> Result<(), EndpointError> {
        if data.len() > 64 {
            return Err(EndpointError::BufferOverflow);
        }
        self.wait_tx().await?;
        select(0);
        fifo_write(0, data);
        write16(CSR, 2 | if last { 8 } else { 0 });
        self.wait_tx().await
    }

    async fn accept(&mut self) {
        self.finish(0x08);
    }

    async fn reject(&mut self) {
        self.finish(0x20);
    }

    async fn accept_set_address(&mut self, addr: u8) {
        unsafe { write_volatile(&raw mut EP0_WAIT_STATUS, true) };
        select(0);
        read16(INTRTX);
        self.finish(0x08);
        let completed = poll_fn(|_| {
            select(0);
            if read16(INTRTX) & 1 != 0 {
                Poll::Ready(read16(CSR) & 0x11 == 0 && !self.cancelled())
            } else if self.cancelled() {
                Poll::Ready(false)
            } else {
                Poll::Pending
            }
        })
        .await;
        unsafe { write_volatile(&raw mut EP0_WAIT_STATUS, false) };
        if completed {
            write8(BASE, addr);
        }
    }
}
