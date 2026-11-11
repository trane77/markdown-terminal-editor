//! UI texts in several languages, picked from the locale.

use std::{env, sync::OnceLock};

/// UI texts in one language.
pub(crate) struct Tr {
    pub(crate) saved: &'static str,
    pub(crate) save_error: &'static str,
    pub(crate) usage: &'static str,
    pub(crate) unsaved: &'static str,
    pub(crate) save_quit: &'static str,
    pub(crate) quit_anyway: &'static str,
    pub(crate) editor: &'static str,
    pub(crate) preview: &'static str,
    pub(crate) preview_keys: &'static str,
    pub(crate) split_side: &'static str,
    pub(crate) split_stacked: &'static str,
    /// labels of the shortcut keys ^E ^P ^L ^F ^S ^Z ^Y ^Q, in this order
    pub(crate) hints: [&'static str; 8],
    pub(crate) find: &'static str,
    pub(crate) next: &'static str,
    pub(crate) previous: &'static str,
    pub(crate) close: &'static str,
    pub(crate) no_matches: &'static str,
    pub(crate) lines: &'static str,
    pub(crate) line: &'static str,
    pub(crate) image: &'static str,
    pub(crate) no_anchor: &'static str,
    pub(crate) open_link: &'static str,
    pub(crate) ok: &'static str,
    pub(crate) cancel: &'static str,
    pub(crate) open_error: &'static str,
    pub(crate) changed_on_disk: &'static str,
    pub(crate) reload: &'static str,
    pub(crate) keep: &'static str,
    pub(crate) reloaded: &'static str,
    /// titles of the GitHub alerts `> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`
    pub(crate) alerts: [&'static str; 5],
}

const EN: Tr = Tr {
    saved: "Saved",
    save_error: "Error saving file",
    usage: "usage",
    unsaved: "The file has unsaved changes. Save them before quitting?",
    save_quit: "Save and quit",
    quit_anyway: "Quit without saving",
    editor: "Editor",
    preview: "Preview",
    preview_keys: "↑↓ PgUp PgDn",
    split_side: "side-by-side split",
    split_stacked: "stacked split",
    hints: ["editor", "preview", "split", "find", "save", "undo", "redo", "quit"],
    find: "Find",
    next: "next",
    previous: "previous",
    close: "close",
    no_matches: "no matches",
    lines: "lines",
    line: "Ln",
    image: "image",
    no_anchor: "Link target not found",
    open_link: "Open this external link in the browser?",
    ok: "OK",
    cancel: "Cancel",
    open_error: "Cannot open the browser",
    changed_on_disk: "The file was changed by another program. Reload it from disk? Changes made here will be lost.",
    reload: "Reload",
    keep: "Keep my version",
    reloaded: "Reloaded from disk",
    alerts: ["Note", "Tip", "Important", "Warning", "Caution"],
};

const ES: Tr = Tr {
    saved: "Guardado",
    save_error: "Error al guardar",
    usage: "uso",
    unsaved: "El archivo tiene cambios sin guardar. ¿Guardarlos antes de salir?",
    save_quit: "Guardar y salir",
    quit_anyway: "Salir sin guardar",
    editor: "Editor",
    preview: "Vista previa",
    preview_keys: "↑↓ RePág AvPág",
    split_side: "división lado a lado",
    split_stacked: "división apilada",
    hints: [
        "editor",
        "vista previa",
        "dividir",
        "buscar",
        "guardar",
        "deshacer",
        "rehacer",
        "salir",
    ],
    find: "Buscar",
    next: "siguiente",
    previous: "anterior",
    close: "cerrar",
    no_matches: "sin resultados",
    lines: "líneas",
    line: "Lín",
    image: "imagen",
    no_anchor: "Destino del enlace no encontrado",
    open_link: "¿Abrir este enlace externo en el navegador?",
    ok: "Aceptar",
    cancel: "Cancelar",
    open_error: "No se puede abrir el navegador",
    changed_on_disk:
        "Otro programa ha modificado el archivo. ¿Recargarlo desde el disco? Se perderán los cambios hechos aquí.",
    reload: "Recargar",
    keep: "Mantener mi versión",
    reloaded: "Recargado desde el disco",
    alerts: ["Nota", "Consejo", "Importante", "Advertencia", "Precaución"],
};

const DE: Tr = Tr {
    saved: "Gespeichert",
    save_error: "Fehler beim Speichern",
    usage: "Verwendung",
    unsaved: "Die Datei hat ungespeicherte Änderungen. Vor dem Beenden speichern?",
    save_quit: "Speichern und beenden",
    quit_anyway: "Ohne Speichern beenden",
    editor: "Editor",
    preview: "Vorschau",
    preview_keys: "↑↓ Bild↑ Bild↓",
    split_side: "geteilt nebeneinander",
    split_stacked: "geteilt übereinander",
    hints: ["Editor", "Vorschau", "teilen", "suchen", "speichern", "rückgängig", "wiederholen", "beenden"],
    find: "Suchen",
    next: "weiter",
    previous: "zurück",
    close: "schließen",
    no_matches: "keine Treffer",
    lines: "Zeilen",
    line: "Z",
    image: "Bild",
    no_anchor: "Linkziel nicht gefunden",
    open_link: "Diesen externen Link im Browser öffnen?",
    ok: "OK",
    cancel: "Abbrechen",
    open_error: "Browser kann nicht geöffnet werden",
    changed_on_disk: "Die Datei wurde von einem anderen Programm geändert. Von der Festplatte neu laden? Die hier gemachten Änderungen gehen verloren.",
    reload: "Neu laden",
    keep: "Meine Version behalten",
    reloaded: "Von der Festplatte neu geladen",
    alerts: ["Hinweis", "Tipp", "Wichtig", "Warnung", "Vorsicht"],
};

const PT: Tr = Tr {
    saved: "Guardado",
    save_error: "Erro ao guardar",
    usage: "uso",
    unsaved: "O ficheiro tem alterações não guardadas. Guardá-las antes de sair?",
    save_quit: "Guardar e sair",
    quit_anyway: "Sair sem guardar",
    editor: "Editor",
    preview: "Pré-visualização",
    preview_keys: "↑↓ PgUp PgDn",
    split_side: "divisão lado a lado",
    split_stacked: "divisão empilhada",
    hints: [
        "editor",
        "pré-visualização",
        "dividir",
        "procurar",
        "guardar",
        "anular",
        "refazer",
        "sair",
    ],
    find: "Procurar",
    next: "seguinte",
    previous: "anterior",
    close: "fechar",
    no_matches: "sem resultados",
    lines: "linhas",
    line: "Ln",
    image: "imagem",
    no_anchor: "Destino da ligação não encontrado",
    open_link: "Abrir esta ligação externa no navegador?",
    ok: "OK",
    cancel: "Cancelar",
    open_error: "Não é possível abrir o navegador",
    changed_on_disk:
        "O ficheiro foi alterado por outro programa. Recarregá-lo do disco? As alterações feitas aqui serão perdidas.",
    reload: "Recarregar",
    keep: "Manter a minha versão",
    reloaded: "Recarregado do disco",
    alerts: ["Nota", "Dica", "Importante", "Aviso", "Cuidado"],
};

const FR: Tr = Tr {
    saved: "Enregistré",
    save_error: "Erreur d'enregistrement",
    usage: "usage",
    unsaved: "Le fichier contient des modifications non enregistrées. Les enregistrer avant de quitter ?",
    save_quit: "Enregistrer et quitter",
    quit_anyway: "Quitter sans enregistrer",
    editor: "Éditeur",
    preview: "Aperçu",
    preview_keys: "↑↓ PgPréc PgSuiv",
    split_side: "partage côte à côte",
    split_stacked: "partage superposé",
    hints: ["éditeur", "aperçu", "partager", "chercher", "enregistrer", "annuler", "rétablir", "quitter"],
    find: "Chercher",
    next: "suivant",
    previous: "précédent",
    close: "fermer",
    no_matches: "aucun résultat",
    lines: "lignes",
    line: "Ln",
    image: "image",
    no_anchor: "Cible du lien introuvable",
    open_link: "Ouvrir ce lien externe dans le navigateur ?",
    ok: "OK",
    cancel: "Annuler",
    open_error: "Impossible d'ouvrir le navigateur",
    changed_on_disk: "Le fichier a été modifié par un autre programme. Le recharger depuis le disque ? Les modifications faites ici seront perdues.",
    reload: "Recharger",
    keep: "Garder ma version",
    reloaded: "Rechargé depuis le disque",
    alerts: ["Remarque", "Astuce", "Important", "Avertissement", "Attention"],
};

const IT: Tr = Tr {
    saved: "Salvato",
    save_error: "Errore nel salvataggio",
    usage: "uso",
    unsaved: "Il file ha modifiche non salvate. Salvarle prima di uscire?",
    save_quit: "Salva ed esci",
    quit_anyway: "Esci senza salvare",
    editor: "Editor",
    preview: "Anteprima",
    preview_keys: "↑↓ PgSu PgGiù",
    split_side: "split affiancato",
    split_stacked: "split sovrapposto",
    hints: ["editor", "anteprima", "split", "cerca", "salva", "annulla", "ripristina", "esci"],
    find: "Cerca",
    next: "successivo",
    previous: "precedente",
    close: "chiudi",
    no_matches: "nessun risultato",
    lines: "righe",
    line: "Ln",
    image: "immagine",
    no_anchor: "Destinazione del link non trovata",
    open_link: "Aprire il link esterno nel browser?",
    ok: "OK",
    cancel: "Annulla",
    open_error: "Impossibile aprire il browser",
    changed_on_disk: "Il file è stato modificato da un altro programma. Ricaricarlo dal disco? Le modifiche fatte qui andranno perse.",
    reload: "Ricarica",
    keep: "Mantieni la mia versione",
    reloaded: "Ricaricato dal disco",
    alerts: ["Nota", "Suggerimento", "Importante", "Avvertenza", "Attenzione"],
};

/// Language from a locale name, POSIX ("de_DE.UTF-8") or BCP 47 ("de-DE"); English if untranslated.
fn lang(locale: &str) -> &'static Tr {
    match locale.get(..2) {
        Some("es") => &ES,
        Some("de") => &DE,
        Some("pt") => &PT,
        Some("fr") => &FR,
        Some("it") => &IT,
        _ => &EN,
    }
}

/// The POSIX locale variables (LC_ALL > LC_MESSAGES > LANG) win, so they also override the system
/// language on Windows and macOS. If none is set (usual on Windows, and in some macOS terminals),
/// the user's preferred UI language from the OS is used.
pub(crate) fn tr() -> &'static Tr {
    static TR: OnceLock<&Tr> = OnceLock::new();
    TR.get_or_init(|| {
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|v| env::var(v).ok().filter(|s| !s.is_empty()))
            .or_else(sys_locale::get_locale);
        lang(&locale.unwrap_or_default())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_language_from_locale() {
        assert_eq!(lang("de_DE.UTF-8").preview, "Vorschau");
        assert_eq!(lang("pt_BR").saved, "Guardado");
        assert_eq!(lang("it-IT").preview, "Anteprima");
        assert_eq!(lang("C.UTF-8").preview, "Preview");
        assert_eq!(lang("").preview, "Preview");
    }
}
