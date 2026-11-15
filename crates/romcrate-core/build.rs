//! Embeds optional ScreenScraper *developer* credentials (the app's own
//! registration, not a user account) from the build environment.
//! They come from GitHub Actions secrets and are lightly obfuscated so they
//! don't show up as plain text in the binary. Builds without them simply
//! disable the ScreenScraper guest source.

use std::{env, fs, path::Path};

fn scramble(s: &str, key: &[u8]) -> Vec<u8> {
    s.bytes()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect()
}

fn main() {
    println!("cargo:rerun-if-env-changed=SS_DEVID");
    println!("cargo:rerun-if-env-changed=SS_DEVPASSWORD");
    let key: Vec<u8> = (0..32u8)
        .map(|i| i.wrapping_mul(73).wrapping_add(41))
        .collect();
    let id = env::var("SS_DEVID").unwrap_or_default();
    let pw = env::var("SS_DEVPASSWORD").unwrap_or_default();
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("ss_creds.rs");
    fs::write(
        out,
        format!(
            "pub const SS_KEY: &[u8] = &{:?};\npub const SS_DEVID: &[u8] = &{:?};\npub const SS_DEVPASSWORD: &[u8] = &{:?};\n",
            key,
            scramble(&id, &key),
            scramble(&pw, &key)
        ),
    )
    .unwrap();
}
