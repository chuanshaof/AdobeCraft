//! The library's folder tree: where on disk the imported photos live.
//!
//! A *folder* here is a folder holding at least one photo of the library (imported, not deleted,
//! not merely browsed in Local; demo scenes have no folder). The tree lists those folders and the
//! ones above them, never a folder that holds nothing of the library, with how many photos each
//! holds — its subfolders included, like a year in By Date counts its months.
//!
//! Every folder is a row of its own, the ones above the imported photos included
//! (`/Users` → `me` → `Pictures`), so a row is exactly the folder it names. A folder spelled two
//! ways (`/` or `\`, repeated or trailing separators, `.`) is one folder (see
//! [`crate::query::folder_key`]).
//!
//! Each disk is a *volume*, a top-level row of its own: a drive letter (`D:`), a Windows share
//! (`//server/share`), a mounted disk or share (macOS `/Volumes/tokyo`, Linux `/media/me/usb`,
//! `/mnt/disk`, `/run/user/1000/gvfs/smb-share:…`, `/net/host`) or, for everything else, the
//! startup disk (`/`). Only the spelling of the stored paths tells them apart: no disk is
//! touched, so a share that is asleep or gone lists and counts like one that is there, and the
//! sidebar never waits on the network.
//!
//! Limits of reading only the stored spelling of a path: file names are compared as written
//! (Windows folds case, other systems do not), so on a case-insensitive disk `/Volumes/tokyo` and
//! `/Volumes/TOKYO` are two volumes here; and one place reached by two spellings
//! (`/Volumes/Macintosh HD/Users/me` and `/Users/me`, `/media/me/usb` and `/run/media/me/usb`) is
//! two rows. Each row is still consistent: choosing it shows exactly the photos it counts.
//!
//! This is the library's own view of its photos; browsing any folder on disk without importing
//! is Local's job (see [`crate::local`]).

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::query::{folder_key, folder_within, split_path};
use crate::{Catalog, Source};

/// Folder levels kept per photo. No real path comes near it; a deeper one is counted at its
/// ancestor this many levels down, which bounds the tree's depth (and so every walk of it)
/// whatever a catalog holds.
pub const MAX_DEPTH: usize = 64;

/// One row of the library's folder tree.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FolderNode {
    /// What the row says: the folder's name; for a volume, its name (`nas`, `D:`,
    /// `//server/share`, `/` for the startup disk).
    pub name: String,
    /// The folder the row stands for, written out from the names of the photos' paths (forward
    /// slashes, no verbatim prefix); hand it back to [`crate::Filter::library_folder`] to see its
    /// photos.
    pub path: String,
    /// Photos in this folder or any folder inside it.
    pub count: usize,
    /// Photos directly in this folder.
    pub own: usize,
    /// A disk rather than a folder in one; its `path` is where it is mounted.
    pub volume: bool,
    /// Whether choosing the row (`Filter::library_folder`) shows exactly its `count` photos. Not
    /// so for the startup disk and for folders that hold other disks (`/Volumes`, `/mnt`…): their
    /// path covers those disks' photos too, so choosing them is not offered.
    pub selectable: bool,
    pub children: Vec<FolderNode>,
}

/// A folder while the tree is being built (children by identity key).
struct Level {
    name: String,
    path: String,
    own: usize,
    volume: bool,
    children: Vec<String>,
}

/// Where a stored folder path lies: its volume and the folder names below it.
struct Placed {
    mount: String,
    volume: String,
    names: Vec<String>,
}

/// A gvfs mount's name made readable: `smb-share:server=tokyo,share=photos` → `photos on tokyo`;
/// any other (`sftp:host=nas`) as it is.
fn mount_name(n: &str) -> String {
    let field = |key: &str| {
        let rest = n.strip_prefix("smb-share:")?;
        rest.split(',').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('=')).filter(|v| !v.is_empty())
    };
    match (field("share"), field("server")) {
        (Some(share), Some(server)) => format!("{share} on {server}"),
        _ => n.to_string(),
    }
}

/// The volume `dir` lies on and the folder names below it, read the way [`folder_key`] reads a
/// path (so a row and the filter that chooses it never disagree). `None` for a path that is not
/// absolute or names no share.
fn place(dir: &str) -> Option<Placed> {
    let sp = split_path(dir, false);
    let names: Vec<&str> = sp.parts.iter().map(String::as_str).collect();
    let named = |mount: String, volume: &str, used: usize| (mount, volume.to_string(), used);
    let (mount, volume, used) = if sp.prefix.starts_with("//") {
        // `//server/share`: a server alone (no share) is no place to keep photos
        if sp.prefix.ends_with('/') {
            return None;
        }
        named(sp.prefix.clone(), &sp.prefix, 0)
    } else if !sp.prefix.is_empty() {
        named(sp.prefix.clone(), &sp.prefix, 0)
    } else if sp.absolute {
        match names.as_slice() {
            ["Volumes", n, ..] => named(format!("/Volumes/{n}"), n, 2),
            ["run", "media", u, n, ..] => named(format!("/run/media/{u}/{n}"), n, 4),
            ["media", u, n, ..] => named(format!("/media/{u}/{n}"), n, 3),
            ["mnt", n, ..] => named(format!("/mnt/{n}"), n, 2),
            // Linux desktops mount network places per user, naming server and share
            ["run", "user", uid, "gvfs", n, ..] => named(format!("/run/user/{uid}/gvfs/{n}"), &mount_name(n), 5),
            // NFS automounts: Linux `/net/<host>`, macOS `/Network/Servers/<host>`
            ["net", host, ..] => named(format!("/net/{host}"), host, 2),
            ["Network", "Servers", host, ..] => named(format!("/Network/Servers/{host}"), host, 3),
            _ => named("/".to_string(), "/", 0),
        }
    } else {
        return None;
    };
    Some(Placed { mount, volume, names: names.iter().skip(used).take(MAX_DEPTH).map(|n| n.to_string()).collect() })
}

