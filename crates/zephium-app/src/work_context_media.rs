//! Bounded text disclosure from an already admitted, revision-bound media blob.
use zephium_core::{
    resources::{MediaAssetV1, MediaKind},
    work::{context::truncate, runtime::MAX_WORK_FILE_TEXT_BYTES},
};

const MAX_PDF_STREAM_BYTES: usize = 1024 * 1024;
const MAX_PDF_TEXT_PAGES: usize = 64;
static EXTRACTION_SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

pub(super) async fn disclose(asset: MediaAssetV1, bytes: Vec<u8>) -> String {
    let fallback = format!("{}\nno text layer", metadata(&asset));
    tokio::time::timeout(super::READ_TIMEOUT, async move {
        let permit = EXTRACTION_SLOT.acquire().await.ok()?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            text(&asset, &bytes)
        })
        .await
        .ok()
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(fallback)
}

pub(super) fn metadata(asset: &MediaAssetV1) -> String {
    format!("{} ({}, {} bytes)", asset.name, asset.mime, asset.bytes)
}

pub(super) fn text(asset: &MediaAssetV1, bytes: &[u8]) -> String {
    let (pages, body) = match asset.kind {
        MediaKind::Pdf => pdf(bytes),
        MediaKind::File => (
            Some(1),
            std::str::from_utf8(bytes)
                .ok()
                .filter(|text| {
                    !text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                })
                .map(|text| truncate(text, MAX_WORK_FILE_TEXT_BYTES).0),
        ),
        MediaKind::Image => return metadata(asset),
    };
    let heading = match pages {
        Some(pages) => format!(
            "{} ({}, {} bytes, {pages} pages)",
            asset.name, asset.mime, asset.bytes
        ),
        None => metadata(asset),
    };
    let body = body.filter(|body| !body.trim().is_empty());
    truncate(
        &format!("{heading}\n{}", body.as_deref().unwrap_or("no text layer")),
        MAX_WORK_FILE_TEXT_BYTES,
    )
    .0
}

fn pdf(bytes: &[u8]) -> (Option<usize>, Option<String>) {
    let started = std::time::Instant::now();
    let Ok(document) = lopdf::Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            strict: true,
            max_decompressed_size: Some(MAX_PDF_STREAM_BYTES),
            ..Default::default()
        },
    ) else {
        return (None, None);
    };
    if document.is_encrypted() {
        return (None, None);
    }
    let pages = document.get_pages();
    let count = pages.len();
    let mut text = String::new();
    for page in pages.keys().take(MAX_PDF_TEXT_PAGES) {
        if started.elapsed() >= super::READ_TIMEOUT {
            return (Some(count), None);
        }
        let Ok(body) = document.extract_text_with_limit(&[*page], MAX_PDF_STREAM_BYTES) else {
            return (Some(count), None);
        };
        let remaining = MAX_WORK_FILE_TEXT_BYTES.saturating_sub(text.len());
        text.push_str(&truncate(&body, remaining).0);
        if text.len() >= MAX_WORK_FILE_TEXT_BYTES.saturating_sub(4) {
            break;
        }
        text.push('\n');
    }
    if count > MAX_PDF_TEXT_PAGES && text.len() < MAX_WORK_FILE_TEXT_BYTES {
        text.push_str("\n[Text extraction limited to the first 64 pages]");
    }
    (Some(count), Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Document, Object, Stream};

    fn asset(bytes: &[u8]) -> MediaAssetV1 {
        zephium_store::admit_media(
            bytes,
            "fixture",
            zephium_core::resources::MediaOrigin::Imported,
        )
        .unwrap()
    }

    fn fixture(body: &[u8]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages = doc.new_object_id();
        let font = doc.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
        );
        let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
        let content = doc.add_object(Stream::new(dictionary! {}, body.to_vec()));
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "Contents" => content, "Resources" => resources, "MediaBox" => vec![0.into(), 0.into(), 500.into(), 500.into()] });
        doc.objects.insert(
            pages,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
            ),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn work_media_extracts_pdf_text_and_reports_missing_layers() {
        let bytes = fixture(b"BT /F1 12 Tf 10 10 Td (Version 1.2.3) Tj ET");
        let result = text(&asset(&bytes), &bytes);
        assert!(result.contains("1 pages)\nVersion 1.2.3"));
        for bytes in [fixture(b""), b"%PDF-1.7 encrypted or malformed".to_vec()] {
            assert!(text(&asset(&bytes), &bytes).ends_with("no text layer"));
        }
    }

    #[test]
    fn work_media_encrypted_and_compression_bomb_pdfs_have_no_text_layer() {
        let bytes = fixture(b"BT /F1 12 Tf (Private text) Tj ET");
        let mut document = Document::load_mem(&bytes).unwrap();
        document.trailer.set(
            "ID",
            vec![
                Object::string_literal("fixture-id"),
                Object::string_literal("fixture-id"),
            ],
        );
        let state = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V2 {
            document: &document,
            owner_password: "owner",
            user_password: "user",
            key_length: 40,
            permissions: lopdf::Permissions::all(),
        })
        .unwrap();
        document.encrypt(&state).unwrap();
        let mut encrypted = Vec::new();
        document.save_to(&mut encrypted).unwrap();
        let result = text(&asset(&encrypted), &encrypted);
        assert!(result.ends_with("no text layer"));
        assert!(!result.contains("Private text"));

        let bytes = fixture(&vec![b' '; MAX_PDF_STREAM_BYTES + 1]);
        let mut document = Document::load_mem(&bytes).unwrap();
        document.compress();
        let mut compressed = Vec::new();
        document.save_to(&mut compressed).unwrap();
        assert!(text(&asset(&compressed), &compressed).ends_with("no text layer"));
    }

    #[test]
    fn work_media_text_clips_at_character_boundary_and_refuses_binary() {
        let bytes = "界".repeat(MAX_WORK_FILE_TEXT_BYTES).into_bytes();
        let result = text(&asset(&bytes), &bytes);
        assert!(result.len() <= MAX_WORK_FILE_TEXT_BYTES);
        assert!(result.ends_with('界'));
        let bytes = b"hidden\0binary";
        assert!(text(&asset(bytes), bytes).ends_with("no text layer"));
    }
}
