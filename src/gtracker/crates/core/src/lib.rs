//! Cœur de GTracker : base joshw.info, archives, décodage zxtune, lecture rodio.

pub mod archive;
pub mod beats;
pub mod convert;
pub mod db;
pub mod decode;
pub mod fft;
pub mod genh;
pub mod joshw;
pub mod key;
pub mod loopdetect;
pub mod m3u;
pub mod mixer;
pub mod model;
pub mod peaks;
pub mod player;
pub mod project;
pub mod psf;
pub mod spc;
pub mod stretch;
pub mod tempo;
pub mod util;
pub mod vgmstream;

pub use model::{Entry, LoopWindow, Track};

use std::path::PathBuf;

/// Emplacement de `zxtune123` : variable `ZXTUNE`, sinon PATH, sinon `bin/zxtune123`.
pub fn zxtune_path() -> PathBuf {
    if let Ok(p) = std::env::var("ZXTUNE") {
        return PathBuf::from(p);
    }
    let cached = util::cache_root().join("bin").join("zxtune123");
    if cached.exists() {
        return cached;
    }
    if let Ok(exe) = std::env::current_exe() {
        for up in [1usize, 2, 3] {
            let mut p = exe.clone();
            for _ in 0..up {
                p.pop();
            }
            let cand = p.join("bin/zxtune123");
            if cand.exists() {
                return cand;
            }
        }
    }
    PathBuf::from("bin/zxtune123")
}