/// The disk or share (`/Volumes/nas`, `D:`, `/` for the startup disk) a file-backed photo is on,
/// from its stored path alone.
pub fn volume_of(p: &crate::Photo) -> Option<String> {
    let Source::File { path } = &p.source else { return None };
    let dir = path.rfind(['/', '\\']).and_then(|i| path.get(..i.max(1)))?;
    place(dir).map(|pl| folder_key(&pl.mount))
}

/// Whether `path` is a whole disk or share (`/`, `C:\`, `\\srv\share`, `/Volumes/nas`), not a
/// folder in one.
pub fn is_disk_root(path: &str) -> bool {
    place(path).is_some_and(|p| p.names.is_empty())
}

/// Whether `path` is the startup disk's root (`/`), whose path also covers every other disk.
pub fn is_startup_disk(path: &str) -> bool {
    place(path).is_some_and(|p| p.names.is_empty() && p.mount == "/")
}

/// A folder named by its last two names (`photos/travel`), so two folders called `travel` are
/// told apart; the path itself when it has no names (`/`).
pub fn folder_label(path: &str) -> String {
    let names: Vec<&str> = path.split(['/', '\\']).filter(|n| !n.is_empty()).collect();
    if names.is_empty() { path.to_string() } else { names.iter().skip(names.len().saturating_sub(2)).copied().collect::<Vec<_>>().join("/") }
}

impl Catalog {
    /// The library's volumes with their folders and photo counts: volumes by name
    /// (case-insensitive), each folder with its subfolders the same way.
    pub fn folder_tree(&self) -> Vec<FolderNode> {
        // photos per containing folder, as spelled (sorted, so the spelling that names a folder
        // never depends on hash order)
        let mut dirs: BTreeMap<&str, usize> = BTreeMap::new();
        for p in self.photos().filter(|p| p.in_library()) {
            if let Source::File { path } = &p.source
                && let Some(i) = path.rfind(['/', '\\'])
                // a photo straight in the root has an empty folder part: the root itself
                && let Some(dir) = path.get(..i.max(1))
            {
                *dirs.entry(dir).or_default() += 1;
            }
        }
        let mut levels: HashMap<String, Level> = HashMap::new();
        let mut roots: Vec<String> = Vec::new();
        for (dir, n) in dirs {
            let Some(placed) = place(dir) else { continue };
            let vkey = folder_key(&placed.mount);
            if !levels.contains_key(&vkey) {
                levels.insert(
                    vkey.clone(),
                    Level { name: placed.volume.clone(), path: placed.mount.clone(), own: 0, volume: true, children: Vec::new() },
                );
                roots.push(vkey.clone());
            }
            let mut prefix = if placed.mount == "/" { String::new() } else { placed.mount.clone() };
            let mut parent = vkey;
            for name in &placed.names {
                prefix.push('/');
                prefix.push_str(name);
                let key = folder_key(&prefix);
                if !levels.contains_key(&key) {
                    levels.insert(key.clone(), Level { name: name.clone(), path: prefix.clone(), own: 0, volume: false, children: Vec::new() });
                    if let Some(l) = levels.get_mut(&parent) {
                        l.children.push(key.clone());
                    }
                }
                parent = key;
            }
            if let Some(l) = levels.get_mut(&parent) {
                l.own += n;
            }
        }
        let mut tree: Vec<FolderNode> = roots.iter().filter_map(|k| build(k, &levels)).collect();
        sort(&mut tree);
        // a folder that holds another disk's mount covers that disk's photos too
        let mounts: Vec<String> = tree.iter().filter(|v| v.path != "/").map(|v| v.path.clone()).collect();
        mark_selectable(&mut tree, &mounts);
        tree
    }
}

/// The node for `key` with everything below it (at most [`MAX_DEPTH`] deep).
fn build(key: &str, levels: &HashMap<String, Level>) -> Option<FolderNode> {
    let l = levels.get(key)?;
    let mut children: Vec<FolderNode> = l.children.iter().filter_map(|k| build(k, levels)).collect();
    sort(&mut children);
    let count = l.own + children.iter().map(|c| c.count).sum::<usize>();
    Some(FolderNode { name: l.name.clone(), path: l.path.clone(), count, own: l.own, volume: l.volume, selectable: true, children })
}

fn mark_selectable(nodes: &mut [FolderNode], mounts: &[String]) {
    for n in nodes {
        n.selectable = !(n.volume && n.path == "/") && (n.volume || !mounts.iter().any(|m| folder_within(m, &n.path)));
        mark_selectable(&mut n.children, mounts);
    }
}

fn sort(v: &mut [FolderNode]) {
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
}
