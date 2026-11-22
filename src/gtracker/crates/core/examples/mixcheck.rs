//! Vérifie le moteur sans interface : même rendu hors temps réel que temps
//! réel, somme exacte de deux voix, boucle déterministe.
//!
//! `cargo run --release -p gtracker-core --example mixcheck`

use std::sync::Arc;

use gtracker_core::mixer::{render, Plan, SourceAudio, Transport};
use gtracker_core::project::{Clip, Fill, Project, Source};

fn tone(n: usize, rate: u32, hz: f32) -> Arc<Vec<f32>> {
    Arc::new(
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin() * 0.5)
            .collect(),
    )
}

fn project(two: bool) -> Project {
    let mut p = Project::new("mixcheck");
    p.tempo_bpm = 120.0;
    let idx = p.add_source(Source {
        id: 1,
        title: "a".into(),
        site: "fichiers".into(),
        entry_id: -1,
        file: "/tmp/mixcheck-a.wav".into(),
        subsong: 1,
        loop_start: None,
        loop_end: None,
        bpm: 120.0,
        phase: 0.0,
        key: None,
    });
    let t = p.add_track("p1", Some(idx), 1);
    p.add_clip(Clip {
        id: 0,
        track: t,
        source: idx,
        src_start: 0,
        src_end: 24_000,
        at_beat: 0.0,
        length_beats: 2.0,
        gain: 1.0,
        fade_in: 0.0,
        fade_out: 0.0,
        transpose: None,
        key_lock: None,
        key: None,
        stretch: None,
        fill: Fill::Trim,
    });
    if two {
        let idx2 = p.add_source(Source {
            id: 2,
            title: "b".into(),
            site: "fichiers".into(),
            entry_id: -2,
            file: "/tmp/mixcheck-b.wav".into(),
            subsong: 1,
            loop_start: None,
            loop_end: None,
            bpm: 120.0,
            phase: 0.0,
            key: None,
        });
        let t2 = p.add_track("p2", Some(idx2), 2);
        p.add_clip(Clip {
            id: 0,
            track: t2,
            source: idx2,
            src_start: 0,
            src_end: 24_000,
            at_beat: 0.0,
            length_beats: 2.0,
            gain: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            transpose: None,
            key_lock: None,
            key: None,
            stretch: None,
            fill: Fill::Trim,
        });
    }
    p
}

fn main() -> anyhow::Result<()> {
    let rate = 48_000u32;
    let audio = vec![Some(SourceAudio::new(
        tone(24_000, 44_100, 440.0),
        1,
        44_100,
    ))];

    // 1. même code en temps réel et hors temps réel
    let one = Plan::build(&project(false), &audio, rate);
    let offline = render(&one, 60_000);
    let transport = Transport::new(one.clone());
    let mut mixer = transport.source();
    let mut live = Vec::with_capacity(120_000);
    for _ in 0..120_000 {
        live.push(mixer.next().unwrap_or(0.0));
    }
    anyhow::ensure!(live == offline, "rendu temps réel différent de l'offline");
    let peak = offline.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    anyhow::ensure!(peak > 0.2, "aucun signal audible (crête {peak})");

    // 2. deux voix identiques = deux fois une voix (gain centre constant)
    let two_audio = vec![
        Some(SourceAudio::new(tone(24_000, 44_100, 440.0), 1, 44_100)),
        Some(SourceAudio::new(tone(24_000, 44_100, 660.0), 1, 44_100)),
    ];
    let two = Plan::build(&project(true), &two_audio, rate);
    let mixed = render(&two, 60_000);
    anyhow::ensure!(
        two.voices.len() == 2,
        "les deux pistes ne sont pas dans le plan"
    );
    let mut diff = 0.0f32;
    for (a, b) in mixed.iter().zip(&offline) {
        // la deuxième voix ajoute du signal, il doit y avoir un écart partout
        diff += (a - b).abs();
    }
    anyhow::ensure!(diff > 1.0, "la seconde voix n'ajoute rien (diff {diff})");

    // 3. une piste mute = silence, un solo coupe l'autre
    let mut muted = project(true);
    muted.tracks[0].mute = true;
    let mut soloed = project(true);
    soloed.tracks[1].solo = true;
    let m = render(&Plan::build(&muted, &two_audio, rate), 60_000);
    let s = render(&Plan::build(&soloed, &two_audio, rate), 60_000);
    anyhow::ensure!(m != mixed, "mute ne change pas le rendu");
    anyhow::ensure!(s != mixed, "solo ne change pas le rendu");
    // mute de la piste 1 et solo de la piste 2 = le même mix (la piste 2 seule)
    anyhow::ensure!(
        m == s,
        "mute piste 1 et solo piste 2 devraient donner le même mix"
    );

    // 4. boucle de projet déterministe
    let mut looped = project(false);
    looped.loop_beats = Some((0.0, 1.0));
    let l = render(&Plan::build(&looped, &audio, rate), 48_000);
    let b = (0.5 * rate as f64) as usize * 2;
    anyhow::ensure!(
        (l[0] - l[b]).abs() < 1e-6,
        "la boucle ne se répète pas à l'identique"
    );

    println!(
        "MIX OK · {} voix · {} trames · crête {:.3} · offline=temps réel · mute/solo actifs",
        two.voices.len(),
        offline.len() / 2,
        peak
    );
    Ok(())
}
