//! Host tool to generate signing keys and mint, inspect, and verify
//! update capsules (workplan WP-8).
//!
//! ```text
//! update-capsule-cli keygen --out-secret sk.bin --out-public pk.bin [--seed <hex32>]
//! update-capsule-cli sign   --secret sk.bin --payload code.bin --out capsule.bin
//!                           --payload-type 1 --slot 3 --platform 1 --abi 1
//!                           --version 5 --key-id 1 --key-epoch 1
//!                           [--load-vaddr 0x40000000] [--entry-offset 0]
//!                           [--deps-sha256 <hex32>]
//! update-capsule-cli show   capsule.bin
//! update-capsule-cli verify capsule.bin --public pk.bin [--platform N]
//!                           [--counter N] [--region-base N] [--region-size N]
//! ```
//!
//! `verify` is a development aid: it builds a single-slot [`SystemProfile`]
//! from the capsule header (overridable via flags) and runs the same
//! pipeline a verifier PD would. A real verifier pins its profile; it
//! never derives it from the capsule.

use std::process::exit;

use update_capsule::header::{self, HEADER_LEN, SIGNED_PREFIX_LEN};
use update_capsule::mint::{derive_public_key, mint, CapsuleSpec};
use update_capsule::verify::{
    verify_capsule, RollbackStore, SlotPolicy, SystemProfile, TrustedKey,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("keygen") => cmd_keygen(&args[1..]),
        Some("sign") => cmd_sign(&args[1..]),
        Some("show") => cmd_show(&args[1..]),
        Some("verify") => cmd_verify(&args[1..]),
        _ => {
            eprintln!("usage: update-capsule-cli <keygen|sign|show|verify> [options]");
            eprintln!("       (see crate README for details)");
            2
        }
    };
    exit(code);
}

// ---------------------------------------------------------------------------
// argument helpers (deliberately dependency-free)
// ---------------------------------------------------------------------------

struct Opts<'a>(&'a [String]);

impl<'a> Opts<'a> {
    fn get(&self, flag: &str) -> Option<&'a str> {
        self.0
            .iter()
            .position(|a| a == flag)
            .and_then(|i| self.0.get(i + 1))
            .map(String::as_str)
    }

    fn require(&self, flag: &str) -> String {
        match self.get(flag) {
            Some(v) => v.to_string(),
            None => {
                eprintln!("error: missing required option {flag} <value>");
                exit(2);
            }
        }
    }

    /// First positional (non-flag) argument.
    fn positional(&self) -> Option<&'a str> {
        let mut skip = false;
        for a in self.0 {
            if skip {
                skip = false;
                continue;
            }
            if a.starts_with("--") {
                skip = true;
                continue;
            }
            return Some(a);
        }
        None
    }
}

fn parse_int(s: &str, flag: &str) -> u64 {
    let r = if let Some(hex) = s.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        s.parse::<u64>()
    };
    match r {
        Ok(v) => v,
        Err(_) => {
            eprintln!("error: {flag}: not a number: {s}");
            exit(2);
        }
    }
}

fn narrow<T: TryFrom<u64>>(v: u64, flag: &str) -> T {
    match T::try_from(v) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("error: {flag}: value {v} out of range");
            exit(2);
        }
    }
}

fn parse_hex32(s: &str, flag: &str) -> [u8; 32] {
    let bytes = s.as_bytes();
    if bytes.len() != 64 || !bytes.iter().all(u8::is_ascii_hexdigit) {
        eprintln!("error: {flag}: expected 64 hex chars");
        exit(2);
    }
    let mut out = [0u8; 32];
    for (i, chunk) in bytes.chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap();
    }
    out
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn read_file(path: &str) -> Vec<u8> {
    match std::fs::read(path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: reading {path}: {e}");
            exit(1);
        }
    }
}

fn read_key32(path: &str) -> [u8; 32] {
    let data = read_file(path);
    match <[u8; 32]>::try_from(data.as_slice()) {
        Ok(k) => k,
        Err(_) => {
            eprintln!(
                "error: {path}: expected exactly 32 bytes, got {}",
                data.len()
            );
            exit(1);
        }
    }
}

fn write_file(path: &str, data: &[u8]) {
    if let Err(e) = std::fs::write(path, data) {
        eprintln!("error: writing {path}: {e}");
        exit(1);
    }
}

// ---------------------------------------------------------------------------
// subcommands
// ---------------------------------------------------------------------------

fn cmd_keygen(args: &[String]) -> i32 {
    let opts = Opts(args);
    let out_secret = opts.require("--out-secret");
    let out_public = opts.require("--out-public");

    let secret: [u8; 32] = match opts.get("--seed") {
        // Deterministic keys for tests and golden vectors only.
        Some(seed) => parse_hex32(seed, "--seed"),
        None => {
            let mut sk = [0u8; 32];
            if let Err(e) = getrandom::fill(&mut sk) {
                eprintln!("error: system RNG unavailable: {e}");
                return 1;
            }
            sk
        }
    };
    let public = derive_public_key(&secret);

    write_file(&out_secret, &secret);
    write_file(&out_public, &public);
    println!("public key: {}", hex(&public));
    0
}

