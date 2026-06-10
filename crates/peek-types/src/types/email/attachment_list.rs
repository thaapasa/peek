//! `AttachmentListSource`: the email-attachments [`ListSource`]. A flat
//! listing whose payload isn't file-shaped — instead of perms/mtime it
//! shows each part's **content type**, the first non-file column to ride
//! the generic listing engine. Rows extract through the standard `e`
//! pipeline (`email::extract`), keyed by the attachment's stable name.

use crate::theme::PeekTheme;
use crate::viewer::listing::row::{self, SizeCell};
use crate::viewer::listing::{ListSource, ListingHelp, NameCell, RowCells};
use crate::viewer::modes::{ExtractTarget, RenderCtx};

use super::message::ParsedEmail;

/// Upper bound on the content-type column so a pathological MIME type can't
/// crowd out the name. Real types sit well under this.
const CT_MAX_WIDTH: usize = 32;

struct AttachmentRow {
    key: String,
    size: u64,
    content_type: String,
}

pub struct AttachmentListSource {
    rows: Vec<AttachmentRow>,
    /// Width of the content-type column: the widest type, capped.
    ct_width: usize,
}

impl AttachmentListSource {
    pub fn new(email: &ParsedEmail) -> Self {
        let rows: Vec<AttachmentRow> = email
            .attachments
            .iter()
            .map(|a| AttachmentRow {
                key: a.key.clone(),
                size: a.size,
                content_type: a.content_type.clone(),
            })
            .collect();
        let ct_width = rows
            .iter()
            .map(|r| r.content_type.len())
            .max()
            .unwrap_or(0)
            .min(CT_MAX_WIDTH);
        Self { rows, ct_width }
    }

    fn content_type_cell(&self, idx: usize, theme: &PeekTheme) -> String {
        let ct = &self.rows[idx].content_type;
        theme.paint(&format!("{ct:<w$}", w = self.ct_width), theme.value)
    }

    fn size_cell(&self, idx: usize, theme: &PeekTheme) -> String {
        let size = self.rows[idx].size;
        row::paint_size(&row::format_size(SizeCell::Bytes(size)), size, false, theme)
    }
}

impl ListSource for AttachmentListSource {
    fn len(&self) -> usize {
        self.rows.len()
    }

    fn parent(&self, _idx: usize) -> Option<usize> {
        None
    }

    fn selectable(&self, _idx: usize) -> bool {
        true
    }

    fn name(&self, idx: usize) -> &str {
        &self.rows[idx].key
    }

    fn source_label(&self) -> &str {
        "Email"
    }

    fn extract_target(&self, idx: usize) -> Option<ExtractTarget> {
        Some(ExtractTarget::EntryPath(self.rows[idx].key.clone()))
    }

    /// Flat — no parents, so no sticky toggle to advertise.
    fn help(&self) -> ListingHelp {
        ListingHelp {
            sticky: false,
            ..Default::default()
        }
    }

    fn row_cells(&self, idx: usize, ctx: &RenderCtx) -> RowCells {
        let theme = ctx.peek_theme;
        RowCells {
            prefix: String::new(),
            left: vec![
                self.content_type_cell(idx, theme),
                self.size_cell(idx, theme),
            ],
            name: NameCell {
                text: self.rows[idx].key.clone(),
                is_dir: false,
            },
        }
    }

    fn flat_line(&self, idx: usize, theme: &PeekTheme) -> Option<String> {
        let name = theme.paint(&self.rows[idx].key, theme.foreground);
        Some(format!(
            "{}  {}  {}",
            self.content_type_cell(idx, theme),
            self.size_cell(idx, theme),
            name
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::email::message::{Attachment, Body, ParsedEmail};

    fn email(attachments: Vec<Attachment>) -> ParsedEmail {
        ParsedEmail {
            from: None,
            to: None,
            cc: None,
            subject: None,
            date: None,
            message_id: None,
            body: Body::Empty,
            attachments,
        }
    }

    fn attachment(key: &str, size: u64, ct: &str) -> Attachment {
        Attachment {
            key: key.to_string(),
            size,
            content_type: ct.to_string(),
        }
    }

    #[test]
    fn rows_are_flat_and_selectable() {
        let src = AttachmentListSource::new(&email(vec![
            attachment("a.pdf", 10, "application/pdf"),
            attachment("b.png", 20, "image/png"),
        ]));
        assert_eq!(src.len(), 2);
        assert!((0..src.len()).all(|i| src.selectable(i) && src.parent(i).is_none()));
        assert_eq!(src.name(0), "a.pdf");
    }

    #[test]
    fn content_type_column_sized_to_widest_capped() {
        let src = AttachmentListSource::new(&email(vec![
            attachment("a", 1, "image/png"),
            attachment("b", 1, "application/vnd.oasis.opendocument.text"),
        ]));
        assert_eq!(src.ct_width, CT_MAX_WIDTH);
    }

    #[test]
    fn extract_key_is_attachment_name() {
        let src = AttachmentListSource::new(&email(vec![attachment("notes.txt", 1, "text/plain")]));
        match src.extract_target(0) {
            Some(ExtractTarget::EntryPath(p)) => assert_eq!(p, "notes.txt"),
            other => panic!("expected EntryPath, got {other:?}"),
        }
    }
}
