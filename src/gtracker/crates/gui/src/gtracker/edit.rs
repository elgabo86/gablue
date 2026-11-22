//! Opérations d'édition du GTracker, **pures** : aimantation, déplacement,
//! rognage, tranche → clip, historique.
//!
//! Toute la logique de geste vit ici pour être testée sans interface : une
//! mutation appliquée par un glisser doit être reproductible en une fonction.

use gtracker_core::project::{Clip, Fill, Id, Project};

/// Aimante un temps (en beats) au pas de grille. Un pas nul rend la valeur.
pub fn snap(beat: f64, step: f64) -> f64 {
    if step <= 0.0 || !beat.is_finite() {
        return beat.max(0.0);
    }
    ((beat / step).round() * step).max(0.0)
}

/// Déplace un clip sans changer sa longueur.
pub fn move_clip(clip: &mut Clip, at_beat: f64, step: f64) {
    clip.at_beat = snap(at_beat, step).max(0.0);
}

/// Rogne le bord droit : la longueur musicale suit le contenu (on coupe de
/// l'audio, on ne l'étire pas). Le contenu est conservé au prorata.
pub fn trim_right(clip: &mut Clip, new_len_beats: f64, step: f64) {
    let new_len = snap(new_len_beats, step).max(step.max(0.001));
    let old_len = clip.length_beats.max(1e-9);
    let span = clip.src_end.saturating_sub(clip.src_start) as f64;
    let ratio = (new_len / old_len).clamp(0.001, 1000.0);
    clip.src_end = clip.src_start + (span * ratio).round().max(1.0) as usize;
    clip.length_beats = new_len;
}

/// Rogne le bord gauche : le contenu restant ne glisse pas dans le temps
/// (`at_beat` avance d'autant).
pub fn trim_left(clip: &mut Clip, new_at: f64, step: f64) {
    let new_at = snap(new_at, step).max(0.0);
    let delta = new_at - clip.at_beat;
    let old_len = clip.length_beats.max(1e-9);
    let span = clip.src_end.saturating_sub(clip.src_start) as f64;
    if span < 1.0 {
        // clip dégénéré (projet abîmé) : rien à rogner, ne pas soustraire 1 à 0
        return;
    }
    let cut = (delta / old_len * span).round().max(0.0) as usize;
    let cut = cut.min(span as usize - 1);
    clip.src_start += cut;
    clip.at_beat = new_at;
    clip.length_beats = (old_len - delta).max(step.max(0.001));
}

/// Clip créé par le dépôt d'une tranche sur une piste.
///
/// `length_beats` vient de la durée de la tranche et du tempo maître ; la
/// tranche garde ainsi sa longueur musicale d'origine, même si le tempo du
/// projet diffère de celui de la source.
#[allow(clippy::too_many_arguments)]
pub fn clip_from_slice(
    track: Id,
    source: usize,
    src_start: usize,
    src_end: usize,
    at_beat: f64,
    step: f64,
    length_beats: f64,
) -> Clip {
    Clip {
        id: 0,
        track,
        source,
        src_start,
        src_end,
        at_beat: snap(at_beat, step).max(0.0),
        length_beats: length_beats.max(step.max(0.001)),
        gain: 1.0,
        fade_in: 0.0,
        fade_out: 0.0,
        transpose: None,
        key_lock: None,
        key: None,
        stretch: None,
        fill: Fill::Trim,
    }
}

/// Duplique un clip juste après lui (même contenu).
pub fn duplicate(project: &mut Project, id: Id) -> Option<Id> {
    let mut c = project.clip(id)?.clone();
    let at = c.at_beat + c.length_beats;
    c.id = 0;
    c.at_beat = at;
    Some(project.add_clip(c))
}

/// Coupe un clip à un temps donné : rend l'identifiant de la partie droite, ou
/// `None` si la tête de lecture n'est pas strictement à l'intérieur.
pub fn split_at(project: &mut Project, id: Id, beat: f64) -> Option<Id> {
    let c = project.clip(id)?.clone();
    let offset = beat - c.at_beat;
    if offset <= 0.0 || offset >= c.length_beats {
        return None;
    }
    let span = c.src_end.saturating_sub(c.src_start);
    let cut = (offset / c.length_beats * span as f64).round() as usize;
    if cut == 0 || cut >= span {
        return None;
    }
    // partie gauche
    if let Some(left) = project.clip_mut(id) {
        left.src_end = c.src_start + cut;
        left.length_beats = offset;
    }
    // partie droite
    let mut right = c.clone();
    right.id = 0;
    right.src_start = c.src_start + cut;
    right.at_beat = c.at_beat + offset;
    right.length_beats = c.length_beats - offset;
    right.fade_in = 0.0;
    Some(project.add_clip(right))
}

