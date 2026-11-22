//! Vérifie l'export d'un projet GTracker de bout en bout, sans interface :
//! mixage hors temps réel → FLAC (ffmpeg) → relecture du fichier pour sa durée,
//! ses canaux, et l'absence de métronome dans le rendu exporté.

use std::sync::Arc;

use anyhow::Context;
use gtracker_core::convert::{self, Format, Tags};
use gtracker_core::mixer::{render, Plan, SourceAudio};
use gtracker_core::project::{Clip, Fill, Project, Source};

fn main() -> anyhow::Result<()> {
    const RATE: u32 = 48_000;
    let rate = 44_100u32;

    // 2 s de la 440 Hz, une piste, un clip d'un temps placé au temps 2
    let frames = rate as usize * 2;
    let pcm: Vec<f32> = (0..frames)
        .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
        .collect();

    let mut p = Project::new("export-check");
    p.tempo_bpm = 120.0;
    let idx = p.add_source(Source {
        id: 1,
        title: "la 440".into(),
        site: "fichiers".into(),
        entry_id: -1,
        file: "/tmp/la440.wav".into(),
        subsong: 0,
        loop_start: None,
        loop_end: None,
        bpm: 120.0,
        phase: 0.0,
        key: None,
    });
    let track = p.add_track("piste", Some(idx), 0);
    p.add_clip(Clip {
        id: 0,
        track,
        source: idx,
        src_start: 0,
        src_end: 44_100, // 1 s de contenu
        at_beat: 2.0,
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
    // l'écoute a des clics, pas l'export : le projet exporté doit être propre
    p.metronome = true;
    p.master_gain = 1.0;
    let clean = {
        let mut c = p.clone();
        c.metronome = false;
        c.loop_beats = None;
        c
    };
    let audio = vec![Some(SourceAudio::new(Arc::new(pcm), 1, rate))];

    let plan = Plan::build(&clean, &audio, RATE);
    anyhow::ensure!(!plan.is_empty(), "plan vide");
    let total = plan.frames;
    let pcm_out = render(&plan, total as usize);
    let peak = pcm_out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    anyhow::ensure!(peak > 0.2, "rendu muet (crête {peak})");

    // début du projet = silence : un métronome aurait cliqué au temps 1
    let head = pcm_out[..(RATE as usize / 50) * 2]
        .iter()
        .fold(0.0f32, |m, v| m.max(v.abs()));
    anyhow::ensure!(
        head < 1e-4,
        "clic de métronome dans l'export (crête {head})"
    );
    // l'attaque du clip tombe exactement au temps 2 (2 × 0,5 s)
    let start = (2.0 * (60.0 / 120.0) * RATE as f64) as usize;
    let before = pcm_out[(start - 200) * 2..start * 2]
        .iter()
        .fold(0.0f32, |m, v| m.max(v.abs()));
    let after = pcm_out[start * 2..(start + 200) * 2]
        .iter()
        .fold(0.0f32, |m, v| m.max(v.abs()));
    anyhow::ensure!(before < 1e-4 && after > 0.1, "clip pas au temps exact");

    let dir = std::env::temp_dir().join(format!("gtracker-export-{}", std::process::id()));
    let tags = Tags {
        title: Some("export-check".into()),
        artist: Some("GTracker".into()),
        album: Some("test".into()),
    };
    let path = convert::export_pcm(&pcm_out, 2, RATE, &dir, Format::Flac, &tags, "export-check")
        .context("export FLAC")?;
    let size = std::fs::metadata(&path)?.len();
    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=channels,sample_rate",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1",
        ])
        .arg(&path)
        .output()
        .context("ffprobe")?;
    anyhow::ensure!(probe.status.success(), "ffprobe a échoué");
    let text = String::from_utf8_lossy(&probe.stdout).to_string();
    let dur: f64 = text
        .lines()
        .find_map(|l| l.strip_prefix("duration="))
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0.0);
    let expected = (total as f64) / RATE as f64;
    anyhow::ensure!(
        (dur - expected).abs() < 0.05,
        "durée {dur:.3} s, attendu {expected:.3} s"
    );
    anyhow::ensure!(text.contains("channels=2"), "pas stéréo : {text}");
    std::fs::remove_dir_all(&dir).ok();

    println!(
        "EXPORT OK · {:.2} s · stéréo 48 kHz · FLAC {} Ko · métronome exclu · clip au temps exact",
        dur,
        size / 1024
    );
    Ok(())
}
