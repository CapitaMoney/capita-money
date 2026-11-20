// Capita Money – capita-issuer
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, version 3.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// for more details: <https://www.gnu.org/licenses/>.

//! Face dedup: one person, two documents (rule 33,
//! `docs/adr/0030-face-dedup.md`).
//!
//! A person key is made from name and date of birth (rule 17). One person
//! with two documents whose names are written differently – another
//! transliteration, a married name in one of them – would get two keys and
//! two accounts. Face dedup finds them at the document check: the face
//! print of the chip photo is compared with the prints of everybody with
//! the same date of birth. A match refuses nobody: the verification is held
//! until the Verein board has decided (video call, both documents).
//!
//! - Off by the rules (`identity.face_dedup`; off until the legal check
//!   P04.3): nothing here runs, nothing is kept (ADR 0017).
//! - On: the service keeps one line per person – the person key, the face
//!   print of the chip photo sealed with a key of its own, the day – in the
//!   file of a birthday group. No name, no photo, no account, no date of
//!   birth in clear: a group is named by a keyed hash of the date of birth.
//!
//! [`FileFaceStore`] keeps the lines in files, one per group, each written
//! whole and anew on every change: a print that is forgotten is gone from
//! the file, not merely marked. A lock file lets the board's commands run
//! beside the service; every change is written to a new file first and then
//! put in place, so that a power cut leaves the old file or the new one,
//! never half of one.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use capita_face::embed::{Print, DIM};
use capita_person_key::canonical::Date;
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::sign::Signer;
use openssl::symm::{decrypt_aead, encrypt_aead, Cipher};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::hex;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;
/// Bytes of a face print: 128 numbers of 4 bytes each.
const PRINT_BYTES: usize = DIM.saturating_mul(4);
/// Bytes of the nonce of AES-256-GCM.
const NONCE_BYTES: usize = 12;
/// Bytes of the tag of AES-256-GCM.
const TAG_BYTES: usize = 16;
/// Bytes of a group name: 16, written as 32 hexadecimal digits.
const GROUP_BYTES: usize = 16;

/// The secret of the face store and the two keys made from it: one names
/// the birthday groups, one seals the prints. Kept apart from the secret of
/// the person keys: whoever holds that one can test names, whoever holds
/// this one can read faces.
pub struct DedupKey {
    group: Vec<u8>,
    seal: [u8; 32],
}

impl DedupKey {
    /// The keys of a secret of at least 32 random bytes.
    ///
    /// # Errors
    /// A secret that is too short.
    pub fn from_secret(secret: &[u8]) -> Result<Self, String> {
        if secret.len() < 32 {
            return Err("the secret of the face store needs at least 32 bytes".into());
        }
        let group = mac(secret, b"capita face-dedup group v1")?;
        let seal = <[u8; 32]>::try_from(mac(secret, b"capita face-dedup seal v1")?.as_slice())
            .map_err(|_| "face store: no seal key".to_owned())?;
        Ok(Self { group, seal })
    }

    /// The name of the birthday group of a date of birth: a keyed hash, 32
    /// hexadecimal digits. The same date gives the same name; without the
    /// secret the name tells nothing.
    ///
    /// # Errors
    /// If OpenSSL fails.
    pub fn group(&self, born: Date) -> Result<String, String> {
        let day = format!("{:04}-{:02}-{:02}", born.year(), born.month(), born.day());
        let tag = mac(&self.group, day.as_bytes())?;
        Ok(hex(tag.get(..GROUP_BYTES).unwrap_or(&tag)))
    }

    fn aad(person: &str, group: &str) -> Vec<u8> {
        let mut aad = b"capita face-dedup v1\0".to_vec();
        aad.extend_from_slice(person.as_bytes());
        aad.push(0);
        aad.extend_from_slice(group.as_bytes());
        aad
    }