/// Étire la durée musicale du clip jusqu'à la mesure suivante (`bars` temps
/// par mesure). Action **explicite** : le contenu est réétalé, la hauteur
/// dépend du mode (repitch ou stretch).
pub fn fit_bar(project: &mut Project, id: Id, bars: f64, step: f64) -> bool {
    let bars = bars.max(1.0);
    let Some(c) = project.clip_mut(id) else {
        return false;
    };
    let end = c.at_beat + c.length_beats;
    let target = (end / bars).ceil() * bars;
    let new_len = (target - c.at_beat).max(step.max(0.001));
    if (new_len - c.length_beats).abs() < 1e-9 {
        return false;
    }
    c.length_beats = new_len;
    c.fill = Fill::Trim;
    true
}

/// Répète le contenu du clip jusqu'à la fin de la mesure : c'est `Fill::Loop`,
/// le mixer enroule dans la plage source sans rouvrir un clip de plus.
pub fn loop_to_bar(project: &mut Project, id: Id, bars: f64, step: f64) -> bool {
    let bars = bars.max(1.0);
    let Some(c) = project.clip_mut(id) else {
        return false;
    };
    let end = c.at_beat + c.length_beats;
    let target = (end / bars).ceil() * bars;
    let new_len = (target - c.at_beat).max(step.max(0.001));
    c.fill = Fill::Loop;
    c.length_beats = new_len;
    true
}

/// Un clip dégénéré (rogné à zéro) doit disparaître : un clip de longueur nulle
/// est invisible à l'écran mais occupe le projet.
pub fn drop_empty(project: &mut Project) {
    project
        .clips
        .retain(|c| c.src_end > c.src_start && c.length_beats > 0.0);
}

/// Historique d'annulation par instantanés (le projet est petit).
#[derive(Default)]
pub struct History {
    undo: Vec<(String, Project)>,
    redo: Vec<(String, Project)>,
}

const HIST_MAX: usize = 100;

impl History {
    /// À appeler **avant** chaque mutation. Un glisser = un seul appel (au
    /// début), pas un par image.
    pub fn checkpoint(&mut self, label: &str, project: &Project) {
        self.undo.push((label.to_string(), project.clone()));
        if self.undo.len() > HIST_MAX {
            self.undo.remove(0);
        }
        // une nouvelle action invalide la branche refaite
        self.redo.clear();
    }

    pub fn undo(&mut self, project: &mut Project) -> Option<String> {
        let (label, snap) = self.undo.pop()?;
        self.redo.push((label.clone(), project.clone()));
        *project = snap;
        Some(label)
    }

