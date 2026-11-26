// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::io::{self, ErrorKind, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use kumi_icd::{USB_PRODUCT_ID, USB_VENDOR_ID};

use crate::wire::FrameLink;

pub const MAX_FRAME: usize = 4096;
pub const MAX_MESSAGE: usize = 18 + 4096;
pub const DEVICE_DESCRIPTOR: usize = 18;
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
pub const POLL: Duration = Duration::from_millis(50);
pub const DEFAULT_DEADLINE: u32 = 900;
pub const MAX_DEADLINE: u32 = 86_400;
pub const HELPER_MODE: &str = "kumi-bridge";

pub fn encode(frame: &[u8]) -> io::Result<Vec<u8>> {
    if frame.is_empty() || frame.len() > MAX_FRAME {
        return Err(io::Error::new(ErrorKind::InvalidInput, "bridge frames hold 1..4096 bytes"));
    }
    let mut message = Vec::with_capacity(frame.len() + 2);
    message.extend_from_slice(&(frame.len() as u16).to_le_bytes());
    message.extend_from_slice(frame);
    Ok(message)
}

pub fn read_message<R: Read>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut prefix = [0u8; 2];
    loop {
        match reader.read(&mut prefix[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    reader.read_exact(&mut prefix[1..])?;
    let length = usize::from(u16::from_le_bytes(prefix));
    if length == 0 || length > MAX_MESSAGE {
        return Err(io::Error::new(ErrorKind::InvalidData, format!("bridge message of {length} bytes")));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

pub fn is_kumi_descriptor(identity: &[u8]) -> bool {
    identity.len() >= DEVICE_DESCRIPTOR
        && identity[1] == 1
        && u16::from_le_bytes([identity[8], identity[9]]) == USB_VENDOR_ID
        && u16::from_le_bytes([identity[10], identity[11]]) == USB_PRODUCT_ID
}

fn plain(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b))
}

fn usb_path_ok(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.len() == 6
        && parts[..4] == ["", "dev", "bus", "usb"]
        && parts[4..].iter().all(|part| part.len() == 3 && part.bytes().all(|b| b.is_ascii_digit()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelperMode {
    Kumi,
    Bridge,
    InstallBridge,
}

impl HelperMode {
    pub fn as_str(self) -> &'static str {
        match self {
            HelperMode::Kumi => HELPER_MODE,
            HelperMode::Bridge => "bridge",
            HelperMode::InstallBridge => "install-bridge",
        }
    }
}

pub const LOCAL: &str = "local";

pub fn helper_command(helper: &str, usb_path: &str, deadline: u32) -> io::Result<String> {
    helper_command_for(helper, usb_path, deadline, HelperMode::Kumi)
}

pub fn helper_command_for(helper: &str, usb_path: &str, deadline: u32, mode: HelperMode) -> io::Result<String> {
    if !plain(helper) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "the helper path may hold letters, digits and _./- only"));
    }
    if !usb_path_ok(usb_path) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "the USB path must look like /dev/bus/usb/001/012"));
    }
    if !(1..=MAX_DEADLINE).contains(&deadline) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "the deadline must be 1..86400 seconds"));
    }
    Ok(format!("timeout -s KILL {deadline} {helper} {} {usb_path}", mode.as_str()))
}

pub fn phone_command(serial: &str, remote: &str) -> Command {
    if serial == LOCAL {
        let mut command = Command::new("sh");
        command.args(["-c", remote]);
        return command;
    }
    let adb = std::env::var_os("KUMI_ADB").unwrap_or_else(|| "adb".into());
    let mut command = Command::new(adb);
    command.args(["-s", serial, "shell", "-T", remote]);
    command
}

pub fn adb_command(serial: &str, helper: &str, usb_path: &str, deadline: u32) -> io::Result<Command> {
    Ok(phone_command(serial, &helper_command(helper, usb_path, deadline)?))
}

pub struct HelperProcess {
    child: Child,
    input: Option<ChildStdin>,
    messages: Receiver<io::Result<Vec<u8>>>,
}

impl HelperProcess {
    pub fn start(mut command: Command) -> io::Result<Self> {
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()?;
        let input = child.stdin.take();
        let mut output = child.stdout.take().ok_or_else(|| io::Error::other("the helper's stdout is unavailable"))?;
        let (sender, messages) = mpsc::channel();
        thread::spawn(move || {
            loop {
                let message = match read_message(&mut output) {
                    Ok(Some(frame)) => Ok(frame),
                    Ok(None) => Err(io::Error::new(ErrorKind::UnexpectedEof, "the phone helper closed the link")),
                    Err(error) => Err(error),
                };
                let last = message.is_err();
                if sender.send(message).is_err() || last {
                    break;
                }
            }
        });
        Ok(Self { child, input, messages })
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let input = self.input.as_mut().ok_or_else(|| io::Error::other("the link is closed"))?;
        input.write_all(bytes)?;
        input.flush()
    }

    pub fn next(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        match self.messages.recv_timeout(wait) {
            Ok(message) => message.map(Some),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                Err(io::Error::new(ErrorKind::UnexpectedEof, "the phone helper closed the link"))
            }
        }
    }

    pub fn first_message(&mut self) -> io::Result<Vec<u8>> {
        self.next(OPEN_TIMEOUT)?.ok_or_else(|| io::Error::new(ErrorKind::TimedOut, "the phone helper sent no descriptors"))
    }
}