    /// Seals a print for the line of `person` in `group`: AES-256-GCM with a
    /// fresh random nonce; person and group are bound into it, so a sealed
    /// print moved to another line no longer opens.
    ///
    /// # Errors
    /// No random numbers, or OpenSSL fails.
    pub fn seal(&self, person: &str, group: &str, print: &Print) -> Result<String, String> {
        let mut nonce = [0u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|_| "no random numbers".to_owned())?;
        let mut plain = Vec::with_capacity(PRINT_BYTES);
        for number in &print.0 {
            plain.extend_from_slice(&number.to_le_bytes());
        }
        let mut tag = [0u8; TAG_BYTES];
        let sealed = encrypt_aead(
            Cipher::aes_256_gcm(),
            &self.seal,
            Some(&nonce),
            &Self::aad(person, group),
            &plain,
            &mut tag,
        )
        .map_err(|e| format!("seal: {e}"))?;
        plain.fill(0);
        let mut out = Vec::new();
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&sealed);
        out.extend_from_slice(&tag);
        Ok(B64.encode(out))
    }

    /// Opens a print sealed for the line of `person` in `group`.
    ///
    /// # Errors
    /// Anything changed, moved to another line or sealed with another
    /// secret.
    pub fn open(&self, person: &str, group: &str, sealed: &str) -> Result<Print, String> {
        let bytes = B64
            .decode(sealed)
            .map_err(|_| "sealed print: not base64".to_owned())?;
        let (nonce, rest) = bytes
            .split_at_checked(NONCE_BYTES)
            .ok_or("sealed print: too short")?;
        let cut = rest
            .len()
            .checked_sub(TAG_BYTES)
            .ok_or("sealed print: too short")?;
        let (body, tag) = rest
            .split_at_checked(cut)
            .ok_or("sealed print: too short")?;
        let mut plain = decrypt_aead(
            Cipher::aes_256_gcm(),
            &self.seal,
            Some(nonce),
            &Self::aad(person, group),
            body,
            tag,
        )
        .map_err(|_| "sealed print: changed, moved, or sealed with another secret".to_owned())?;
        let print = print_from(&plain);
        plain.fill(0);
        print
    }
}

fn print_from(bytes: &[u8]) -> Result<Print, String> {
    if bytes.len() != PRINT_BYTES {
        return Err("sealed print: wrong length".into());
    }
    let mut print = [0f32; DIM];
    for (slot, four) in print.iter_mut().zip(bytes.chunks_exact(4)) {
        let four = <[u8; 4]>::try_from(four).map_err(|_| "sealed print".to_owned())?;
        let number = f32::from_le_bytes(four);
        if !number.is_finite() {
            return Err("sealed print: not a number".into());
        }
        *slot = number;
    }
    Ok(Print(print))
}

/// HMAC-SHA256.
fn mac(key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    let key = PKey::hmac(key).map_err(|e| format!("hmac: {e}"))?;
    let mut signer =
        Signer::new(MessageDigest::sha256(), &key).map_err(|e| format!("hmac: {e}"))?;
    signer.update(data).map_err(|e| format!("hmac: {e}"))?;
    signer.sign_to_vec().map_err(|e| format!("hmac: {e}"))
}

/// `[checks.document.dedup]` of the configuration: needed when the rules
/// switch face dedup on (`identity.face_dedup`).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DedupConfig {
    /// The folder of the face store ([`FileFaceStore`]).
    pub store: PathBuf,
    /// The file with the secret of the face store: hexadecimal text, at
    /// least 32 random bytes; not the secret of the person keys.
    pub key_file: PathBuf,
    /// Same person from this likeness (cosine) on. No default on purpose:
    /// measure first (`capita-face <models> measure <folder>`).
    pub same_person: f32,
    /// Where a held person turns to: the Verein board (e-mail or web
    /// address).
    pub contact: String,
}

/// One person of a birthday group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The person key, hexadecimal.
    pub person: String,
    /// The face print of the chip photo, sealed ([`DedupKey::seal`]).
    pub print: String,
    /// When the verification it comes from was made, in seconds since the
    /// Unix epoch.
    pub at: u64,
}

/// A print of the same group that looks like the new one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    /// Its person key, hexadecimal.
    pub person: String,
    /// How alike the two prints are (cosine; 1 = the same direction).
    pub likeness: f32,
}

/// A verification held for the board.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// The number of the case: 16 hexadecimal digits, random. For the
    /// board only; the person never sees it.
    pub id: String,
    /// The person key of the verification held, hexadecimal.
    pub person: String,
    /// The prints it matched.
    pub matches: Vec<Match>,
    /// When it was opened, in seconds since the Unix epoch.
    pub opened: u64,
}

/// What the board decided about two person keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// One person: the later verification is refused, the account that
    /// exists stays (rule 21).
    Same,
    /// Two people (twins, a lookalike, an error of the model): never held
    /// for each other again.
    Different,
}

