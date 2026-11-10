//! Immagini dell'anteprima: lettura, misura in celle, codifica per il terminale.

use std::{collections::HashMap, path::PathBuf};

use image::{imageops::FilterType, DynamicImage};
use ratatui::layout::Rect;
use ratatui_image::{picker::Picker, protocol::Protocol, Resize};

pub(crate) struct ImageSlot {
    pub(crate) line: usize,
    pub(crate) x: u16,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) path: PathBuf,
}

/// Immagini dell'anteprima: lettura, misura in celle e codifica per il terminale, con cache.
pub(crate) struct Images {
    pub(crate) picker: Picker,
    /// cartella del file .md: i percorsi relativi delle immagini partono da qui
    base: PathBuf,
    /// altezza massima di un'immagine, in righe
    pub(crate) max_rows: u16,
    // ponytail: le cache non si aggiornano se un'immagine cambia su disco; serve riavviare l'editor
    decoded: HashMap<PathBuf, Option<DynamicImage>>,
    resized: HashMap<(PathBuf, u16, u16), DynamicImage>,
    protocols: HashMap<(PathBuf, u16, u16, u16, u16), Protocol>,
}

impl Images {
    pub(crate) fn new(picker: Picker, base: PathBuf) -> Self {
        Images { picker, base, max_rows: 20, decoded: HashMap::new(), resized: HashMap::new(), protocols: HashMap::new() }
    }

    /// Celle (colonne, righe) occupate dall'immagine `dest` entro `width` colonne, rispettando le
    /// proporzioni. None se non è un file locale leggibile (remota, mancante, formato non supportato).
    pub(crate) fn size(&mut self, dest: &str, width: u16) -> Option<(PathBuf, u16, u16)> {
        if dest.contains("://") || width == 0 {
            return None;
        }
        let path = self.base.join(dest);
        let img = self.decoded.entry(path.clone()).or_insert_with(|| image::open(&path).ok()).as_ref()?;
        let (fw, fh) = self.picker.font_size();
        let (fw, fh, w, h) = (fw as f64, fh as f64, img.width() as f64, img.height() as f64);
        let mut cols = (w / fw).ceil().clamp(1.0, width as f64);
        let mut rows = (cols * fw * h / w / fh).ceil().max(1.0);
        if rows > self.max_rows as f64 {
            rows = self.max_rows as f64;
            cols = (rows * fh * w / h / fw).round().clamp(1.0, width as f64);
        }
        Some((path, cols as u16, rows as u16))
    }

    /// Immagine codificata per il terminale, limitata alle righe `skip..skip + take`: le altre sono
    /// fuori dallo schermo. Ritagliare l'immagine stessa funziona con ogni protocollo, sixel compreso.
    pub(crate) fn protocol(&mut self, slot: &ImageSlot, skip: u16, take: u16) -> Option<&Protocol> {
        let key = (slot.path.clone(), slot.cols, slot.rows, skip, take);
        if !self.protocols.contains_key(&key) {
            // ponytail: svuota tutto oltre una soglia; un LRU se lo scorrimento tra molte immagini rallenta
            if self.protocols.len() > 64 {
                self.protocols.clear();
            }
            if self.resized.len() > 16 {
                self.resized.clear();
            }
            let (fw, fh) = self.picker.font_size();
            let (pw, ph) = (slot.cols as u32 * fw as u32, slot.rows as u32 * fh as u32);
            let img = self.decoded.get(&slot.path)?.as_ref()?;
            let full = self
                .resized
                .entry((slot.path.clone(), slot.cols, slot.rows))
                .or_insert_with(|| img.resize_exact(pw, ph, FilterType::Triangle));
            let part = full.crop_imm(0, skip as u32 * fh as u32, pw, take as u32 * fh as u32);
            let proto = self.picker.new_protocol(part, Rect::new(0, 0, slot.cols, take), Resize::Fit(None)).ok()?;
            self.protocols.insert(key.clone(), proto);
        }
        self.protocols.get(&key)
    }
}
