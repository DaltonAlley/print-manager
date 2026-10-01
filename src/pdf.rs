use std::{fs::File, io::Read, path::Path};

use image::{ImageReader, Limits};
use printpdf::{
    FontId, Mm, Op, ParsedFont, PdfDocument, PdfPage, PdfSaveOptions, Point, Pt, RawImage,
    RawImageData, RawImageFormat, TextItem, TextMatrix, XObjectId, XObjectTransform,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    calculation::{
        CalculationError, DocumentTotals, LineCalculationInput, Money, calculate_document,
    },
    domain::DocumentKind,
};

const LIBERATION_SANS: &[u8] = include_bytes!("../assets/fonts/LiberationSans-Regular.ttf");
const PAGE_WIDTH: Mm = Mm(210.0);
const PAGE_HEIGHT: Mm = Mm(297.0);
const TEXT_START_Y: Mm = Mm(282.0);
const TEXT_BOTTOM_Y: Mm = Mm(20.0);
const TEXT_LINE_HEIGHT: Pt = Pt(13.2);
const TEXT_FONT_SIZE: Pt = Pt(10.0);
const CONTENT_LEFT: Mm = Mm(15.0);
const CONTENT_RIGHT: Mm = Mm(195.0);
const TABLE_COLUMN_GAP: Pt = Pt(8.0);
const TABLE_COLUMN_WIDTHS: [Pt; 5] = [Pt(225.0), Pt(65.0), Pt(55.0), Pt(65.0), Pt(68.0)];
const TABLE_HEADERS: [&str; 5] = [
    "Description",
    "Quantity",
    "Unit",
    "Unit price",
    "Line total",
];
const LOGO_TEXT_GAP: Pt = Pt(8.0);
const LOGO_MAX_BYTES: u64 = 2 * 1024 * 1024;
const LOGO_MAX_DIMENSION: u32 = 2_000;
const LOGO_MAX_ALLOC: u64 = 16 * 1024 * 1024;
const LOGO_MAX_POINTS: f32 = 70.0;