impl Verdict {
    /// The word of the board's command.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "same" => Some(Self::Same),
            "different" => Some(Self::Different),
            _ => None,
        }
    }

    /// The word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::Different => "different",
        }
    }
}

/// A verdict on a pair of person keys; the smaller key comes first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pair {
    /// The smaller person key.
    pub a: String,
    /// The larger person key.
    pub b: String,
    /// What the board decided.
    pub verdict: Verdict,
    /// When, in seconds since the Unix epoch.
    pub at: u64,
}

fn ordered<'a>(x: &'a str, y: &'a str) -> (&'a str, &'a str) {
    if x <= y {
        (x, y)
    } else {
        (y, x)
    }
}

/// Where the prints, the open cases and the board's verdicts are kept. A
/// database, or a private matching spread over several services (prints no
/// single service can read), fits behind the same trait.
pub trait FaceStore: Send {
    /// The people of a birthday group.
    ///
    /// # Errors
    /// The store failed.
    fn group(&self, group: &str) -> Result<Vec<Entry>, String>;

    /// Puts the line of `entry.person` into `group`: replaces that person's
    /// earlier line, and the line of `replaces` – the key the same person
    /// had before a change of name or of secret –, whose verdicts and open
    /// case pass to the new key.
    ///
    /// # Errors
    /// The store failed.
    fn put(&mut self, group: &str, entry: Entry, replaces: Option<&str>) -> Result<(), String>;

    /// Forgets a person key: its print, its open case, the verdicts that
    /// name it. For an account that is closed (rules 20, 21). True if a
    /// print was kept.
    ///
    /// # Errors
    /// The store failed.
    fn forget(&mut self, person: &str) -> Result<bool, String>;

    /// The open cases, oldest first.
    ///
    /// # Errors
    /// The store failed.
    fn cases(&self) -> Result<Vec<Case>, String>;

    /// Opens a case. If the same person has an open case already, that one
    /// is kept and returned.
    ///
    /// # Errors
    /// The store failed.
    fn open_case(&mut self, case: Case) -> Result<Case, String>;

    /// Closes a case with the board's verdict on every print it matched.
    /// `None` if there is no open case of that number.
    ///
    /// # Errors
    /// The store failed.
    fn decide(&mut self, id: &str, verdict: Verdict, at: u64) -> Result<Option<Case>, String>;

    /// What the board decided about two person keys, if anything.
    ///
    /// # Errors
    /// The store failed.
    fn verdict(&self, x: &str, y: &str) -> Result<Option<Verdict>, String>;
}

/// Shared by both stores: the changes, on plain lists.
mod lists {
    use super::{ordered, Case, Entry, Pair, Verdict};

    pub fn put(entries: &mut Vec<Entry>, entry: Entry, replaces: Option<&str>) {
        entries.retain(|e| e.person != entry.person && Some(e.person.as_str()) != replaces);
        entries.push(entry);
    }

    pub fn rename(cases: &mut [Case], pairs: &mut Vec<Pair>, old: &str, new: &str) {
        for case in cases.iter_mut() {
            if case.person == old {
                new.clone_into(&mut case.person);
            }
            for found in &mut case.matches {
                if found.person == old {
                    new.clone_into(&mut found.person);
                }
            }
        }
        let mut renamed = Vec::with_capacity(pairs.len());
        for pair in pairs.drain(..) {
            let a = if pair.a == old { new } else { pair.a.as_str() };
            let b = if pair.b == old { new } else { pair.b.as_str() };
            let (a, b) = ordered(a, b);
            renamed.push(Pair {
                a: a.to_owned(),
                b: b.to_owned(),
                verdict: pair.verdict,
                at: pair.at,
            });
        }
        *pairs = renamed;
    }

    pub fn forget(cases: &mut Vec<Case>, pairs: &mut Vec<Pair>, person: &str) {
        cases.retain(|c| c.person != person);
        for case in cases.iter_mut() {
            case.matches.retain(|m| m.person != person);
        }
        cases.retain(|c| !c.matches.is_empty());
        pairs.retain(|p| p.a != person && p.b != person);
    }

    pub fn open(cases: &mut Vec<Case>, case: Case) -> Case {
        if let Some(open) = cases.iter().find(|c| c.person == case.person) {
            return open.clone();
        }
        cases.push(case.clone());
        case
    }