impl Drop for HelperProcess {
    fn drop(&mut self) {
        drop(self.input.take());
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct BridgeLink {
    process: HelperProcess,
    identity: Vec<u8>,
    timeout: Duration,
}

impl BridgeLink {
    pub fn adb(serial: &str, helper: &str, usb_path: &str, deadline: u32) -> io::Result<Self> {
        Self::spawn(adb_command(serial, helper, usb_path, deadline)?)
    }

    pub fn spawn(command: Command) -> io::Result<Self> {
        let mut link = Self::start(command)?;
        link.open()?;
        Ok(link)
    }

    fn start(command: Command) -> io::Result<Self> {
        Ok(Self { process: HelperProcess::start(command)?, identity: Vec::new(), timeout: REPLY_TIMEOUT })
    }

    fn open(&mut self) -> io::Result<()> {
        let identity = self.process.first_message()?;
        if !is_kumi_descriptor(&identity) {
            return Err(io::Error::other("the phone helper did not open a Kumi 1209:0001 device"));
        }
        self.identity = identity;
        Ok(())
    }

    pub fn identity(&self) -> &[u8] {
        &self.identity
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
}

impl FrameLink for BridgeLink {
    type Error = io::Error;

    fn send(&mut self, frame: &[u8]) -> io::Result<()> {
        self.process.write(&encode(frame)?)
    }

    fn receive(&mut self) -> io::Result<Vec<u8>> {
        let wait = self.timeout;
        self.process.next(wait)?.ok_or_else(|| io::Error::new(ErrorKind::TimedOut, "no frame from the case"))
    }

    fn try_receive(&mut self) -> io::Result<Option<Vec<u8>>> {
        self.process.next(POLL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(vendor: u16, product: u16) -> Vec<u8> {
        let mut descriptor = vec![18, 1, 0, 2, 0, 0, 0, 64];
        descriptor.extend_from_slice(&vendor.to_le_bytes());
        descriptor.extend_from_slice(&product.to_le_bytes());
        descriptor.extend_from_slice(&[0, 1, 1, 2, 0, 1]);
        descriptor
    }

    #[test]
    fn messages_round_trip_and_bad_lengths_are_refused() {
        let mut stream = Vec::new();
        for frame in [vec![1u8], vec![7u8; 512], vec![9u8; MAX_FRAME]] {
            stream.extend(encode(&frame).unwrap());
        }
        let mut reader = stream.as_slice();
        assert_eq!(read_message(&mut reader).unwrap().unwrap(), vec![1u8]);
        assert_eq!(read_message(&mut reader).unwrap().unwrap().len(), 512);
        assert_eq!(read_message(&mut reader).unwrap().unwrap().len(), MAX_FRAME);
        assert_eq!(read_message(&mut reader).unwrap(), None);
        assert!(encode(&[]).is_err());
        assert!(encode(&vec![0; MAX_FRAME + 1]).is_err());
        assert!(read_message(&mut [0u8, 0].as_slice()).is_err());
        assert!(read_message(&mut [5u8, 0, 1].as_slice()).is_err());
    }

    #[test]
    fn the_helper_command_is_built_only_from_plain_parts() {
        assert_eq!(
            helper_command("/data/local/tmp/spv40-usb", "/dev/bus/usb/001/012", 900).unwrap(),
            "timeout -s KILL 900 /data/local/tmp/spv40-usb kumi-bridge /dev/bus/usb/001/012"
        );
        for (helper, path, deadline) in [
            ("/data/local/tmp/x; reboot", "/dev/bus/usb/001/012", 900),
            ("/data/local/tmp/x", "/dev/bus/usb/1/12", 900),
            ("/data/local/tmp/x", "/dev/bus/usb/001/012/../x", 900),
            ("", "/dev/bus/usb/001/012", 900),
            ("/h", "/dev/bus/usb/001/012", 0),
            ("/h", "/dev/bus/usb/001/012", MAX_DEADLINE + 1),
        ] {
            assert!(helper_command(helper, path, deadline).is_err(), "{helper} {path} {deadline}");
        }
        let command = adb_command("ABC123", "/h", "/dev/bus/usb/001/002", 5).unwrap();
        let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["-s", "ABC123", "shell", "-T", "timeout -s KILL 5 /h kumi-bridge /dev/bus/usb/001/002"]);
        let remote = helper_command_for("/h", "/dev/bus/usb/001/002", 5, HelperMode::InstallBridge).unwrap();
        assert_eq!(remote, "timeout -s KILL 5 /h install-bridge /dev/bus/usb/001/002");
        let local = phone_command(LOCAL, &remote);
        assert_eq!(local.get_program(), "sh");
        let args: Vec<_> = local.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["-c", remote.as_str()]);
    }

    #[test]
    fn a_loopback_helper_carries_frames_after_the_kumi_descriptor() {
        let mut link = BridgeLink::start(Command::new("cat")).unwrap();
        link.send(&descriptor(USB_VENDOR_ID, USB_PRODUCT_ID)).unwrap();
        link.open().unwrap();
        assert_eq!(link.identity(), descriptor(USB_VENDOR_ID, USB_PRODUCT_ID).as_slice());
        assert_eq!(link.try_receive().unwrap(), None);
        for size in [1usize, 63, 64, 65, 512, 4096] {
            let frame: Vec<u8> = (0..size).map(|i| i as u8).collect();
            link.send(&frame).unwrap();
            assert_eq!(link.receive().unwrap(), frame);
        }
    }

    #[test]
    fn a_foreign_device_and_a_closed_helper_are_errors() {
        let mut link = BridgeLink::start(Command::new("cat")).unwrap();
        link.send(&descriptor(0x0666, 0x0888)).unwrap();
        assert!(link.open().is_err());
        assert!(!is_kumi_descriptor(&descriptor(USB_VENDOR_ID, USB_PRODUCT_ID)[..17]));
        let mut closed = BridgeLink::start(Command::new("true")).unwrap();
        closed.set_timeout(Duration::from_secs(2));
        assert_eq!(closed.receive().unwrap_err().kind(), ErrorKind::UnexpectedEof);
    }
}
