// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![allow(dead_code)]

use std::io;

use kumi_client::Client;
use kumi_client::bridge::{BridgeLink, DEFAULT_DEADLINE};
use kumi_client::virtual_case::VirtualCase;
use kumi_client::wire::{FrameLink, FrameTransport};

pub const USAGE: &str = "[--adb SERIAL --helper PATH --usb-path /dev/bus/usb/BBB/DDD [--deadline SECONDS]] [ARGS]
without --adb the example drives the in-process virtual case";

pub enum Link {
    Virtual(Box<VirtualCase>),
    Bridge(BridgeLink),
}

impl FrameLink for Link {
    type Error = io::Error;

    fn send(&mut self, frame: &[u8]) -> io::Result<()> {
        match self {
            Link::Virtual(case) => Ok(case.send(frame)?),
            Link::Bridge(link) => link.send(frame),
        }
    }

    fn receive(&mut self) -> io::Result<Vec<u8>> {
        match self {
            Link::Virtual(case) => Ok(case.receive()?),
            Link::Bridge(link) => link.receive(),
        }
    }

    fn try_receive(&mut self) -> io::Result<Option<Vec<u8>>> {
        match self {
            Link::Virtual(case) => Ok(case.try_receive()?),
            Link::Bridge(link) => link.try_receive(),
        }
    }
}

pub type Case = Client<FrameTransport<Link>>;

pub struct Options {
    pub case: Case,
    pub args: Vec<String>,
}

fn take(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, String> {
    let Some(at) = args.iter().position(|arg| arg == flag) else { return Ok(None) };
    if at + 1 >= args.len() {
        return Err(format!("{flag} needs a value\nusage: {USAGE}"));
    }
    let value = args.remove(at + 1);
    args.remove(at);
    Ok(Some(value))
}

pub fn connect() -> Result<Options, Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let serial = take(&mut args, "--adb")?;
    let helper = take(&mut args, "--helper")?;
    let usb_path = take(&mut args, "--usb-path")?;
    let deadline = take(&mut args, "--deadline")?.map(|text| text.parse::<u32>()).transpose()?.unwrap_or(DEFAULT_DEADLINE);
    let link = match (serial, helper, usb_path) {
        (Some(serial), Some(helper), Some(usb_path)) => Link::Bridge(BridgeLink::adb(&serial, &helper, &usb_path, deadline)?),
        (None, None, None) => Link::Virtual(Box::new(VirtualCase::new())),
        _ => return Err(format!("--adb, --helper and --usb-path go together\nusage: {USAGE}").into()),
    };
    Ok(Options { case: Client::over(link), args })
}

pub fn virtual_case(case: &mut Case) -> Option<&mut VirtualCase> {
    match case.link_mut() {
        Link::Virtual(virtual_case) => Some(virtual_case),
        Link::Bridge(_) => None,
    }
}

pub fn number(text: &str) -> Result<u32, String> {
    let parsed = match text.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|_| format!("{text} is not a number"))
}