    pub fn decide(
        cases: &mut Vec<Case>,
        pairs: &mut Vec<Pair>,
        id: &str,
        verdict: Verdict,
        at: u64,
    ) -> Option<Case> {
        let place = cases.iter().position(|c| c.id == id)?;
        let case = cases.remove(place);
        for found in &case.matches {
            let (a, b) = ordered(&case.person, &found.person);
            pairs.retain(|p| !(p.a == a && p.b == b));
            pairs.push(Pair {
                a: a.to_owned(),
                b: b.to_owned(),
                verdict,
                at,
            });
        }
        Some(case)
    }

    pub fn verdict(pairs: &[Pair], x: &str, y: &str) -> Option<Verdict> {
        let (a, b) = ordered(x, y);
        pairs
            .iter()
            .rev()
            .find(|p| p.a == a && p.b == b)
            .map(|p| p.verdict)
    }
}

/// A face store in memory: tests.
#[derive(Clone, Debug, Default)]
pub struct MemoryFaceStore {
    groups: BTreeMap<String, Vec<Entry>>,
    cases: Vec<Case>,
    pairs: Vec<Pair>,
}

impl FaceStore for MemoryFaceStore {
    fn group(&self, group: &str) -> Result<Vec<Entry>, String> {
        Ok(self.groups.get(group).cloned().unwrap_or_default())
    }

    fn put(&mut self, group: &str, entry: Entry, replaces: Option<&str>) -> Result<(), String> {
        if let Some(old) = replaces {
            lists::rename(&mut self.cases, &mut self.pairs, old, &entry.person);
        }
        lists::put(
            self.groups.entry(group.to_owned()).or_default(),
            entry,
            replaces,
        );
        Ok(())
    }

    fn forget(&mut self, person: &str) -> Result<bool, String> {
        let mut found = false;
        for entries in self.groups.values_mut() {
            let before = entries.len();
            entries.retain(|e| e.person != person);
            found |= entries.len() != before;
        }
        self.groups.retain(|_, entries| !entries.is_empty());
        lists::forget(&mut self.cases, &mut self.pairs, person);
        Ok(found)
    }

    fn cases(&self) -> Result<Vec<Case>, String> {
        Ok(self.cases.clone())
    }

    fn open_case(&mut self, case: Case) -> Result<Case, String> {
        Ok(lists::open(&mut self.cases, case))
    }

    fn decide(&mut self, id: &str, verdict: Verdict, at: u64) -> Result<Option<Case>, String> {
        Ok(lists::decide(
            &mut self.cases,
            &mut self.pairs,
            id,
            verdict,
            at,
        ))
    }

    fn verdict(&self, x: &str, y: &str) -> Result<Option<Verdict>, String> {
        Ok(lists::verdict(&self.pairs, x, y))
    }
}

/// A face store in a folder:
///
/// ```text
/// groups/<group>.jsonl   the people of one birthday group, one line each
/// cases.jsonl            the open cases
/// pairs.jsonl            the board's verdicts
/// lock                   taken by every reader and writer
/// ```
///
/// Nothing is created before the first print is kept. Folders and files
/// can be read by their owner only.
#[derive(Clone, Debug)]
pub struct FileFaceStore {
    dir: PathBuf,
}

impl FileFaceStore {
    /// The store in `dir`.
    #[must_use]
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn groups(&self) -> PathBuf {
        self.dir.join("groups")
    }

    fn group_file(&self, group: &str) -> Result<PathBuf, String> {
        let valid = group.len() == GROUP_BYTES.saturating_mul(2)
            && group
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !valid {
            return Err("face store: not a group name".into());
        }
        Ok(self.groups().join(format!("{group}.jsonl")))
    }

    /// Takes the lock: shared for reading, exclusive for a change. `None`
    /// when reading a store that holds nothing yet.
    fn lock(&self, change: bool) -> Result<Option<File>, String> {
        let path = self.dir.join("lock");
        let problem = |e: std::io::Error| format!("{}: {e}", path.display());
        if change {
            private_dir(&self.groups())?;
        } else if !path.exists() {
            return Ok(None);
        }
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(problem)?;
        if change {
            file.lock().map_err(problem)?;
        } else {
            file.lock_shared().map_err(problem)?;
        }
        Ok(Some(file))
    }
}

fn private_dir(path: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The lines of a file, one JSON object each; none if there is no file.
fn read_lines<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(number, line)| {
            serde_json::from_str(line)
                .map_err(|e| format!("{}, line {}: {e}", path.display(), number.saturating_add(1)))
        })
        .collect()
}

