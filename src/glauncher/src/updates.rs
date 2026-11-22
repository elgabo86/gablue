//! Mises à jour : commande en tâche de fond et progression lisible.
//!
//! La sortie de la commande suit le protocole `::gablue:` déjà utilisé par les
//! dialogues de progression de Gablue Manager
//! (`gablue_manager/local.py`, `parse_gablue_progress`) :
//!
//! ```text
//! ::gablue: step n total libellé     une étape commence ; (n-1)/total est derrière
//! ::gablue: busy libellé             travail sans total → barre indéterminée
//! ::gablue: percent valeur libellé   avancement mesuré (octets, moteurs…)
//! ::gablue: done [libellé]           100 %
//! ```
//!
//! **Un seul protocole, deux rendus** : le manager affiche un dialogue, le
//! launcher une barre. Ne pas inventer un second format — une commande de mise
//! à jour doit pouvoir être branchée sur l'un ou l'autre sans être réécrite.

use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

/// Préfixe des lignes de protocole (identique à `GABLUE_PROGRESS_PREFIX`).
pub const PREFIX: &str = "::gablue:";

/// Commande par défaut : l'image, sans mot de passe (`sudoers.d/nopasswd`
/// autorise `%wheel` sur `bootc upgrade`). `GLAUNCHER_UPDATE_CMD` la remplace,
/// pour le banc d'essai (faux émetteur de protocole, aucune écriture).
pub fn default_command() -> String {
    command_from(std::env::var("GLAUNCHER_UPDATE_CMD").ok())
}

/// Cœur testable de [`default_command`] (l'environnement n'est pas modifiable
/// dans un test : `set_var` est `unsafe` depuis l'édition 2024).
fn command_from(override_value: Option<String>) -> String {
    override_value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "sudo -n bootc upgrade".to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Travail sans total connu (téléchargement, vérification…).
    Busy(String),
    /// Avancement mesuré, en pourcentage (0 à 100).
    Percent(f32, String),
    /// Étape `n` sur `total`.
    Progress(usize, usize, String),
    /// Terminé, libellé facultatif.
    Done(Option<String>),
}

/// Une ligne de sortie → une étape. `None` = ligne de journal ordinaire, qui
/// n'a pas à faire bouger la barre.
pub fn parse(line: &str) -> Option<Step> {
    let text = line.trim_end();
    let rest = text.strip_prefix(PREFIX)?.trim_start();
    // `split_once` évite d'allouer une liste pour deux mots
    let (verb, rest) = rest.split_once(' ').unwrap_or((rest, ""));
    let rest = rest.trim();
    match verb {
        // même règle que le Python : les trois morceaux sont obligatoires,
        // un `step` incomplet est ignoré plutôt que deviné
        "step" => {
            let mut parts = rest.splitn(3, ' ');
            let step: usize = parts.next()?.parse().ok()?;
            let total: usize = parts.next()?.parse().ok()?;
            let label = parts.next()?.trim();
            if label.is_empty() {
                return None;
            }
            Some(Step::Progress(step.max(1), total.max(1), label.to_string()))
        }
        "busy" => Some(Step::Busy(if rest.is_empty() {
            "En cours…".to_string()
        } else {
            rest.to_string()
        })),
        "percent" => {
            let (value, label) = rest.split_once(' ').unwrap_or((rest, ""));
            let percent: f32 = value.parse().ok()?;
            Some(Step::Percent(
                percent.clamp(0.0, 100.0),
                label.trim().to_string(),
            ))
        }
        "done" => Some(Step::Done(
            (!rest.is_empty()).then(|| rest.to_string()),
        )),
        _ => None,
    }
}

/// État de la mise à jour en cours (ou de la dernière).
#[derive(Debug, Clone)]
pub struct Update {
    pub label: String,
    /// `None` = avancement inconnu : barre indéterminée.
    pub percent: Option<f32>,
    pub done: bool,
    pub error: Option<String>,
    /// Dernière ligne de journal de la commande : c'est elle qu'on affiche
    /// quand ça échoue (« failed to download… » plutôt qu'un code de sortie).
    pub detail: String,
}

impl Default for Update {
    fn default() -> Self {
        Self {
            label: "préparation…".to_string(),
            percent: None,
            done: false,
            error: None,
            detail: String::new(),
        }
    }
}

impl Update {
    /// Applique une étape du protocole. `step` compte en `(n-1)/total` : la
    /// barre n'atteint 100 % qu'au `done` (même règle que Gablue Manager).
    pub fn apply(&mut self, step: Step) {
        match step {
            Step::Busy(label) => {
                self.label = label;
                self.percent = None;
            }
            Step::Percent(percent, label) => {
                if !label.is_empty() {
                    self.label = label;
                }
                self.percent = Some(percent);
            }
            Step::Progress(step, total, label) => {
                self.label = format!("Étape {step}/{total} — {label}");
                self.percent = Some(100.0 * (step - 1) as f32 / total as f32);
            }
            Step::Done(label) => {
                if let Some(label) = label {
                    self.label = label;
                }
                self.percent = Some(100.0);
                self.done = true;
            }
        }
    }

    /// Fin de la commande. Code 0 : l'image est en attente (bootc/rpm-ostree
    /// « stage »), donc on ne dit jamais « redémarré », on dit « au
    /// redémarrage » ; « no changes » vaut « déjà à jour ».
    pub fn finish(&mut self, code: Option<i32>) {
        self.percent = Some(100.0);
        match code {
            Some(0) => {
                self.done = true;
                self.label = if self.deja_a_jour() {
                    "déjà à jour".to_string()
                } else {
                    "mise à jour prête — s'appliquera au redémarrage".to_string()
                };
            }
            Some(code) => {
                self.error = Some(format!("échec (code {code})"));
                if self.detail.is_empty() {
                    self.label = self.error.clone().unwrap_or_default();
                }
            }
            None => {
                self.error = Some("interrompu".to_string());
                if self.detail.is_empty() {
                    self.label = "interrompu".to_string();
                }
            }
        }
    }

