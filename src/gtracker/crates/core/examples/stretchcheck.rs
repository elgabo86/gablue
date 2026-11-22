//! Vérifie le stretch sans interface : durée exacte, hauteur tenue, cache, et
//! transposition de +12 demi-tons dans le mixer (durée inchangée, octave).
//!
//! `cargo run --release -p gtracker-core --example stretchcheck`

use std::sync::Arc;

use gtracker_core::mixer::{render, Plan, SourceAudio};
use gtracker_core::project::{Clip, Fill, Project, Source, StretchMode};
use gtracker_core::stretch::{clear_cache, wsola};

fn sine(rate: u32, secs: f32, hz: f32, amp: f32) -> Vec<f32> {
    (0..(rate as f32 * secs) as usize)
        .map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
        .collect()
}

fn crossings(pcm: &[f32]) -> usize {
    pcm.windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count()
}

fn main() -> anyhow::Result<()> {
    let rate = 44_100u32;
    let src = sine(rate, 0.4, 440.0, 0.5);
    let out = wsola(&src, 1, 0, src.len(), src.len() * 2);
    anyhow::ensure!(
        out.len() == src.len() * 2,
        "durée non exacte : {}",
        out.len()
    );
    let f_in = crossings(&src) as f64 / 0.4 / 2.0;
    let f_out = crossings(&out) as f64 / 0.8 / 2.0;
    anyhow::ensure!(
        (f_in - f_out).abs() / f_in < 0.05,
        "hauteur changée : {f_in} → {f_out}"
    );
    let peak = out.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    anyhow::ensure!(peak > 0.45, "amplitude écrasée : crête {peak}");

    // cache : même requête, même buffer
    let shared = Arc::new(src.clone());
    let a = gtracker_core::stretch::cached(&shared, 1, 0, shared.len(), shared.len() * 2);
    let b = gtracker_core::stretch::cached(&shared, 1, 0, shared.len(), shared.len() * 2);
    anyhow::ensure!(Arc::ptr_eq(&a, &b), "le cache ne réutilise pas le buffer");

    // transposition +12 dans le mixer : durée identique, une octave plus haut
    let pcm = Arc::new(sine(rate, 0.5, 440.0, 0.4));
    let mut p = Project::new("stretch");
    p.tempo_bpm = 120.0;
    let idx = p.add_source(Source {
        id: 1,
        title: "s".into(),
        site: "fichiers".into(),
        entry_id: -1,
        file: "/tmp/stretch.wav".into(),
        subsong: 1,
        loop_start: None,
        loop_end: None,
        bpm: 120.0,
        phase: 0.0,
        key: None,
    });
    let t = p.add_track("p", Some(idx), 0);
    p.add_clip(Clip {
        id: 0,
        track: t,
        source: idx,
        src_start: 0,
        src_end: pcm.len(),
        at_beat: 0.0,
        length_beats: 2.0,
        gain: 1.0,
        fade_in: 0.0,
        fade_out: 0.0,
        transpose: None,
        key_lock: None,
        key: None,
        stretch: Some(StretchMode::Stretch),
        fill: Fill::Trim,
    });
    let audio = vec![Some(SourceAudio::new(pcm.clone(), 1, rate))];
    let plain = render(&Plan::build(&p, &audio, rate), (rate as f64 * 1.0) as usize);
    let mut up = p.clone();
    up.clips[0].transpose = Some(12);
    let shifted = render(
        &Plan::build(&up, &audio, rate),
        (rate as f64 * 1.0) as usize,
    );
    let c_plain = crossings(&plain[..rate as usize * 2]) as f64;
    let c_up = crossings(&shifted[..rate as usize * 2]) as f64;
    anyhow::ensure!(
        (c_up / c_plain - 2.0).abs() < 0.05,
        "transposition +12 : rapport de hauteur {:.3} (attendu 2)",
        c_up / c_plain
    );

    clear_cache();
    println!(
        "STRETCH OK · 0,4 s → 0,8 s · {:.1} Hz tenus · 1 octave = durée inchangée · crête {:.2} · cache partagé",
        f_out, peak
    );
    Ok(())
}
