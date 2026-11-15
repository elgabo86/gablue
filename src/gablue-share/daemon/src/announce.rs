//! Annonce mDNS du poste (`_gablueshare._tcp`), tenue par `avahi-publish-service`.
//!
//! Règles :
//! - rien n'est publié tant qu'aucun dossier n'est partagé : un poste sans
//!   partage n'existe pas pour les autres, et n'apparaît donc dans aucune liste ;
//! - l'annonce nomme le poste et porte `peer_id` (identité stable) et le
//!   nombre de partages dans son TXT ;
//! - le processus d'annonce est lié à la vie du démon (`PR_SET_PDEATHSIG`) :
//!   il ne peut pas survivre à un crash, donc jamais d'annonce fantôme.

use crate::Shared;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

const SERVICE: &str = "_gablueshare._tcp";

/// État souhaité de l'annonce ; comparé à l'état courant pour ne republier
/// que lorsque quelque chose change.
#[derive(PartialEq, Eq)]
struct Wanted {
    name: String,
    shares: usize,
}

pub fn spawn(state: Shared, port: u16) {
    std::thread::spawn(move || {
        let mut child: Option<Child> = None;
        let mut announced: Option<Wanted> = None;

        loop {
            let (name, peer_id, shares, announces) = {
                let snapshot = Arc::clone(&state.read().unwrap_or_else(|p| p.into_inner()));
                (
                    snapshot.name.clone(),
                    snapshot.peer_id.clone(),
                    snapshot.available().count(),
                    snapshot.visibility.announces(),
                )
            };
            // mDNS ne parle que sur le réseau local : en visibilité « tailnet
            // seulement », on n'annonce rien (les autres postes trouvent le
            // nôtre par leur sonde `tailscale status`).
            let wanted = if shares == 0 || !announces {
                None
            } else {
                Some(Wanted {
                    name: name.clone(),
                    shares,
                })
            };

            // Le processus est mort (démon avahi relancé, erreur) : on republie.
            if let Some(handle) = child.as_mut() {
                if matches!(handle.try_wait(), Ok(Some(_))) {
                    child = None;
                    announced = None;
                }
            }

            if wanted != announced {
                stop(&mut child);
                if let Some(target) = &wanted {
                    match publish(&target.name, &peer_id, shares, port) {
                        Ok(handle) => {
                            eprintln!(
                                "gablue-shared : annonce « {} » sur {SERVICE} ({} partage(s))",
                                target.name, target.shares
                            );
                            child = Some(handle);
                            announced = wanted;
                        }
                        Err(err) => {
                            eprintln!("gablue-shared : annonce impossible ({err})");
                            announced = None;
                        }
                    }
                } else if shares > 0 {
                    eprintln!(
                        "gablue-shared : annonce retirée (visibilité tailnet seulement)"
                    );
                    announced = None;
                } else {
                    eprintln!("gablue-shared : plus aucun partage, annonce retirée");
                    announced = None;
                }
            }

            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

fn publish(name: &str, peer_id: &str, shares: usize, port: u16) -> std::io::Result<Child> {
    let mut command = Command::new("avahi-publish-service");
    command
        .arg("-s")
        .arg(name)
        .arg(SERVICE)
        .arg(port.to_string())
        .arg(format!("peer_id={peer_id}"))
        .arg("v=1")
        .arg(format!("shares={shares}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // L'annonce meurt avec le démon, y compris sur SIGKILL : pas de fantôme.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    command.spawn()
}

fn stop(child: &mut Option<Child>) {
    if let Some(mut handle) = child.take() {
        let _ = handle.kill();
        let _ = handle.wait();
    }
}
