// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use postcard_rpc::Key;
use postcard_rpc::header::{VarHeader, VarKey, VarSeq};
use serde::Serialize;

pub use postcard_rpc::header::VarHeader as Header;

pub const FULL_SPEED_PACKET: usize = 64;
pub const HIGH_SPEED_BULK_PACKET: usize = 512;
pub const MAX_HEADER: usize = 1 + 8 + 4;
pub const MAX_FRAME: usize = MAX_HEADER + crate::MAX_PAYLOAD;
pub const TOPIC_SEQUENCE: VarSeq = VarSeq::Seq4(0);

pub fn header_len(seq: &VarSeq) -> usize {
    1 + 8
        + match seq {
            VarSeq::Seq1(_) => 1,
            VarSeq::Seq2(_) => 2,
            VarSeq::Seq4(_) => 4,
        }
}

pub fn write_header(key: Key, seq: VarSeq, out: &mut [u8]) -> Option<usize> {
    VarHeader { key: VarKey::Key8(key), seq_no: seq }.write_to_slice(out).map(|(head, _)| head.len())
}

pub fn encode<T: Serialize + ?Sized>(key: Key, seq: VarSeq, body: &T, out: &mut [u8]) -> Option<usize> {
    let head = write_header(key, seq, out)?;
    let used = postcard::to_slice(body, out.get_mut(head..)?).ok()?.len();
    Some(head + used)
}

pub fn decode(frame: &[u8]) -> Option<(Header, &[u8])> {
    VarHeader::take_from_slice(frame)
}

pub fn packets(frame: &[u8], max_packet: usize) -> impl Iterator<Item = &[u8]> {
    let max_packet = max_packet.max(1);
    let zero_length = frame.len().is_multiple_of(max_packet);
    frame.chunks(max_packet).chain(zero_length.then_some(&frame[..0]))
}

pub struct Reassembler<const N: usize> {
    max_packet: usize,
    buffer: [u8; N],
    used: usize,
    overflow: bool,
    complete: bool,
}

impl<const N: usize> Reassembler<N> {
    pub const fn new(max_packet: usize) -> Self {
        Self { max_packet, buffer: [0; N], used: 0, overflow: false, complete: false }
    }

    pub fn reset(&mut self) {
        self.used = 0;
        self.overflow = false;
        self.complete = false;
    }

    pub fn push(&mut self, packet: &[u8]) -> Option<&[u8]> {
        if self.complete {
            self.reset();
        }
        match self.buffer.get_mut(self.used..self.used + packet.len()) {
            Some(space) if !self.overflow && packet.len() <= self.max_packet => {
                space.copy_from_slice(packet);
                self.used += packet.len();
            }
            _ => self.overflow = true,
        }
        if packet.len() == self.max_packet {
            return None;
        }
        self.complete = true;
        if self.overflow || self.used == 0 { None } else { Some(&self.buffer[..self.used]) }
    }
}