fn cmd_sign(args: &[String]) -> i32 {
    let opts = Opts(args);
    let secret = read_key32(&opts.require("--secret"));
    let payload = read_file(&opts.require("--payload"));
    let out = opts.require("--out");

    let spec = CapsuleSpec {
        payload_type: narrow(
            parse_int(&opts.require("--payload-type"), "--payload-type"),
            "--payload-type",
        ),
        target_slot: narrow(parse_int(&opts.require("--slot"), "--slot"), "--slot"),
        target_platform: narrow(
            parse_int(&opts.require("--platform"), "--platform"),
            "--platform",
        ),
        abi_version: narrow(parse_int(&opts.require("--abi"), "--abi"), "--abi"),
        monotonic_version: parse_int(&opts.require("--version"), "--version"),
        load_vaddr: opts
            .get("--load-vaddr")
            .map_or(0, |v| parse_int(v, "--load-vaddr")),
        entry_offset: opts
            .get("--entry-offset")
            .map_or(0, |v| parse_int(v, "--entry-offset")),
        // IC-2: MUST be 0 until a trusted time source is specified; the
        // CLI cannot mint what every verifier must reject.
        not_after: 0,
        signer_key_id: narrow(parse_int(&opts.require("--key-id"), "--key-id"), "--key-id"),
        key_epoch: narrow(
            parse_int(&opts.require("--key-epoch"), "--key-epoch"),
            "--key-epoch",
        ),
        deps_sha256: opts
            .get("--deps-sha256")
            .map_or([0u8; 32], |v| parse_hex32(v, "--deps-sha256")),
    };

    match mint(&spec, &payload, &secret) {
        Ok(capsule) => {
            write_file(&out, &capsule);
            println!(
                "wrote {} ({} bytes, payload {} bytes)",
                out,
                capsule.len(),
                payload.len()
            );
            0
        }
        Err(e) => {
            eprintln!("error: minting failed: {e:?}");
            1
        }
    }
}

fn cmd_show(args: &[String]) -> i32 {
    let opts = Opts(args);
    let Some(path) = opts.positional() else {
        eprintln!("usage: update-capsule-cli show <capsule>");
        return 2;
    };
    let capsule = read_file(path);
    let h = match header::parse(&capsule) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("rejected: {e:?}");
            return 1;
        }
    };
    println!("format_version:    2");
    println!("payload_type:      {}", h.payload_type);
    println!("target_slot:       {}", h.target_slot);
    println!("target_platform:   {}", h.target_platform);
    println!("abi_version:       {}", h.abi_version);
    println!("monotonic_version: {}", h.monotonic_version);
    println!("payload_len:       {}", h.payload_len);
    println!("load_vaddr:        {:#x}", h.load_vaddr);
    println!("entry_offset:      {:#x}", h.entry_offset);
    println!("not_after:         {}", h.not_after);
    println!("signer_key_id:     {}", h.signer_key_id);
    println!("key_epoch:         {}", h.key_epoch);
    println!(
        "payload_sha256:    {}",
        hex(header::payload_sha256_field(&capsule))
    );
    println!(
        "deps_sha256:       {}",
        hex(header::deps_sha256_field(&capsule))
    );
    println!(
        "signature:         {}",
        hex(header::signature_field(&capsule))
    );
    0
}

struct CliCounter(u64);

impl RollbackStore for CliCounter {
    fn current(&self, _slot: u8, _payload_type: u8) -> Option<u64> {
        Some(self.0)
    }
}

fn cmd_verify(args: &[String]) -> i32 {
    let opts = Opts(args);
    let Some(path) = opts.positional() else {
        eprintln!("usage: update-capsule-cli verify <capsule> --public pk.bin [options]");
        return 2;
    };
    let capsule = read_file(path);
    let public_key = read_key32(&opts.require("--public"));

    // Development default: adopt the header's own claims for the parts
    // of the profile not overridden, so `verify` checks structure, hash,
    // signature, and rollback. A real verifier PD pins all of this.
    let h = match header::parse(&capsule) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("rejected: {e:?}");
            return 1;
        }
    };
    let mut deps = [0u8; 32];
    deps.copy_from_slice(header::deps_sha256_field(&capsule));

    let keys = [TrustedKey {
        key_id: h.signer_key_id,
        key_epoch: h.key_epoch,
        public_key,
    }];
    let slots = [SlotPolicy {
        slot: h.target_slot,
        payload_type: h.payload_type,
        abi_version: opts
            .get("--abi")
            .map_or(h.abi_version, |v| narrow(parse_int(v, "--abi"), "--abi")),
        region_base: opts
            .get("--region-base")
            .map_or(h.load_vaddr, |v| parse_int(v, "--region-base")),
        region_size: opts
            .get("--region-size")
            .map_or(h.payload_len, |v| parse_int(v, "--region-size")),
        slot_generation: 0,
        deps_sha256: deps,
    }];
    let profile = SystemProfile {
        platform: opts.get("--platform").map_or(h.target_platform, |v| {
            narrow(parse_int(v, "--platform"), "--platform")
        }),
        keys: &keys,
        slots: &slots,
    };
    let counter = CliCounter(
        opts.get("--counter")
            .map_or(0, |v| parse_int(v, "--counter")),
    );

    let mut scratch =
        vec![0u8; SIGNED_PREFIX_LEN + (capsule.len() - HEADER_LEN.min(capsule.len()))];
    match verify_capsule(&capsule, &profile, &counter, 1, &mut scratch) {
        Ok(auth) => {
            println!("OK: eligible for installation");
            println!("  target_slot:       {}", auth.target_slot);
            println!("  payload_type:      {}", auth.payload_type);
            println!("  monotonic_version: {}", auth.monotonic_version);
            println!("  payload_sha256:    {}", hex(&auth.payload_sha256));
            0
        }
        Err(e) => {
            eprintln!("rejected: {e:?}");
            1
        }
    }
}
