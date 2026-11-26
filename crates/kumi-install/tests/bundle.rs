// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use ed25519_dalek::{Signer, SigningKey};
use kumi_install::PlanError;
use kumi_install::bundle::{self, BundleError, HEAD, SIGNATURE};
use kumi_install::sample;

fn key() -> SigningKey {
    SigningKey::from_bytes(&[9; 32])
}

fn parts(key: &SigningKey) -> (Vec<u8>, Vec<u8>) {
    (sample::stage0(&key.verifying_key().to_bytes()), sample::image(key, [0, 3, 1], 7))
}

fn resign(data: &mut Vec<u8>, key: &SigningKey) {
    let body = data.len() - SIGNATURE;
    data.truncate(body);
    let signature = key.sign(data).to_bytes();
    data.extend_from_slice(&signature);
}

#[test]
fn a_package_opens_with_its_key_and_returns_both_parts() {
    let k = key();
    let (stage0, image) = parts(&k);
    let data = bundle::build(&stage0, &image, "0.3.1-ci42", &k).unwrap();
    assert_eq!(data.len(), HEAD + stage0.len() + image.len() + SIGNATURE);
    let opened = bundle::open(&data, &k.verifying_key().to_bytes()).unwrap();
    assert_eq!(opened.version, [0, 3, 1]);
    assert_eq!(opened.generation, 7);
    assert_eq!(opened.build, "0.3.1-ci42");
    assert_eq!(opened.stage0, stage0);
    assert_eq!(opened.image, image);
}

#[test]
fn any_changed_byte_or_another_key_is_refused() {
    let k = key();
    let (stage0, image) = parts(&k);
    let data = bundle::build(&stage0, &image, "x", &k).unwrap();
    let public = k.verifying_key().to_bytes();
    for at in [0usize, 9, 30, HEAD + 5, HEAD + stage0.len() + 300, data.len() - 1] {
        let mut bad = data.clone();
        bad[at] ^= 0x01;
        assert!(bundle::open(&bad, &public).is_err(), "byte {at}");
    }
    let other = SigningKey::from_bytes(&[4; 32]).verifying_key().to_bytes();
    assert_eq!(bundle::open(&data, &other), Err(BundleError::Signature));
    assert_eq!(bundle::open(&data[..HEAD], &public), Err(BundleError::Truncated));
}

#[test]
fn a_signed_package_must_still_hold_a_bootable_stage0_with_the_key_and_a_matching_image() {
    let k = key();
    let public = k.verifying_key().to_bytes();
    let (stage0, image) = parts(&k);
    let foreign = sample::stage0(&SigningKey::from_bytes(&[5; 32]).verifying_key().to_bytes());
    assert_eq!(bundle::build(&foreign, &image, "x", &k), Err(BundleError::Stage0(PlanError::Stage0LacksKey)));
    let foreign_image = sample::image(&SigningKey::from_bytes(&[5; 32]), [0, 3, 1], 7);
    assert_eq!(bundle::build(&stage0, &foreign_image, "x", &k), Err(BundleError::Image(PlanError::ImageSignature)));
    assert_eq!(bundle::build(&stage0, &image, "a-build-name-that-is-too-long", &k), Err(BundleError::Build));

    let mut relabelled = bundle::build(&stage0, &image, "x", &k).unwrap();
    relabelled[9] = 9;
    resign(&mut relabelled, &k);
    assert_eq!(bundle::open(&relabelled, &public), Err(BundleError::VersionMismatch));

    let mut huge = bundle::build(&stage0, &image, "x", &k).unwrap();
    huge[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    resign(&mut huge, &k);
    assert_eq!(bundle::open(&huge, &public), Err(BundleError::Lengths));
}
