// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use kumi_client::wire::FrameLink;
use serde_json::Value;

pub const MAX_FRAME: usize = 4096;

pub struct ChildLink {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}

impl ChildLink {
    pub fn spawn(config: &Path, server: Option<&Path>) -> io::Result<Self> {
        let settings: Value = serde_json::from_slice(&std::fs::read(config)?)?;
        let python = settings["python"].as_str().unwrap_or("python3");
        let server = match server {
            Some(path) => path.to_path_buf(),
            None => settings["server"]
                .as_str()
                .map(Into::into)
                .ok_or_else(|| io::Error::other("set \"server\" in the emulator config or pass --server"))?,
        };
        let mut child = Command::new(python)
            .arg(server)
            .arg("--config")
            .arg(config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let input = child.stdin.take().ok_or_else(|| io::Error::other("child stdin unavailable"))?;
        let output = child.stdout.take().ok_or_else(|| io::Error::other("child stdout unavailable"))?;
        Ok(Self { child, input, output })
    }

    fn exchange(&mut self, opcode: u8, data: &[u8]) -> io::Result<Vec<u8>> {
        if data.len() >= MAX_FRAME {
            return Err(io::Error::other("IPC frame too large"));
        }
        self.input.write_all(&(data.len() as u32 + 1).to_le_bytes())?;
        self.input.write_all(&[opcode])?;
        self.input.write_all(data)?;
        self.input.flush()?;
        let mut header = [0; 4];
        self.output.read_exact(&mut header)?;
        let length = u32::from_le_bytes(header) as usize;
        if !(1..=MAX_FRAME).contains(&length) {
            return Err(io::Error::other("invalid IPC response length"));
        }
        let mut reply = vec![0; length];
        self.output.read_exact(&mut reply)?;
        if reply[0] != 0 {
            return Err(io::Error::other(String::from_utf8_lossy(&reply[1..]).into_owned()));
        }
        Ok(reply[1..].to_vec())
    }

    pub fn command(&mut self, request: Value) -> io::Result<Value> {
        let data = self.exchange(3, &serde_json::to_vec(&request)?)?;
        Ok(serde_json::from_slice(&data)?)
    }
}

impl FrameLink for ChildLink {
    type Error = io::Error;

    fn send(&mut self, frame: &[u8]) -> io::Result<()> {
        self.exchange(1, frame).map(|_| ())
    }

    fn receive(&mut self) -> io::Result<Vec<u8>> {
        self.exchange(2, &[])
    }
}

impl Drop for ChildLink {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
