// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::fs;
use std::io::{Read, Write};
use std::process::ExitCode;

use kumi_install::elf;
use kumi_install::pack::{self, ImageCheck, ImageSpec};
use kumi_install::spv::{self, Verdict};
use kumi_layout::{FLAG_LAB, FLAG_SAFE_MODE_CAPABLE};

const USAGE: &str = "usage:
  kumi-pack keygen <secret-key-file>
  kumi-pack public <secret-key-file>
  kumi-pack stage0 <stage0-elf> <out> [<public-key-hex>]
  kumi-pack image [--lab] <firmware-elf> <out> <secret-key-file> <major.minor.patch> <generation>
  kumi-pack bundle <stage0.bin> <image.bin> <secret-key-file> <build-name> <out.kumi>
  kumi-pack inspect <file> [<public-key-hex>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&refs) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("kumi-pack: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[&str]) -> Result<(), String> {
    match args {
        ["keygen", secret] => keygen(secret),
        ["public", secret] => {
            println!("{}", pack::hex(read_secret(secret)?.verifying_key().as_bytes()));
            Ok(())
        }
        ["stage0", input, output] => stage0(input, output, None),
        ["stage0", input, output, public] => stage0(input, output, Some(public)),
        ["image", input, output, secret, version, generation] => image(input, output, secret, version, generation, false),
        ["image", "--lab", input, output, secret, version, generation] => image(input, output, secret, version, generation, true),
        ["bundle", stage0, image, secret, build, output] => bundle(stage0, image, secret, build, output),
        ["inspect", file] => inspect(file, None),
        ["inspect", file, public] => inspect(file, Some(public)),
        _ => Err(USAGE.to_string()),
    }
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn read_secret(path: &str) -> Result<ed25519_dalek::SigningKey, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    pack::signing_key(&text).map_err(|_| format!("{path}: expected 64 hex digits"))
}

fn write_new(path: &str, data: &[u8], private: bool) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(path).map_err(|e| format!("{path}: {e}"))?;
    file.write_all(data).map_err(|e| format!("{path}: {e}"))
}

fn keygen(path: &str) -> Result<(), String> {
    let mut seed = [0u8; 32];
    fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut seed)).map_err(|e| format!("/dev/urandom: {e}"))?;
    let key = ed25519_dalek::SigningKey::from_bytes(&seed);
    write_new(path, format!("{}\n", pack::hex(&seed)).as_bytes(), true)?;
    println!("{}", pack::hex(key.verifying_key().as_bytes()));
    Ok(())
}

fn stage0(input: &str, output: &str, public: Option<&str>) -> Result<(), String> {
    let elf = elf::parse(&read(input)?).map_err(|e| format!("{input}: {e:?}"))?;
    let key = public.map(pack::parse_hex32).transpose().map_err(|_| "public key: expected 64 hex digits".to_string())?;
    let image = pack::pack_stage0(&elf, key.as_ref()).map_err(|e| format!("{input}: {e:?}"))?;
    write_new(output, &image, false)?;
    println!("stage0 {} bytes, checksum 0x{:08x}", image.len(), kumi_layout::get32(&image, 0x2C));
    Ok(())
}

fn parse_version(text: &str) -> Result<[u8; 3], String> {
    let parts: Vec<u8> = text.split('.').map(str::parse).collect::<Result<_, _>>().map_err(|_| format!("version {text}"))?;
    parts.try_into().map_err(|_| format!("version {text}: expected major.minor.patch"))
}

fn image(input: &str, output: &str, secret: &str, version: &str, generation: &str, force_lab: bool) -> Result<(), String> {
    let raw = read(input)?;
    let elf = elf::parse(&raw).map_err(|e| format!("{input}: {e:?}"))?;
    let key = read_secret(secret)?;
    let generation = generation.parse().map_err(|_| format!("generation {generation}"))?;
    let is_lab = force_lab || elf::has_symbol(&raw, "KUMI_LAB_BOOT");
    let flags = FLAG_SAFE_MODE_CAPABLE | if is_lab { FLAG_LAB } else { 0 };
    let spec = ImageSpec { flags, version: parse_version(version)?, generation };
    let image = pack::pack_image(&elf, &spec, &key).map_err(|e| format!("{input}: {e:?}"))?;
    write_new(output, &image, false)?;
    println!(
        "image {} bytes, entry 0x{:08x}, flags 0x{:04x}, sha256 {}",
        image.len(),
        elf.entry,
        flags,
        pack::hex(&kumi_install::sha256(&image))
    );
    Ok(())
}

fn bundle(stage0: &str, image: &str, secret: &str, build: &str, output: &str) -> Result<(), String> {
    let key = read_secret(secret)?;
    let data = kumi_install::bundle::build(&read(stage0)?, &read(image)?, build, &key).map_err(|e| format!("{e:?}"))?;
    write_new(output, &data, false)?;
    println!("package {} bytes, build {build}, sha256 {}", data.len(), pack::hex(&kumi_install::sha256(&data)));
    Ok(())
}

fn inspect(path: &str, public: Option<&str>) -> Result<(), String> {
    let data = read(path)?;
    let key = public.map(pack::verifying_key).transpose().map_err(|_| "public key: not a valid ed25519 key".to_string())?;
    if data.starts_with(&kumi_install::bundle::MAGIC) {
        let Some(key) = key else {
            return Err("a Kumi package needs the public key to be checked".to_string());
        };
        let opened = kumi_install::bundle::open(&data, key.as_bytes()).map_err(|e| format!("{path}: {e:?}"))?;
        let v = opened.version;
        println!(
            "Kumi package {}.{}.{} ({}), generation {}, stage0 {} bytes, image {} bytes: verified",
            v[0],
            v[1],
            v[2],
            opened.build,
            opened.generation,
            opened.stage0.len(),
            opened.image.len()
        );
        return Ok(());
    }
    if data.len() >= 8 && kumi_layout::get32(&data, 4) == spv::MAGIC {
        match spv::brom_verdict(&data) {
            Verdict::Boots { length, checksummed, alias_48 } => {
                println!("SPV image: boot ROM starts it; length {length}, checksum {checksummed}, 0x48000000 entry {alias_48}")
            }
            Verdict::Service => println!("SPV image: the boot ROM would refuse it and enter service mode"),
        }
        if let Some(key) = key {
            println!("public key present: {}", data.windows(32).any(|w| w == key.as_bytes()));
        }
        return Ok(());
    }
    let Some(key) = key else {
        return Err("a Kumi image needs the public key to be checked".to_string());
    };
    let (header, check) = pack::check_image(&data, &key).map_err(|e| format!("{path}: {e:?}"))?;
    let v = header.version;
    println!("Kumi image {}.{}.{}, generation {}, entry 0x{:08x}", v[0], v[1], v[2], header.generation, header.entry);
    for segment in header.segments() {
        println!("  {:?} 0x{:08x} size {}", segment.kind, segment.destination, segment.size);
    }
    println!("check: {check:?}");
    if check == ImageCheck::Valid { Ok(()) } else { Err("image does not verify".to_string()) }
}
