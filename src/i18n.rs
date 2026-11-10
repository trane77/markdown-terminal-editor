//! Testi dell'interfaccia nelle varie lingue, scelti dal locale.

use std::{env, sync::OnceLock};

/// Testi dell'interfaccia in una lingua.
pub(crate) struct Tr {
    pub(crate) saved: &'static str,
    pub(crate) save_error: &'static str,
    pub(crate) usage: &'static str,
    pub(crate) unsaved: &'static str,
    pub(crate) editor: &'static str,
    pub(crate) preview: &'static str,
    pub(crate) preview_keys: &'static str,
    pub(crate) split_side: &'static str,
    pub(crate) split_stacked: &'static str,
    pub(crate) keys: &'static str,
    pub(crate) line: &'static str,
    pub(crate) image: &'static str,
    pub(crate) no_anchor: &'static str,
}

const EN: Tr = Tr {
    saved: "Saved",
    save_error: "Error saving file",
    usage: "usage",
    unsaved: "Unsaved changes! Press Ctrl+Q again to quit without saving",
    editor: "Editor",
    preview: "Preview",
    preview_keys: "↑↓ PgUp PgDn",
    split_side: "side-by-side split",
    split_stacked: "stacked split",
    keys: "^E editor  ^P preview  ^L split/rotate  ^S save  ^Q quit",
    line: "Ln",
    image: "image",
    no_anchor: "Link target not found",
};

const ES: Tr = Tr {
    saved: "Guardado",
    save_error: "Error al guardar",
    usage: "uso",
    unsaved: "¡Cambios sin guardar! Pulsa Ctrl+Q otra vez para salir sin guardar",
    editor: "Editor",
    preview: "Vista previa",
    preview_keys: "↑↓ RePág AvPág",
    split_side: "división lado a lado",
    split_stacked: "división apilada",
    keys: "^E editor  ^P vista previa  ^L dividir/girar  ^S guardar  ^Q salir",
    line: "Lín",
    image: "imagen",
    no_anchor: "Destino del enlace no encontrado",
};

const DE: Tr = Tr {
    saved: "Gespeichert",
    save_error: "Fehler beim Speichern",
    usage: "Verwendung",
    unsaved: "Ungespeicherte Änderungen! Erneut Strg+Q zum Beenden ohne Speichern",
    editor: "Editor",
    preview: "Vorschau",
    preview_keys: "↑↓ Bild↑ Bild↓",
    split_side: "geteilt nebeneinander",
    split_stacked: "geteilt übereinander",
    keys: "^E Editor  ^P Vorschau  ^L teilen/drehen  ^S speichern  ^Q beenden",
    line: "Z",
    image: "Bild",
    no_anchor: "Linkziel nicht gefunden",
};

const PT: Tr = Tr {
    saved: "Guardado",
    save_error: "Erro ao guardar",
    usage: "uso",
    unsaved: "Alterações não guardadas! Prima Ctrl+Q outra vez para sair sem guardar",
    editor: "Editor",
    preview: "Pré-visualização",
    preview_keys: "↑↓ PgUp PgDn",
    split_side: "divisão lado a lado",
    split_stacked: "divisão empilhada",
    keys: "^E editor  ^P pré-visualização  ^L dividir/rodar  ^S guardar  ^Q sair",
    line: "Ln",
    image: "imagem",
    no_anchor: "Destino da ligação não encontrado",
};

const FR: Tr = Tr {
    saved: "Enregistré",
    save_error: "Erreur d'enregistrement",
    usage: "usage",
    unsaved: "Modifications non enregistrées ! Ctrl+Q à nouveau pour quitter sans enregistrer",
    editor: "Éditeur",
    preview: "Aperçu",
    preview_keys: "↑↓ PgPréc PgSuiv",
    split_side: "partage côte à côte",
    split_stacked: "partage superposé",
    keys: "^E éditeur  ^P aperçu  ^L partager/pivoter  ^S enregistrer  ^Q quitter",
    line: "Ln",
    image: "image",
    no_anchor: "Cible du lien introuvable",
};

const IT: Tr = Tr {
    saved: "Salvato",
    save_error: "Errore nel salvataggio",
    usage: "uso",
    unsaved: "Modifiche non salvate! Ctrl+Q di nuovo per uscire senza salvare",
    editor: "Editor",
    preview: "Anteprima",
    preview_keys: "↑↓ PgSu PgGiù",
    split_side: "split affiancato",
    split_stacked: "split sovrapposto",
    keys: "^E editor  ^P anteprima  ^L split/ruota  ^S salva  ^Q esci",
    line: "Ln",
    image: "immagine",
    no_anchor: "Destinazione del link non trovata",
};

/// Lingua dal locale POSIX (LC_ALL > LC_MESSAGES > LANG, es. "de_DE.UTF-8"); inglese se assente o non tradotta.
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
        let var = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|v| env::var(v).ok().filter(|s| !s.is_empty()));
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
