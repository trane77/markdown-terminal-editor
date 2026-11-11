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
    pub(crate) keys: &'static str,
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
    keys: "^E editor  ^P preview  ^L split/rotate  ^S save  ^Q quit",
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
    keys: "^E editor  ^P vista previa  ^L dividir/girar  ^S guardar  ^Q salir",
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
    keys: "^E Editor  ^P Vorschau  ^L teilen/drehen  ^S speichern  ^Q beenden",
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
    keys: "^E editor  ^P pré-visualização  ^L dividir/rodar  ^S guardar  ^Q sair",
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
    keys: "^E éditeur  ^P aperçu  ^L partager/pivoter  ^S enregistrer  ^Q quitter",
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
    keys: "^E editor  ^P anteprima  ^L split/ruota  ^S salva  ^Q esci",
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
};

/// Language from the POSIX locale (LC_ALL > LC_MESSAGES > LANG, e.g. "de_DE.UTF-8"); English if unset or untranslated.
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

pub(crate) fn tr() -> &'static Tr {
    static TR: OnceLock<&Tr> = OnceLock::new();
    TR.get_or_init(|| {
        let var = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|v| env::var(v).ok().filter(|s| !s.is_empty()));
        lang(&var.unwrap_or_default())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_language_from_locale() {
        assert_eq!(lang("de_DE.UTF-8").preview, "Vorschau");
        assert_eq!(lang("pt_BR").saved, "Guardado");
        assert_eq!(lang("C.UTF-8").preview, "Preview");
        assert_eq!(lang("").preview, "Preview");
    }
}