/// Writes the lines anew: into a new file, synced, then put in place of the
/// old one. No lines: the file is removed.
fn write_lines<T: Serialize>(path: &Path, items: &[T]) -> Result<(), String> {
    let problem = |e: std::io::Error| format!("{}: {e}", path.display());
    if items.is_empty() {
        return match fs::remove_file(path) {
            Ok(()) => sync_dir(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(problem(e)),
        };
    }
    let mut text = String::new();
    for item in items {
        text.push_str(&serde_json::to_string(item).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    let fresh = path.with_extension("jsonl.new");
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&fresh).map_err(problem)?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(problem)?;
    drop(file);
    fs::rename(&fresh, path).map_err(problem)?;
    sync_dir(path)
}

/// Makes a rename or removal in the folder of `path` durable.
fn sync_dir(path: &Path) -> Result<(), String> {
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    #[cfg(unix)]
    {
        File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

impl FaceStore for FileFaceStore {
    fn group(&self, group: &str) -> Result<Vec<Entry>, String> {
        let path = self.group_file(group)?;
        let Some(_lock) = self.lock(false)? else {
            return Ok(Vec::new());
        };
        read_lines(&path)
    }

    fn put(&mut self, group: &str, entry: Entry, replaces: Option<&str>) -> Result<(), String> {
        let path = self.group_file(group)?;
        let _lock = self.lock(true)?;
        if let Some(old) = replaces {
            let (cases_file, pairs_file) =
                (self.dir.join("cases.jsonl"), self.dir.join("pairs.jsonl"));
            let mut cases: Vec<Case> = read_lines(&cases_file)?;
            let mut pairs: Vec<Pair> = read_lines(&pairs_file)?;
            lists::rename(&mut cases, &mut pairs, old, &entry.person);
            write_lines(&cases_file, &cases)?;
            write_lines(&pairs_file, &pairs)?;
        }
        let mut entries: Vec<Entry> = read_lines(&path)?;
        lists::put(&mut entries, entry, replaces);
        write_lines(&path, &entries)
    }

    fn forget(&mut self, person: &str) -> Result<bool, String> {
        let _lock = self.lock(true)?;
        let mut found = false;
        let groups = self.groups();
        let listing = fs::read_dir(&groups).map_err(|e| format!("{}: {e}", groups.display()))?;
        for item in listing {
            let path = item
                .map_err(|e| format!("{}: {e}", groups.display()))?
                .path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let mut entries: Vec<Entry> = read_lines(&path)?;
            let before = entries.len();
            entries.retain(|e| e.person != person);
            if entries.len() != before {
                found = true;
                write_lines(&path, &entries)?;
            }
        }
        let (cases_file, pairs_file) = (self.dir.join("cases.jsonl"), self.dir.join("pairs.jsonl"));
        let mut cases: Vec<Case> = read_lines(&cases_file)?;
        let mut pairs: Vec<Pair> = read_lines(&pairs_file)?;
        lists::forget(&mut cases, &mut pairs, person);
        write_lines(&cases_file, &cases)?;
        write_lines(&pairs_file, &pairs)?;
        Ok(found)
    }

    fn cases(&self) -> Result<Vec<Case>, String> {
        let Some(_lock) = self.lock(false)? else {
            return Ok(Vec::new());
        };
        read_lines(&self.dir.join("cases.jsonl"))
    }

    fn open_case(&mut self, case: Case) -> Result<Case, String> {
        let _lock = self.lock(true)?;
        let path = self.dir.join("cases.jsonl");
        let mut cases: Vec<Case> = read_lines(&path)?;
        let case = lists::open(&mut cases, case);
        write_lines(&path, &cases)?;
        Ok(case)
    }

    fn decide(&mut self, id: &str, verdict: Verdict, at: u64) -> Result<Option<Case>, String> {
        let _lock = self.lock(true)?;
        let (cases_file, pairs_file) = (self.dir.join("cases.jsonl"), self.dir.join("pairs.jsonl"));
        let mut cases: Vec<Case> = read_lines(&cases_file)?;
        let mut pairs: Vec<Pair> = read_lines(&pairs_file)?;
        let decided = lists::decide(&mut cases, &mut pairs, id, verdict, at);
        if decided.is_some() {
            // The verdict first: a power cut in between leaves the case
            // open with its verdict known, and deciding again is harmless.
            write_lines(&pairs_file, &pairs)?;
            write_lines(&cases_file, &cases)?;
        }
        Ok(decided)
    }

    fn verdict(&self, x: &str, y: &str) -> Result<Option<Verdict>, String> {
        let Some(_lock) = self.lock(false)? else {
            return Ok(None);
        };
        let pairs: Vec<Pair> = read_lines(&self.dir.join("pairs.jsonl"))?;
        Ok(lists::verdict(&pairs, x, y))
    }
}

/// What the comparison found.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Nobody else of the group looks alike: the verification goes on.
    Clear,
    /// The board found earlier that this person already holds an account.
    Double,
    /// Held for the board.
    Held(Case),
}

/// Face dedup as the identity service runs it: the keys, the store, the
/// likeness from which two prints count as one person, and where a held
/// person turns to.
pub struct Dedup {
    key: DedupKey,
    store: Box<dyn FaceStore>,
    same_person: f32,
    contact: String,
}

impl Dedup {
    /// `same_person`: two prints are one person from this likeness (cosine)
    /// on – stricter than the one-to-one value of the face check, and
    /// measured before real people (`capita-face <models> measure`).
    /// `contact`: where a held person turns to (the Verein board).
    ///
    /// # Errors
    /// A likeness outside 0 to 1, or no contact.
    pub fn new(
        key: DedupKey,
        store: Box<dyn FaceStore>,
        same_person: f32,
        contact: String,
    ) -> Result<Self, String> {
        if same_person.is_nan() || same_person <= 0.0 || same_person > 1.0 {
            return Err("face dedup: same_person must lie above 0 and at most 1".into());
        }
        if contact.trim().is_empty() {
            return Err("face dedup: a contact for held people is needed".into());
        }
        Ok(Self {
            key,
            store,
            same_person,
            contact,
        })
    }

    /// Where a held person turns to.
    #[must_use]
    pub fn contact(&self) -> &str {
        &self.contact
    }

    /// The store, for the board's commands.
    #[must_use]
    pub fn store(&self) -> &dyn FaceStore {
        self.store.as_ref()
    }

    /// The store, for the board's commands.
    pub fn store_mut(&mut self) -> &mut dyn FaceStore {
        self.store.as_mut()
    }

    /// Compares the print of `person` with everybody of the birthday group
    /// of `born`, except `person` itself and `earlier` (keys the same
    /// person had before). A match the board has not decided on opens a
    /// case; while a case of `person` is open, the person stays held.
    ///
    /// # Errors
    /// The store failed, or a sealed print does not open: then nobody
    /// passes (the check fails closed).
    pub fn check(
        &mut self,
        person: &str,
        born: Date,
        print: &Print,
        earlier: &[String],
        now: u64,
    ) -> Result<Outcome, String> {
        if let Some(case) = self.store.cases()?.into_iter().find(|c| c.person == person) {
            return Ok(Outcome::Held(case));
        }
        let group = self.key.group(born)?;
        let mut matches = Vec::new();
        for entry in self.store.group(&group)? {
            if entry.person == person || earlier.contains(&entry.person) {
                continue;
            }
            let theirs = self.key.open(&entry.person, &group, &entry.print)?;
            let likeness = print.cosine(&theirs);
            if likeness < self.same_person {
                continue;
            }
            match self.store.verdict(person, &entry.person)? {
                Some(Verdict::Different) => {}
                Some(Verdict::Same) => return Ok(Outcome::Double),
                None => matches.push(Match {
                    person: entry.person,
                    likeness,
                }),
            }
        }
        if matches.is_empty() {
            return Ok(Outcome::Clear);
        }
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).map_err(|_| "no random numbers".to_owned())?;
        let case = Case {
            id: hex(&id),
            person: person.to_owned(),
            matches,
            opened: now,
        };
        Ok(Outcome::Held(self.store.open_case(case)?))
    }

    /// Keeps the print of a verification that passed, in place of the
    /// person's earlier print (yearly verification, rule 9) and of the
    /// print of `replaces`.
    ///
    /// # Errors
    /// The store failed.
    pub fn keep(
        &mut self,
        person: &str,
        born: Date,
        print: &Print,
        replaces: Option<&str>,
        now: u64,
    ) -> Result<(), String> {
        let group = self.key.group(born)?;
        let sealed = self.key.seal(person, &group, print)?;
        self.store.put(
            &group,
            Entry {
                person: person.to_owned(),
                print: sealed,
                at: now,
            },
            replaces,
        )
    }
}
