// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::collections::VecDeque;
use std::fmt;

use kumi_icd::frame;
use postcard_rpc::Key;
use postcard_rpc::header::{VarKey, VarSeq};
use postcard_rpc::standard_icd::{ERROR_KEY, WireError};

use crate::Transport;
use crate::events::Event;

pub const EVENT_BACKLOG: usize = 64;
pub const MAX_FOREIGN_FRAMES: usize = 256;

pub trait FrameLink {
    type Error;

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
    fn receive(&mut self) -> Result<Vec<u8>, Self::Error>;

    fn try_receive(&mut self) -> Result<Option<Vec<u8>>, Self::Error> {
        self.receive().map(Some)
    }
}

impl<L: FrameLink + ?Sized> FrameLink for Box<L> {
    type Error = L::Error;

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        (**self).send(frame)
    }

    fn receive(&mut self) -> Result<Vec<u8>, Self::Error> {
        (**self).receive()
    }

    fn try_receive(&mut self) -> Result<Option<Vec<u8>>, Self::Error> {
        (**self).try_receive()
    }
}

impl<L: FrameLink + ?Sized> FrameLink for &mut L {
    type Error = L::Error;

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        (**self).send(frame)
    }

    fn receive(&mut self) -> Result<Vec<u8>, Self::Error> {
        (**self).receive()
    }

    fn try_receive(&mut self) -> Result<Option<Vec<u8>>, Self::Error> {
        (**self).try_receive()
    }
}

#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum FrameError<E> {
    Link(E),
    Wire(WireError),
    NoReply,
}

impl<E: fmt::Display> fmt::Display for FrameError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Link(error) => write!(f, "link: {error}"),
            FrameError::Wire(error) => write!(f, "the case refused the frame: {error:?}"),
            FrameError::NoReply => f.write_str("no reply from the case"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for FrameError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FrameError::Link(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WireState {
    pub sequence: u32,
    pub events: VecDeque<Event>,
    pub dropped_events: u32,
    pub discarded_frames: u32,
}

pub struct FrameTransport<L> {
    link: L,
    state: WireState,
}

enum Sorted {
    Reply(Vec<u8>),
    Failed(WireError),
    Other,
}

impl<L: FrameLink> FrameTransport<L> {
    pub fn new(link: L) -> Self {
        Self::with_state(link, WireState::default())
    }

    pub fn with_state(link: L, state: WireState) -> Self {
        Self { link, state }
    }

    pub fn state(&self) -> &WireState {
        &self.state
    }

    pub fn into_state(self) -> WireState {
        self.state
    }

    pub fn link_mut(&mut self) -> &mut L {
        &mut self.link
    }

    pub fn into_link(self) -> L {
        self.link
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        self.state.events.drain(..).collect()
    }

    pub fn poll_events(&mut self, max_frames: usize) -> Result<Vec<Event>, FrameError<L::Error>> {
        for _ in 0..max_frames {
            match self.link.try_receive().map_err(FrameError::Link)? {
                Some(frame) => {
                    self.sort(&frame, None);
                }
                None => break,
            }
        }
        Ok(self.take_events())
    }

    fn sort(&mut self, received: &[u8], pending: Option<(u32, Key)>) -> Sorted {
        let Some((header, body)) = frame::decode(received) else {
            self.state.discarded_frames = self.state.discarded_frames.wrapping_add(1);
            return Sorted::Other;
        };
        if Event::is_topic(&header.key) {
            match Event::decode(&header.key, body) {
                Some(event) => {
                    if self.state.events.len() == EVENT_BACKLOG {
                        self.state.events.pop_front();
                        self.state.dropped_events = self.state.dropped_events.wrapping_add(1);
                    }
                    self.state.events.push_back(event);
                }
                None => self.state.dropped_events = self.state.dropped_events.wrapping_add(1),
            }
            return Sorted::Other;
        }
        if let Some((seq, response)) = pending
            && matches!(header.seq_no, VarSeq::Seq4(s) if s == seq)
        {
            if header.key == VarKey::Key8(response) {
                return Sorted::Reply(body.to_vec());
            }
            if header.key == VarKey::Key8(ERROR_KEY) {
                return Sorted::Failed(postcard::from_bytes::<WireError>(body).unwrap_or(WireError::DeserFailed));
            }
        }
        self.state.discarded_frames = self.state.discarded_frames.wrapping_add(1);
        Sorted::Other
    }
}

impl<L: FrameLink> Transport for FrameTransport<L> {
    type Error = FrameError<L::Error>;

    fn request(&mut self, request: Key, response: Key, payload: &[u8]) -> Result<Vec<u8>, Self::Error> {
        let mut seq = self.state.sequence.wrapping_add(1);
        if seq == 0 {
            seq = 1;
        }
        self.state.sequence = seq;
        let mut out = vec![0u8; frame::MAX_HEADER + payload.len()];
        let head = frame::write_header(request, VarSeq::Seq4(seq), &mut out).ok_or(FrameError::NoReply)?;
        out[head..head + payload.len()].copy_from_slice(payload);
        out.truncate(head + payload.len());
        self.link.send(&out).map_err(FrameError::Link)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        for _ in 0..MAX_FOREIGN_FRAMES {
            if std::time::Instant::now() >= deadline {
                return Err(FrameError::NoReply);
            }
            let received = self.link.receive().map_err(FrameError::Link)?;
            match self.sort(&received, Some((seq, response))) {
                Sorted::Reply(body) => return Ok(body),
                Sorted::Failed(error) => return Err(FrameError::Wire(error)),
                Sorted::Other => {}
            }
        }
        Err(FrameError::NoReply)
    }
}