struct LogoResource {
    id: XObjectId,
    scale: f32,
    width_points: f32,
    height_points: f32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum LayoutLine {
    Text(String),
    Table(TableLine),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TableLine {
    cells: [String; 5],
    kind: TableLineKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TableLineKind {
    Header,
    Row(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WrappedTableRow {
    index: usize,
    lines: Vec<[String; 5]>,
}

struct DocumentBody {
    before_table: Vec<String>,
    table_rows: Vec<WrappedTableRow>,
    after_table: Vec<String>,
}

const fn document_label(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Quote => "QUOTE",
        DocumentKind::Invoice => "INVOICE",
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdfLine {
    pub description: String,
    pub quantity: Decimal,
    pub unit_label: String,
    pub unit_price: Money,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdfDocumentView {
    pub kind: DocumentKind,
    pub number: String,
    pub status: String,
    pub business_name: String,
    pub business_address: String,
    pub business_phone: String,
    pub business_email: String,
    pub logo_path: Option<String>,
    pub customer_name: String,
    pub customer_email: String,
    pub customer_phone: String,
    pub billing_address: String,
    pub issue_date: String,
    pub expiry_or_due_date: Option<String>,
    pub lines: Vec<PdfLine>,
    pub discount: Money,
    pub tax_basis_points: u32,
    pub notes: String,
    pub terms: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedPdf {
    pub bytes: Vec<u8>,
    pub page_count: usize,
    pub wrapped_line_count: usize,
}

#[derive(Debug, Error)]
pub enum PdfError {
    #[error("document calculation failed: {0}")]
    Calculation(#[from] CalculationError),
    #[error("the embedded Liberation Sans font could not be parsed")]
    InvalidEmbeddedFont,
    #[error(
        "the configured logo could not be read; check that the local file exists and is readable"
    )]
    LogoUnreadable,
    #[error("the configured logo exceeds the 2 MiB file limit")]
    LogoTooLarge,
    #[error("the configured logo must be a valid PNG or JPEG no larger than 2000 × 2000 pixels")]
    LogoInvalid,
}

/// Renders the current quote or invoice view into a valid, paginated PDF.
///
/// # Errors
///
/// Returns an error when document arithmetic fails or the embedded font cannot be parsed.
pub fn render_document(view: &PdfDocumentView) -> Result<RenderedPdf, PdfError> {
    let font = ParsedFont::from_bytes(LIBERATION_SANS, 0, &mut Vec::new())
        .ok_or(PdfError::InvalidEmbeddedFont)?;
    let calculation_lines = view
        .lines
        .iter()
        .map(|line| LineCalculationInput {
            quantity: line.quantity,
            unit_price: line.unit_price,
        })
        .collect::<Vec<_>>();
    let totals = calculate_document(&calculation_lines, view.discount, view.tax_basis_points)?;
    let logo = view
        .logo_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(load_logo)
        .transpose()?;

    let body = document_body(view, &totals, &font);
    let wrapped_line_count = body.before_table.len()
        + 1
        + body
            .table_rows
            .iter()
            .map(|row| row.lines.len())
            .sum::<usize>()
        + body.after_table.len();
    let page_lines = paginate_document(view, &body, &font);
    let page_count = page_lines.len();

    let mut document = PdfDocument::new(&format!("{} {}", document_label(view.kind), view.number));
    let font_id = document.add_font(&font);
    let logo_resource = logo.as_ref().map(|logo| {
        let width = u16::try_from(logo.width).unwrap_or(u16::MAX);
        let height = u16::try_from(logo.height).unwrap_or(u16::MAX);
        let natural_width = f32::from(width) * 72.0 / 300.0;
        let natural_height = f32::from(height) * 72.0 / 300.0;
        let scale = (LOGO_MAX_POINTS / natural_width.max(natural_height)).min(1.0);
        LogoResource {
            id: document.add_image(logo),
            scale,
            width_points: natural_width * scale,
            height_points: natural_height * scale,
        }
    });
    let pages = page_lines
        .iter()
        .enumerate()
        .map(|(index, lines)| {
            render_page(
                &font_id,
                (index == 0).then_some(logo_resource.as_ref()).flatten(),
                lines,
                index + 1,
                page_count,
            )
        })
        .collect();
    document.with_pages(pages);
    let bytes = document.save(&PdfSaveOptions::default(), &mut Vec::new());

    Ok(RenderedPdf {
        bytes,
        page_count,
        wrapped_line_count,
    })
}

#[allow(clippy::too_many_lines)]
fn document_body(
    view: &PdfDocumentView,
    totals: &DocumentTotals,
    font: &ParsedFont,
) -> DocumentBody {
    let mut before_table = Vec::new();
    push_prefixed(
        &mut before_table,
        "Status: ",
        &view.status,
        font,
        content_width(),
    );
    push_prefixed(
        &mut before_table,
        "Issue date: ",
        &view.issue_date,
        font,
        content_width(),
    );
    if let Some(date) = &view.expiry_or_due_date {
        let label = match view.kind {
            DocumentKind::Quote => "Expires",
            DocumentKind::Invoice => "Due",
        };
        push_prefixed(
            &mut before_table,
            &format!("{label}: "),
            date,
            font,
            content_width(),
        );
    }
    if view.kind == DocumentKind::Invoice && view.status == "paid" {
        before_table.push("*** PAID ***".to_owned());
    }
    before_table.push(String::new());
    push_prefixed(
        &mut before_table,
        "Bill to: ",
        &view.customer_name,
        font,
        content_width(),
    );
    push_prefixed(
        &mut before_table,
        "Customer email: ",
        &view.customer_email,
        font,
        content_width(),
    );
    push_prefixed(
        &mut before_table,
        "Customer phone: ",
        &view.customer_phone,
        font,
        content_width(),
    );
    push_prefixed(
        &mut before_table,
        "Billing address: ",
        &view.billing_address,
        font,
        content_width(),
    );
    before_table.extend([String::new(), "LINE ITEMS".to_owned()]);

    let table_rows = view
        .lines
        .iter()
        .zip(&totals.line_totals)
        .enumerate()
        .map(|(index, (line, line_total))| {
            wrap_table_row(
                index,
                &[
                    line.description.clone(),
                    line.quantity.normalize().to_string(),
                    line.unit_label.clone(),
                    format_money(line.unit_price),
                    format_money(*line_total),
                ],
                font,
            )
        })
        .collect();

    let mut after_table = vec![
        String::new(),
        format!("Subtotal: {}", format_money(totals.subtotal)),
        format!("Discount: -{}", format_money(totals.discount)),
        format!(
            "Tax ({} bp): {}",
            view.tax_basis_points,
            format_money(totals.tax)
        ),
        format!("TOTAL: {}", format_money(totals.total)),
        String::new(),
        "Notes".to_owned(),
    ];
    after_table.extend(wrap_text(&view.notes, font, content_width()));
    after_table.push("Terms".to_owned());
    after_table.extend(wrap_text(&view.terms, font, content_width()));

    DocumentBody {
        before_table,
        table_rows,
        after_table,
    }
}

fn header_lines(view: &PdfDocumentView, font: &ParsedFont) -> Vec<String> {
    let mut lines = Vec::new();
    let width = if view.logo_path.is_some() {
        content_width() - LOGO_MAX_POINTS - LOGO_TEXT_GAP.0
    } else {
        content_width()
    };
    push_prefixed(&mut lines, "", &view.business_name, font, width);
    push_prefixed(&mut lines, "", &view.business_address, font, width);
    push_prefixed(&mut lines, "Phone: ", &view.business_phone, font, width);
    push_prefixed(&mut lines, "Email: ", &view.business_email, font, width);
    push_prefixed(
        &mut lines,
        "",
        &format!("{} #{}", document_label(view.kind), view.number),
        font,
        content_width(),
    );
    lines.push(String::new());
    lines
}

fn continuation_lines(view: &PdfDocumentView, font: &ParsedFont) -> Vec<String> {
    let mut lines = Vec::new();
    push_prefixed(
        &mut lines,
        "",
        &format!("{} #{} — continued", document_label(view.kind), view.number),
        font,
        content_width(),
    );
    lines.push(String::new());
    lines
}

fn page_line_capacity() -> usize {
    let mut baseline = TEXT_START_Y.0;
    let line_height_mm = TEXT_LINE_HEIGHT.0 * 25.4 / 72.0;
    let mut capacity = 0;
    while baseline >= TEXT_BOTTOM_Y.0 {
        capacity += 1;
        baseline -= line_height_mm;
    }
    capacity
}

fn paginate_document(
    view: &PdfDocumentView,
    body: &DocumentBody,
    font: &ParsedFont,
) -> Vec<Vec<LayoutLine>> {
    let capacity = page_line_capacity();
    let lead = header_lines(view, font);
    let continuation = continuation_lines(view, font);
    debug_assert!(continuation.len() < capacity);

    let mut pages = vec![Vec::new()];
    append_text_lines(&mut pages, &lead, &continuation, capacity);
    append_text_lines(&mut pages, &body.before_table, &continuation, capacity);
    append_table_rows(&mut pages, &body.table_rows, &continuation, capacity);
    append_text_lines(&mut pages, &body.after_table, &continuation, capacity);
    pages
}

fn append_text_lines(
    pages: &mut Vec<Vec<LayoutLine>>,
    lines: &[String],
    continuation: &[String],
    capacity: usize,
) {
    for line in lines {
        if pages.last().is_none_or(|page| page.len() == capacity) {
            push_continuation_page(pages, continuation);
        }
        if let Some(page) = pages.last_mut() {
            page.push(LayoutLine::Text(line.clone()));
        }
    }
}

fn append_table_rows(
    pages: &mut Vec<Vec<LayoutLine>>,
    rows: &[WrappedTableRow],
    continuation: &[String],
    capacity: usize,
) {
    let Some(first_row) = rows.first() else {
        return;
    };
    let maximum_row_lines = capacity.saturating_sub(continuation.len() + 1);
    let first_required = first_row.lines.len().min(maximum_row_lines) + 1;
    if remaining_capacity(pages, capacity) < first_required {
        push_continuation_page(pages, continuation);
    }
    push_table_header(pages);

    for row in rows {
        if row.lines.len() <= maximum_row_lines {
            if remaining_capacity(pages, capacity) < row.lines.len() {
                push_continuation_page(pages, continuation);
                push_table_header(pages);
            }
            push_table_row_lines(pages, row, 0, row.lines.len());
            continue;
        }

        if remaining_capacity(pages, capacity) < maximum_row_lines {
            push_continuation_page(pages, continuation);
            push_table_header(pages);
        }
        let mut offset = 0;
        while offset < row.lines.len() {
            let count = remaining_capacity(pages, capacity).min(row.lines.len() - offset);
            push_table_row_lines(pages, row, offset, count);
            offset += count;
            if offset < row.lines.len() {
                push_continuation_page(pages, continuation);
                push_table_header(pages);
            }
        }
    }
}

fn push_continuation_page(pages: &mut Vec<Vec<LayoutLine>>, continuation: &[String]) {
    pages.push(continuation.iter().cloned().map(LayoutLine::Text).collect());
}

fn push_table_header(pages: &mut [Vec<LayoutLine>]) {
    if let Some(page) = pages.last_mut() {
        page.push(LayoutLine::Table(TableLine {
            cells: TABLE_HEADERS.map(str::to_owned),
            kind: TableLineKind::Header,
        }));
    }
}

fn push_table_row_lines(
    pages: &mut [Vec<LayoutLine>],
    row: &WrappedTableRow,
    offset: usize,
    count: usize,
) {
    if let Some(page) = pages.last_mut() {
        page.extend(
            row.lines[offset..offset + count]
                .iter()
                .cloned()
                .map(|cells| {
                    LayoutLine::Table(TableLine {
                        cells,
                        kind: TableLineKind::Row(row.index),
                    })
                }),
        );
    }
}

fn remaining_capacity(pages: &[Vec<LayoutLine>], capacity: usize) -> usize {
    pages
        .last()
        .map_or(capacity, |page| capacity.saturating_sub(page.len()))
}

fn content_width() -> f32 {
    CONTENT_RIGHT.into_pt().0 - CONTENT_LEFT.into_pt().0
}

fn push_prefixed(
    lines: &mut Vec<String>,
    prefix: &str,
    value: &str,
    font: &ParsedFont,
    width: f32,
) {
    let indentation = text_width(prefix, font, TEXT_FONT_SIZE);
    let available = (width - indentation).max(widest_glyph_width(font, TEXT_FONT_SIZE));
    let wrapped = wrap_text(value, font, available);
    if wrapped.is_empty() {
        lines.push(prefix.trim_end().to_owned());
        return;
    }
    for (index, line) in wrapped.into_iter().enumerate() {
        if index == 0 {
            lines.push(format!("{prefix}{line}"));
        } else {
            lines.push(format!("{}{line}", " ".repeat(prefix.chars().count())));
        }
    }
}

fn wrap_table_row(index: usize, values: &[String; 5], font: &ParsedFont) -> WrappedTableRow {
    let wrapped = std::array::from_fn::<_, 5, _>(|column| {
        let mut lines = wrap_text(&values[column], font, TABLE_COLUMN_WIDTHS[column].0);
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    });
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
    let lines = (0..height)
        .map(|line| {
            std::array::from_fn(|column| wrapped[column].get(line).cloned().unwrap_or_default())
        })
        .collect();
    WrappedTableRow { index, lines }
}

fn render_page(
    font_id: &FontId,
    logo: Option<&LogoResource>,
    lines: &[LayoutLine],
    page_number: usize,
    page_count: usize,
) -> PdfPage {
    let mut ops = Vec::new();
    if let Some(logo) = logo {
        ops.push(Op::UseXobject {
            id: logo.id.clone(),
            transform: XObjectTransform {
                translate_x: Some(Pt(552.0 - logo.width_points)),
                translate_y: Some(Pt(799.0 - logo.height_points)),
                scale_x: Some(logo.scale),
                scale_y: Some(logo.scale),
                dpi: Some(300.0),
                rotate: None,
            },
        });
    }
    ops.extend([
        Op::StartTextSection,
        Op::SetFontSize {
            size: TEXT_FONT_SIZE,
            font: font_id.clone(),
        },
        Op::SetLineHeight {
            lh: TEXT_LINE_HEIGHT,
        },
    ]);
    let mut baseline = TEXT_START_Y.into_pt().0;
    for line in lines {
        match line {
            LayoutLine::Text(text) => {
                write_text_at(&mut ops, font_id, CONTENT_LEFT.into_pt().0, baseline, text);
            }
            LayoutLine::Table(table) => {
                let mut x = CONTENT_LEFT.into_pt().0;
                for (column, text) in table.cells.iter().enumerate() {
                    write_text_at(&mut ops, font_id, x, baseline, text);
                    x += TABLE_COLUMN_WIDTHS[column].0 + TABLE_COLUMN_GAP.0;
                }
            }
        }
        baseline -= TEXT_LINE_HEIGHT.0;
    }
    ops.push(Op::EndTextSection);
    ops.extend([
        Op::StartTextSection,
        Op::SetTextCursor {
            pos: Point::new(Mm(15.0), Mm(10.0)),
        },
        Op::SetFontSize {
            size: Pt(9.0),
            font: font_id.clone(),
        },
        Op::WriteText {
            items: vec![TextItem::Text(format!(
                "Page {page_number} of {page_count}"
            ))],
            font: font_id.clone(),
        },
        Op::EndTextSection,
    ]);
    PdfPage::new(PAGE_WIDTH, PAGE_HEIGHT, ops)
}

fn write_text_at(ops: &mut Vec<Op>, font_id: &FontId, x: f32, y: f32, text: &str) {
    ops.push(Op::SetTextMatrix {
        matrix: TextMatrix::Translate(Pt(x), Pt(y)),
    });
    ops.push(Op::WriteText {
        items: vec![TextItem::Text(text.to_owned())],
        font: font_id.clone(),
    });
}

fn load_logo(path: &str) -> Result<RawImage, PdfError> {
    let file = File::open(Path::new(path)).map_err(|_| PdfError::LogoUnreadable)?;
    let metadata = file.metadata().map_err(|_| PdfError::LogoUnreadable)?;
    if metadata.len() > LOGO_MAX_BYTES {
        return Err(PdfError::LogoTooLarge);
    }
    let mut bytes = Vec::new();
    file.take(LOGO_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PdfError::LogoUnreadable)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > LOGO_MAX_BYTES {
        return Err(PdfError::LogoTooLarge);
    }
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| PdfError::LogoInvalid)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(LOGO_MAX_DIMENSION);
    limits.max_image_height = Some(LOGO_MAX_DIMENSION);
    limits.max_alloc = Some(LOGO_MAX_ALLOC);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| PdfError::LogoInvalid)?;
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(RawImage {
        pixels: RawImageData::U8(rgba.into_raw()),
        width: usize::try_from(width).map_err(|_| PdfError::LogoInvalid)?,
        height: usize::try_from(height).map_err(|_| PdfError::LogoInvalid)?,
        data_format: RawImageFormat::RGBA8,
        tag: Vec::new(),
    })
}

fn glyph_width(character: char, font: &ParsedFont, font_size: Pt) -> f32 {
    let advance = font
        .lookup_glyph_index(character as u32)
        .map_or(font.font_metrics.advance_width_max, |glyph| {
            font.get_horizontal_advance(glyph)
        });
    f32::from(advance) / f32::from(font.font_metrics.units_per_em) * font_size.0
}

fn widest_glyph_width(font: &ParsedFont, font_size: Pt) -> f32 {
    f32::from(font.font_metrics.advance_width_max) / f32::from(font.font_metrics.units_per_em)
        * font_size.0
}

fn text_width(text: &str, font: &ParsedFont, font_size: Pt) -> f32 {
    text.chars()
        .map(|character| glyph_width(character, font, font_size))
        .sum()
}

fn split_word(word: &str, font: &ParsedFont, max_width: f32) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    let mut width = 0.0;
    for character in word.chars() {
        let character_width = glyph_width(character, font, TEXT_FONT_SIZE);
        if !chunk.is_empty() && width + character_width > max_width {
            chunks.push(std::mem::take(&mut chunk));
            width = 0.0;
        }
        chunk.push(character);
        width += character_width;
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

fn wrap_text(text: &str, font: &ParsedFont, max_width: f32) -> Vec<String> {
    let mut output = Vec::new();
    let space_width = glyph_width(' ', font, TEXT_FONT_SIZE);
    for paragraph in text.lines() {
        let mut current = String::new();
        let mut current_width = 0.0;
        for word in paragraph.split_whitespace() {
            let word_width = text_width(word, font, TEXT_FONT_SIZE);
            if word_width > max_width {
                if !current.is_empty() {
                    output.push(std::mem::take(&mut current));
                    current_width = 0.0;
                }
                output.extend(split_word(word, font, max_width));
            } else if current.is_empty() {
                current.push_str(word);
                current_width = word_width;
            } else if current_width + space_width + word_width <= max_width {
                current.push(' ');
                current.push_str(word);
                current_width += space_width + word_width;
            } else {
                output.push(std::mem::take(&mut current));
                current.push_str(word);
                current_width = word_width;
            }
        }
        if !current.is_empty() {
            output.push(current);
        } else if paragraph.is_empty() {
            output.push(String::new());
        }
    }
    output
}

fn format_money(money: Money) -> String {
    format!("${}.{:02}", money.cents() / 100, money.cents() % 100)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        str::FromStr,
    };

    use image::{ImageFormat, RgbaImage};
    use lopdf::Document;
    use printpdf::PdfParseOptions;

    use super::*;

    fn money(cents: i64) -> Money {
        Money::from_cents(cents).unwrap_or_else(|error| panic!("{error}"))
    }

    fn fixture(kind: DocumentKind, line_count: usize) -> PdfDocumentView {
        let lines = (0..line_count)
            .map(|index| PdfLine {
                description: format!(
                    "Line {index}: premium archival exhibition poster with a deliberately long description that must wrap cleanly within the description column"
                ),
                quantity: Decimal::from_str("1.25").unwrap_or_else(|error| panic!("{error}")),
                unit_label: "each".to_owned(),
                unit_price: money(2_499),
            })
            .collect();
        PdfDocumentView {
            kind,
            number: match kind {
                DocumentKind::Quote => "Q-1001",
                DocumentKind::Invoice => "INV-1001",
            }
            .to_owned(),
            status: if kind == DocumentKind::Invoice {
                "paid"
            } else {
                "draft"
            }
            .to_owned(),
            business_name: "Example Print Shop".to_owned(),
            business_address: "123 Press Lane, Richmond, VA".to_owned(),
            business_phone: "555-0100".to_owned(),
            business_email: "hello@example.test".to_owned(),
            logo_path: None,
            customer_name: "Museum of Very Long Names".to_owned(),
            customer_email: "curator@museum.example".to_owned(),
            customer_phone: "+1 555 0199".to_owned(),
            billing_address: "45 Gallery Avenue, Floor 12, Washington, DC".to_owned(),
            issue_date: "2026-08-20".to_owned(),
            expiry_or_due_date: Some("2026-09-20".to_owned()),
            lines,
            discount: money(750),
            tax_basis_points: 600,
            notes: "Colors must match the approved proof. Package flat and protect every corner."
                .to_owned(),
            terms: "Payment is due according to the date above.".to_owned(),
        }
    }

    fn extracted_text(bytes: &[u8]) -> String {
        let parsed = PdfDocument::parse(bytes, &PdfParseOptions::default(), &mut Vec::new())
            .unwrap_or_else(|error| panic!("{error}"));
        parsed
            .pages
            .iter()
            .flat_map(|page| &page.ops)
            .filter_map(|op| match op {
                Op::WriteText { items, .. } => Some(items),
                _ => None,
            })
            .flatten()
            .filter_map(|item| match item {
                TextItem::Text(text) => Some(text.as_str()),
                TextItem::Offset(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn representative_quote_is_valid_and_wraps_descriptions() {
        let rendered = render_document(&fixture(DocumentKind::Quote, 2))
            .unwrap_or_else(|error| panic!("{error}"));
        let parsed = Document::load_mem(&rendered.bytes).unwrap_or_else(|error| panic!("{error}"));
        let operations = PdfDocument::parse(
            &rendered.bytes,
            &PdfParseOptions::default(),
            &mut Vec::new(),
        )
        .unwrap_or_else(|error| panic!("{error}"));

        assert!(rendered.bytes.starts_with(b"%PDF-"));
        assert_eq!(parsed.get_pages().len(), rendered.page_count);
        assert!(rendered.wrapped_line_count > 20);
        assert!(String::from_utf8_lossy(&rendered.bytes).contains("LiberationSans"));
        assert!(extracted_text(&rendered.bytes).contains("QUOTE #Q-1001"));
        assert!(
            operations
                .pages
                .iter()
                .flat_map(|page| &page.ops)
                .any(|op| matches!(op, Op::SetTextMatrix { .. }))
        );
    }

    #[test]
    fn representative_paid_invoice_paginates() {
        let rendered = render_document(&fixture(DocumentKind::Invoice, 55))
            .unwrap_or_else(|error| panic!("{error}"));
        let parsed = Document::load_mem(&rendered.bytes).unwrap_or_else(|error| panic!("{error}"));

        assert!(rendered.page_count >= 3);
        assert_eq!(parsed.get_pages().len(), rendered.page_count);
        assert!(rendered.wrapped_line_count > 100);
        let text = extracted_text(&rendered.bytes);
        assert!(text.contains("Page 1 of"));
        assert!(text.contains("continued"));
        assert!(text.contains("Line 54:"));
    }

    #[test]
    fn fixed_table_columns_are_font_bounded_and_written_at_distinct_origins() {
        let view = fixture(DocumentKind::Quote, 2);
        let font = ParsedFont::from_bytes(LIBERATION_SANS, 0, &mut Vec::new())
            .unwrap_or_else(|| panic!("embedded font should parse"));
        let calculation_lines = view
            .lines
            .iter()
            .map(|line| LineCalculationInput {
                quantity: line.quantity,
                unit_price: line.unit_price,
            })
            .collect::<Vec<_>>();
        let totals = calculate_document(&calculation_lines, view.discount, view.tax_basis_points)
            .unwrap_or_else(|error| panic!("{error}"));
        let body = document_body(&view, &totals, &font);
        for row in &body.table_rows {
            for line in &row.lines {
                for (column, text) in line.iter().enumerate() {
                    assert!(
                        text_width(text, &font, TEXT_FONT_SIZE)
                            <= TABLE_COLUMN_WIDTHS[column].0 + 0.01,
                        "column {column} overflowed with {text:?}"
                    );
                }
            }
        }

        let rendered = render_document(&view).unwrap_or_else(|error| panic!("{error}"));
        let parsed = PdfDocument::parse(
            &rendered.bytes,
            &PdfParseOptions::default(),
            &mut Vec::new(),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let mut origins = BTreeMap::new();
        for page in &parsed.pages {
            let mut cursor_x = None;
            for operation in &page.ops {
                match operation {
                    Op::SetTextCursor { pos } => cursor_x = Some(pos.x.0),
                    Op::SetTextMatrix { matrix } => cursor_x = Some(matrix.as_array()[4]),
                    Op::WriteText { items, .. } => {
                        for item in items {
                            if let TextItem::Text(text) = item
                                && TABLE_HEADERS.contains(&text.as_str())
                            {
                                origins.entry(text.clone()).or_insert(cursor_x);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut expected_x = CONTENT_LEFT.into_pt().0;
        for (column, header) in TABLE_HEADERS.iter().enumerate() {
            let actual = origins
                .get(*header)
                .and_then(|origin| *origin)
                .unwrap_or_else(|| panic!("missing {header} table operation"));
            assert!((actual - expected_x).abs() < 0.01, "wrong {header} origin");
            expected_x += TABLE_COLUMN_WIDTHS[column].0 + TABLE_COLUMN_GAP.0;
        }
        assert!(
            expected_x - TABLE_COLUMN_GAP.0 <= CONTENT_RIGHT.into_pt().0 + 0.01,
            "fixed table exceeds the content boundary"
        );
    }

    #[test]
    fn continuation_pages_repeat_headers_without_splitting_ordinary_rows() {
        let view = fixture(DocumentKind::Invoice, 55);
        let font = ParsedFont::from_bytes(LIBERATION_SANS, 0, &mut Vec::new())
            .unwrap_or_else(|| panic!("embedded font should parse"));
        let calculation_lines = view
            .lines
            .iter()
            .map(|line| LineCalculationInput {
                quantity: line.quantity,
                unit_price: line.unit_price,
            })
            .collect::<Vec<_>>();
        let totals = calculate_document(&calculation_lines, view.discount, view.tax_basis_points)
            .unwrap_or_else(|error| panic!("{error}"));
        let body = document_body(&view, &totals, &font);
        let pages = paginate_document(&view, &body, &font);
        let mut row_pages: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut table_page_count = 0;
        for (page_index, page) in pages.iter().enumerate() {
            let header_index = page.iter().position(|line| {
                matches!(
                    line,
                    LayoutLine::Table(TableLine {
                        kind: TableLineKind::Header,
                        ..
                    })
                )
            });
            let row_index = page.iter().position(|line| {
                matches!(
                    line,
                    LayoutLine::Table(TableLine {
                        kind: TableLineKind::Row(_),
                        ..
                    })
                )
            });
            if let Some(row_index) = row_index {
                table_page_count += 1;
                assert!(
                    header_index.is_some_and(|header_index| header_index < row_index),
                    "table page {page_index} did not repeat its header before data"
                );
            }
            for line in page {
                if let LayoutLine::Table(TableLine {
                    kind: TableLineKind::Row(index),
                    ..
                }) = line
                {
                    row_pages.entry(*index).or_default().insert(page_index);
                }
            }
        }
        assert!(table_page_count >= 2);
        assert_eq!(row_pages.len(), 55);
        assert!(
            row_pages.values().all(|pages| pages.len() == 1),
            "an ordinary wrapped row crossed a page boundary"
        );
    }

    #[test]
    fn customer_details_max_fields_wrap_and_logo_is_embedded() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let logo_path = directory.path().join("logo.png");
        RgbaImage::from_pixel(32, 24, image::Rgba([20, 80, 160, 255]))
            .save_with_format(&logo_path, ImageFormat::Png)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut view = fixture(DocumentKind::Quote, 1);
        view.logo_path = Some(logo_path.to_string_lossy().into_owned());
        view.business_name = "B".repeat(200);
        view.customer_name = "C".repeat(200);
        view.customer_email = "customer+press@example.test".into();
        view.customer_phone = "+1 (555) 010-9090 extension 123".into();
        view.billing_address = "A".repeat(2_000);
        view.lines[0].description = "D".repeat(500);
        view.lines[0].unit_label = "U".repeat(50);

        let rendered = render_document(&view).unwrap_or_else(|error| panic!("{error}"));
        let parsed = Document::load_mem(&rendered.bytes).unwrap_or_else(|error| panic!("{error}"));
        let text = extracted_text(&rendered.bytes);
        assert!(text.contains("customer+press@example.test"));
        assert!(text.contains("+1 (555) 010-9090 extension 123"));
        for header in TABLE_HEADERS {
            assert!(text.contains(header));
        }
        assert!(text.contains("1.25"));
        assert!(text.contains("$24.99"));
        assert!(rendered.page_count > 1);
        assert!(parsed.objects.values().any(|object| {
            object
                .as_stream()
                .ok()
                .and_then(|stream| stream.dict.get(b"Subtype").ok())
                .and_then(|value| value.as_name().ok())
                == Some(b"Image".as_slice())
        }));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn maximum_valid_fields_keep_every_operation_inside_a4_margins() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let logo_path = directory.path().join("logo.png");
        RgbaImage::from_pixel(2_000, 20, image::Rgba([20, 80, 160, 255]))
            .save_with_format(&logo_path, ImageFormat::Png)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut view = fixture(DocumentKind::Invoice, 50);
        view.logo_path = Some(logo_path.to_string_lossy().into_owned());
        let font = ParsedFont::from_bytes(LIBERATION_SANS, 0, &mut Vec::new())
            .unwrap_or_else(|| panic!("embedded font should parse"));
        let widest = (0..=char::MAX as u32)
            .filter_map(char::from_u32)
            .filter_map(|character| {
                let glyph = font.lookup_glyph_index(character as u32)?;
                Some((font.get_horizontal_advance(glyph), character))
            })
            .max_by_key(|(advance, _)| *advance)
            .map_or('W', |(_, character)| character);
        view.business_name = widest.to_string().repeat(200);
        view.business_address = "A\n".repeat(1_000);
        view.business_phone = "P".repeat(100);
        view.business_email = "E".repeat(320);
        view.customer_name = widest.to_string().repeat(200);
        view.customer_email = "M".repeat(320);
        view.customer_phone = "T".repeat(100);
        view.billing_address = "D".repeat(2_000);
        for line in &mut view.lines {
            line.description = widest.to_string().repeat(500);
            line.unit_label = "U".repeat(50);
        }
        view.notes = "N".repeat(5_000);
        view.terms = "R".repeat(5_000);

        let rendered = render_document(&view).unwrap_or_else(|error| panic!("{error}"));
        let parsed = PdfDocument::parse(
            &rendered.bytes,
            &PdfParseOptions::default(),
            &mut Vec::new(),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(parsed.pages.len(), rendered.page_count);
        assert!(rendered.page_count > 20);

        let minimum_x = Mm(10.0).into_pt().0;
        let maximum_x = Mm(200.0).into_pt().0;
        let minimum_y = Mm(10.0).into_pt().0;
        let maximum_y = Mm(287.0).into_pt().0;
        for page in &parsed.pages {
            let mut cursor_y = None;
            let mut cursor_x = None;
            let mut font_size = 0.0;
            let mut content_baselines = BTreeSet::new();
            let mut transformation = None;
            for operation in &page.ops {
                match operation {
                    Op::SetTextCursor { pos } => {
                        assert!((minimum_x..=maximum_x).contains(&pos.x.0));
                        assert!((minimum_y..=maximum_y).contains(&pos.y.0));
                        cursor_y = Some(pos.y.0);
                        cursor_x = Some(pos.x.0);
                    }
                    Op::SetTextMatrix { matrix } => {
                        let values = matrix.as_array();
                        let x = values[4];
                        let y = values[5];
                        assert!((minimum_x..=maximum_x).contains(&x));
                        assert!((minimum_y..=maximum_y).contains(&y));
                        cursor_y = Some(y);
                        cursor_x = Some(x);
                    }
                    Op::SetFontSize { size, .. } => font_size = size.0,
                    Op::WriteText { items, .. } => {
                        let baseline = cursor_y.unwrap_or_else(|| panic!("text without a cursor"));
                        let origin = cursor_x.unwrap_or_else(|| panic!("text without a cursor"));
                        assert!((minimum_y..=maximum_y).contains(&baseline));
                        if baseline > minimum_y {
                            content_baselines.insert(baseline.to_bits());
                        }
                        let extent = items.iter().fold(origin, |position, item| match item {
                            TextItem::Text(text) => {
                                position + text_width(text, &font, Pt(font_size))
                            }
                            TextItem::Offset(offset) => position + offset * font_size / 1_000.0,
                        });
                        assert!(
                            extent <= maximum_x + 0.01,
                            "rendered text extends to {extent}pt beyond {maximum_x}pt"
                        );
                    }
                    Op::SetTransformationMatrix { matrix } => {
                        transformation = Some(matrix.as_array());
                    }
                    Op::UseXobject { .. } => {
                        let [x_scale, y_skew, x_skew, y_scale, x_offset, y_offset] = transformation
                            .take()
                            .unwrap_or_else(|| panic!("logo without a transformation"));
                        for (x, y) in [
                            (x_offset, y_offset),
                            (x_scale + x_offset, y_skew + y_offset),
                            (x_skew + x_offset, y_scale + y_offset),
                            (x_scale + x_skew + x_offset, y_skew + y_scale + y_offset),
                        ] {
                            assert!((Mm(15.0).into_pt().0..=Mm(195.0).into_pt().0).contains(&x));
                            assert!((Mm(15.0).into_pt().0..=Mm(282.0).into_pt().0).contains(&y));
                        }
                    }
                    _ => {}
                }
            }
            assert!(content_baselines.len() <= page_line_capacity());
        }

        let text = extracted_text(&rendered.bytes);
        assert!(text.matches("Description").count() >= 2);
        assert_eq!(text.matches("$24.99").count(), 50);
        assert_eq!(text.matches("$31.24").count(), 50);
        assert!(text.contains("TOTAL: $"));
        assert!(text.contains("*** PAID ***"));
        assert!(text.contains("continued"));
        assert!(text.chars().filter(|character| *character == 'N').count() >= 5_000);
        assert!(text.chars().filter(|character| *character == 'R').count() >= 5_000);
    }

    #[test]
    fn configured_logo_limits_fail_without_disclosing_the_path() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let logo_path = directory.path().join("secret-customer-logo.png");
        std::fs::write(&logo_path, b"not an image").unwrap_or_else(|error| panic!("{error}"));
        let mut view = fixture(DocumentKind::Quote, 1);
        view.logo_path = Some(logo_path.to_string_lossy().into_owned());
        let error = match render_document(&view) {
            Ok(_) => panic!("invalid configured logo should fail"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("valid PNG or JPEG"));
        assert!(!error.contains("secret-customer-logo"));
    }
}
