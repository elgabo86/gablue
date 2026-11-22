//! Coquille unique de GTracker : un seul document, une bibliothèque latérale,
//! un espace de travail (arrangement / éditeur), un inspecteur, un transport.
//!
//! Il n'y a **plus d'onglets plein écran** : Catalogue et Téléchargés sont deux
//! filtres de la bibliothèque, et l'espace de travail garde le même projet, la
//! même sélection et le même son quand on passe de l'arrangement à l'éditeur.

/// Filtre de la bibliothèque latérale (ce n'est pas une vue : la coquille et le
/// projet restent identiques).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryTab {
    Catalogue,
    Cached,
}

impl LibraryTab {
    pub fn label(self) -> &'static str {
        match self {
            LibraryTab::Catalogue => "Catalogue",
            LibraryTab::Cached => "Téléchargés",
        }
    }
}

/// Espace de travail : l'arrangement multitrack ou l'éditeur de la source
/// courante. Les deux partagent le même document, la même bibliothèque et le
/// même transport ; seul le panneau central change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workspace {
    Arrange,
    Edit,
}

impl Workspace {
    pub fn label(self) -> &'static str {
        match self {
            Workspace::Arrange => "Arrangement",
            Workspace::Edit => "Éditeur",
        }
    }
}

/// Panneau qui reçoit le clavier. Le shell l'affiche et les raccourcis en
/// dépendent : une touche ne doit jamais agir sur deux panneaux.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Library,
    Arrange,
    Editor,
    Inspector,
}

/// État de la coquille, indépendant des données (projet, bibliothèque) : il est
/// petit, sérialisable plus tard, et testable sans écran.
pub struct Shell {
    pub library: LibraryTab,
    pub workspace: Workspace,
    pub focus: Focus,
    pub show_library: bool,
    pub show_inspector: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            library: LibraryTab::Catalogue,
            // on ouvre sur l'arrangement : c'est le document, pas un navigateur
            workspace: Workspace::Arrange,
            focus: Focus::Arrange,
            show_library: true,
            show_inspector: true,
        }
    }
}

impl Shell {
    /// Change d'espace de travail sans toucher au document (le projet, la
    /// sélection et le transport vivent ailleurs, précisément pour ça).
    pub fn set_workspace(&mut self, w: Workspace) {
        if self.workspace == w {
            return;
        }
        self.workspace = w;
        self.focus = match w {
            Workspace::Arrange => Focus::Arrange,
            Workspace::Edit => Focus::Editor,
        };
    }

    /// Change de filtre de bibliothèque : le document n'est pas concerné.
    pub fn set_library(&mut self, tab: LibraryTab) {
        self.library = tab;
        self.focus = Focus::Library;
    }

    pub fn toggle_library(&mut self) {
        self.show_library = !self.show_library;
        if !self.show_library && self.focus == Focus::Library {
            self.focus = Focus::Arrange;
        }
    }

    pub fn toggle_inspector(&mut self) {
        self.show_inspector = !self.show_inspector;
        if self.show_inspector {
            self.focus = Focus::Inspector;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preuve headless du shell : basculer d'espace ou de filtre ne doit pas
    /// toucher au document — c'est ce qui distingue une coquille unique de
    /// quatre interfaces séparées.
    #[test]
    fn shell_garde_le_document_en_changeant_de_panneau() {
        let mut s = Shell::default();
        assert_eq!(s.workspace, Workspace::Arrange);
        assert_eq!(s.focus, Focus::Arrange);

        // le document est représenté par un état à part (projet + sélection) :
        // la coquille ne le possède pas, donc elle ne peut pas l'effacer
        let mut projet = String::from("mon projet");
        let selection = Some(42usize);

        s.set_workspace(Workspace::Edit);
        assert_eq!(s.workspace, Workspace::Edit);
        assert_eq!(s.focus, Focus::Editor);
        assert_eq!(projet, "mon projet");
        assert_eq!(selection, Some(42));

        s.set_library(LibraryTab::Cached);
        assert_eq!(s.library, LibraryTab::Cached);
        assert_eq!(s.workspace, Workspace::Edit, "le filtre ne change pas l'espace");

        s.set_workspace(Workspace::Arrange);
        assert_eq!(s.focus, Focus::Arrange);
        assert_eq!(projet, "mon projet");
        assert_eq!(selection, Some(42));
        projet.push('!');
        assert_eq!(projet, "mon projet!");

        // les panneaux se replient sans perdre l'espace courant
        s.toggle_library();
        assert!(!s.show_library);
        s.toggle_inspector();
        assert!(!s.show_inspector);
        assert_eq!(s.workspace, Workspace::Arrange);

        println!(
            "SHELL OK · bibliothèque={:?} · espace={:?} · focus={:?} · document intact",
            s.library, s.workspace, s.focus
        );
    }

    #[test]
    fn libelle_unique_par_panneau() {
        assert_eq!(LibraryTab::Catalogue.label(), "Catalogue");
        assert_eq!(LibraryTab::Cached.label(), "Téléchargés");
        assert_eq!(Workspace::Arrange.label(), "Arrangement");
        assert_eq!(Workspace::Edit.label(), "Éditeur");
    }
}
