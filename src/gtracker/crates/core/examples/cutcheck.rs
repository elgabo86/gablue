//! Diagnostic : découpe produite pour une piste (fenêtre de boucle, grille,
//! frontières), sans passer par l'interface.
//!
//! `cargo run --release -p gtracker-core --example cutcheck -- <fichier> <subsong> <durée .m3u> <beats>`

use std::path::PathBuf;

use gtracker_core as core;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let file = PathBuf::from(&a[1]);
    let subsong: usize = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(1);
    let length: Option<f64> = a.get(3).and_then(|s| s.parse().ok());
    let beats: f64 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(8.0);
    let track = core::Track {
        subsong: subsong as u32,
        title: "test".into(),
        file,
        length,
        loop_start: None,
        loop_end: None,
    };
    let d = core::decode::decode(&core::zxtune_path(), &track)?;
    let tempo = core::decode::analyse_tempo(&d.pcm, d.channels, d.rate, d.total, d.window);
    println!(
        "total {:.3} · fenêtre {:?} · {:.2} BPM · beat {:.4} · phase {:.3}",
        d.total, d.window, tempo.bpm, tempo.beat, tempo.phase
    );
    let whole = d.window.map(|(_, b)| b).unwrap_or(d.total);
    let mut t = tempo;
    if let Some((a, b)) = d.window {
        t.phase = a; // ancre sur le début de boucle (ce que fait l'UI)
        if b - a > 4.0 {
            t = t.across(b - a);
        }
    }
    let clips = core::tempo::cut_on(whole, &t, beats, d.envelope.as_ref());
    println!(
        "{} tranches · grille ancrée phase {:.3} · pas {:.3} s",
        clips.len(),
        t.phase,
        t.beat * beats
    );
    for (i, c) in clips.iter().take(4).enumerate() {
        println!("  {i}: {:.3} → {:.3}  ({:.3} s)", c.start, c.end, c.end - c.start);
    }
    println!("  …");
    for (i, c) in clips.iter().enumerate().skip(clips.len().saturating_sub(3)) {
        println!("  {i}: {:.3} → {:.3}  ({:.3} s)", c.start, c.end, c.end - c.start);
    }
    Ok(())
}