    /// La sortie dit-elle qu'il n'y avait rien à faire ?
    pub fn deja_a_jour(&self) -> bool {
        let text = format!("{} {}", self.label, self.detail).to_lowercase();
        ["no changes", "no upgrade", "already up to date", "nothing to do"]
            .iter()
            .any(|marker| text.contains(marker))
    }
}

/// Évènements du fil de mise à jour, postés sur le bus interne.
#[derive(Debug, Clone)]
pub enum Event {
    /// Ligne de protocole.
    Line(Step),
    /// Ligne de journal (dernière retenue pour le message d'échec).
    Detail(String),
    /// Fin de la commande, avec son code (None = tuée par un signal).
    Exited(Option<i32>),
    /// La commande n'a pas pu être lancée.
    Failed(String),
}

/// Lance la commande et la lit ligne à ligne. Le fil meurt avec la commande.
/// `stderr` passe par le même canal : son contenu remonte en `Event::Detail`,
/// prêt à être affiché si la commande échoue.
pub fn spawn<F>(script: String, wake: F)
where
    F: Fn(Event) + Send + Sync + 'static,
{
    let wake = std::sync::Arc::new(wake);
    std::thread::spawn(move || {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(&script)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Un launcher tué ne doit pas laisser une commande root orpheline
        // (même choix que la musique : PR_SET_PDEATHSIG). L'image est
        // « stage » : une interruption ne casse pas le déploiement.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let child = command.spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(err) => {
                (*wake)(Event::Failed(format!("{err}")));
                return;
            }
        };

        // stderr dans un fil : un tube plein bloquerait la sortie de la commande
        let stderr = child.stderr.take();
        let err_wake = wake.clone();
        let reader = stderr.map(|stderr| {
            let err_wake = err_wake.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    (*err_wake)(Event::Detail(line));
                }
            })
        });

        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                match parse(&line) {
                    Some(step) => (*wake)(Event::Line(step)),
                    None if !line.trim().is_empty() => (*wake)(Event::Detail(line)),
                    None => {}
                }
            }
        }
        if let Some(reader) = reader {
            let _ = reader.join();
        }
        let code = child.wait().ok().and_then(|status| status.code());
        (*wake)(Event::Exited(code));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etape_compte_en_n_moins_un() {
        let step = parse("::gablue: step 1 4 téléchargement").unwrap();
        assert_eq!(step, Step::Progress(1, 4, "téléchargement".to_string()));
        let mut update = Update::default();
        update.apply(step);
        assert_eq!(update.percent, Some(0.0));
        update.apply(parse("::gablue: step 4 4 application").unwrap());
        assert_eq!(update.percent, Some(75.0));
        assert!(!update.done, "seul `done` fait 100 %");
    }

    #[test]
    fn busy_rend_la_barre_indeterminee() {
        let mut update = Update::default();
        update.apply(parse("::gablue: busy vérification").unwrap());
        assert_eq!(update.label, "vérification");
        assert_eq!(update.percent, None);
        // et le pourcentage suivant redevient mesuré
        update.apply(parse("::gablue: percent 42.5 octets").unwrap());
        assert_eq!(update.percent, Some(42.5));
    }

    #[test]
    fn pourcentage_borne() {
        assert_eq!(
            parse("::gablue: percent 140 trop").unwrap(),
            Step::Percent(100.0, "trop".to_string())
        );
        assert_eq!(
            parse("::gablue: percent -3 moins").unwrap(),
            Step::Percent(0.0, "moins".to_string())
        );
        assert_eq!(parse("::gablue: percent abc"), None);
    }

    #[test]
    fn lignes_ignorees() {
        assert_eq!(parse("Downloading layer…"), None);
        assert_eq!(parse("::gablue:"), None);
        assert_eq!(parse("::gablue: step 1"), None); // incomplet
        assert_eq!(parse("::gablue: step 1 4"), None); // libellé absent
        assert_eq!(parse("::gablue: inconnu x"), None);
    }

    #[test]
    fn done_et_fin_de_commande() {
        let mut update = Update::default();
        update.apply(parse("::gablue: done").unwrap());
        assert!(update.done);
        assert_eq!(update.percent, Some(100.0));

        let mut update = Update::default();
        update.detail = "No changes in /sysroot: 0 upgraded".to_string();
        update.finish(Some(0));
        assert!(update.done);
        assert!(update.deja_a_jour());
        assert_eq!(update.label, "déjà à jour");

        let mut update = Update::default();
        update.finish(Some(0));
        assert!(update.label.contains("redémarrage"));

        let mut update = Update::default();
        update.detail = "error: failed to download image".to_string();
        update.finish(Some(1));
        assert!(!update.done);
        assert_eq!(update.error.as_deref(), Some("échec (code 1)"));
        assert!(!update.deja_a_jour());
    }

    #[test]
    fn commande_surchargeable() {
        // pas de variable → défaut sans mot de passe
        assert_eq!(command_from(None), "sudo -n bootc upgrade");
        assert_eq!(command_from(Some("   ".into())), "sudo -n bootc upgrade");
        assert_eq!(command_from(Some("faux-emetteur".into())), "faux-emetteur");
    }
}