    pub fn redo(&mut self, project: &mut Project) -> Option<String> {
        let (label, snap) = self.redo.pop()?;
        self.undo.push((label.clone(), project.clone()));
        *project = snap;
        Some(label)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtracker_core::project::Source;

    fn project() -> Project {
        let mut p = Project::new("ed");
        p.add_source(Source {
            id: 1,
            title: "a".into(),
            site: "fichiers".into(),
            entry_id: -1,
            file: "/tmp/a.wav".into(),
            subsong: 1,
            loop_start: None,
            loop_end: None,
            bpm: 120.0,
            phase: 0.0,
            key: None,
        });
        p.add_track("p", Some(0), 0);
        p
    }

    #[test]
    fn aimant_et_deplacement() {
        assert_eq!(snap(1.24, 0.25), 1.25);
        assert_eq!(snap(1.1, 0.25), 1.0);
        assert_eq!(snap(3.0, 0.0), 3.0);
        assert_eq!(snap(-4.0, 0.25), 0.0, "jamais avant zéro");
        let mut c = clip_from_slice(1, 0, 0, 1000, 1.0, 0.25, 4.0);
        move_clip(&mut c, 7.4, 0.25);
        assert_eq!(c.at_beat, 7.5);
        assert_eq!(c.length_beats, 4.0);
    }

    #[test]
    fn rogner_le_contenu_suit_la_longueur() {
        let mut c = clip_from_slice(1, 0, 0, 1000, 0.0, 0.25, 4.0);
        trim_right(&mut c, 2.0, 0.25);
        assert_eq!(c.length_beats, 2.0);
        assert_eq!(c.src_end, 500, "moitié du contenu");
        // bord gauche : le contenu restant ne glisse pas
        trim_left(&mut c, 1.0, 0.25);
        assert_eq!(c.at_beat, 1.0);
        assert_eq!(c.length_beats, 1.0);
        assert_eq!(c.src_start, 250);
        // jamais de clip à zéro
        trim_right(&mut c, 0.0, 0.25);
        assert!(c.length_beats >= 0.25 && c.src_end > c.src_start);
        // clip dégénéré : aucun panique, aucun mouvement
        let mut bad = clip_from_slice(1, 0, 100, 100, 0.0, 0.25, 1.0);
        trim_left(&mut bad, 0.5, 0.25);
        assert_eq!(bad.src_start, 100);
        assert_eq!(bad.at_beat, 0.0);
    }

    #[test]
    fn depot_de_tranche_porte_sa_longueur() {
        // 2 s à 120 BPM = 4 temps
        let c = clip_from_slice(1, 2, 0, 88_200, 3.1, 0.25, 4.0);
        assert_eq!(c.at_beat, 3.0);
        assert_eq!(c.length_beats, 4.0);
        assert_eq!(c.source, 2);
        assert_eq!(c.fill, Fill::Trim);
    }

    #[test]
    fn coupe_et_duplication() {
        let mut p = project();
        let id = p.add_clip(clip_from_slice(1, 0, 0, 1000, 0.0, 0.25, 4.0));
        let right = split_at(&mut p, id, 1.0).expect("coupe");
        assert_eq!(p.clip(id).unwrap().length_beats, 1.0);
        assert_eq!(p.clip(id).unwrap().src_end, 250);
        assert_eq!(p.clip(right).unwrap().at_beat, 1.0);
        assert_eq!(p.clip(right).unwrap().length_beats, 3.0);
        assert_eq!(p.clip(right).unwrap().src_start, 250);
        // hors du clip : refuse, ne coupe pas
        assert!(split_at(&mut p, id, 0.0).is_none());
        let dup = duplicate(&mut p, id).expect("duplication");
        assert_eq!(p.clip(dup).unwrap().at_beat, 1.0);
        drop_empty(&mut p);
        assert_eq!(p.clips.len(), 3);
    }

    #[test]
    fn ajuster_et_boucler_sur_la_mesure() {
        let mut p = project();
        let id = p.add_clip(clip_from_slice(1, 0, 0, 1000, 0.0, 0.25, 3.0));
        // 3 temps → 4 temps (la mesure suivante)
        assert!(fit_bar(&mut p, id, 4.0, 0.25));
        assert_eq!(p.clip(id).unwrap().length_beats, 4.0);
        assert_eq!(p.clip(id).unwrap().fill, Fill::Trim);
        // déjà sur la mesure : rien à faire, pas d'entrée d'historique inutile
        assert!(!fit_bar(&mut p, id, 4.0, 0.25));
        // boucler : même longueur, mais le contenu se répète
        assert!(loop_to_bar(&mut p, id, 4.0, 0.25));
        assert_eq!(p.clip(id).unwrap().fill, Fill::Loop);
        let mut p2 = project();
        let id2 = p2.add_clip(clip_from_slice(1, 0, 0, 1000, 2.0, 0.25, 1.5));
        assert!(loop_to_bar(&mut p2, id2, 4.0, 0.25));
        assert_eq!(p2.clip(id2).unwrap().length_beats, 2.0, "de 2 à 4 temps");
    }

    #[test]
    fn historique_annule_et_refait() {
        let mut p = project();
        let mut h = History::default();
        h.checkpoint("ajout", &p);
        let id = p.add_clip(clip_from_slice(1, 0, 0, 1000, 0.0, 0.25, 4.0));
        assert_eq!(p.clips.len(), 1);
        assert_eq!(h.undo(&mut p).as_deref(), Some("ajout"));
        assert_eq!(p.clips.len(), 0);
        assert_eq!(h.redo(&mut p).as_deref(), Some("ajout"));
        assert_eq!(p.clips.len(), 1);
        assert!(p.clip(id).is_some());
        // une nouvelle action jette la branche refaite
        h.undo(&mut p);
        h.checkpoint("autre", &p);
        assert!(!h.can_redo());
    }
}
