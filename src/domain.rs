#![allow(clippy::missing_errors_doc)]

use std::str::FromStr;

use rust_decimal::Decimal;

use crate::calculation::{LineCalculationInput, Money, calculate_document};

pub const MAX_LINES: usize = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobStage {
    Design,
    Waiting,
    Print,
    Finish,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("unknown production stage")]
pub struct JobStageParseError;

impl JobStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Design => "design",
            Self::Waiting => "waiting",
            Self::Print => "print",
            Self::Finish => "finish",
            Self::Complete => "complete",
        }
    }
}

impl FromStr for JobStage {
    type Err = JobStageParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "design" => Ok(Self::Design),
            "waiting" => Ok(Self::Waiting),
            "print" => Ok(Self::Print),
            "finish" => Ok(Self::Finish),
            "complete" => Ok(Self::Complete),
            _ => Err(JobStageParseError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoardLane {
    Quote,
    Production(JobStage),
}

impl FromStr for BoardLane {
    type Err = JobStageParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "quote" {
            Ok(Self::Quote)
        } else {
            value.parse().map(Self::Production)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum DocumentKind {
    Quote,
    Invoice,
}

/// File identity recorded independently of either persistence or storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttachmentIntegrity {
    pub(crate) stored_name: String,
    pub(crate) byte_size: i64,
    pub(crate) sha256: String,
}

impl DocumentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quote => "quote",
            Self::Invoice => "invoice",
        }
    }
    pub const fn plural(self) -> &'static str {
        match self {
            Self::Quote => "quotes",
            Self::Invoice => "invoices",
        }
    }
    pub const fn date_label(self) -> &'static str {
        match self {
            Self::Quote => "Expiry date",
            Self::Invoice => "Due date",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineInput {
    pub description: String,
    pub quantity: String,
    pub unit_label: String,
    pub unit_price: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocumentInput {
    pub status: String,
    pub customer_name: String,
    pub customer_email: String,
    pub customer_phone: String,
    pub billing_address: String,
    pub issue_date: String,
    pub expiry_or_due_date: String,
    pub discount: String,
    pub tax_rate: String,
    pub notes: String,
    pub terms: String,
    pub lines: Vec<LineInput>,
}

impl Default for DocumentInput {
    fn default() -> Self {
        Self {
            status: "draft".into(),
            customer_name: String::new(),
            customer_email: String::new(),
            customer_phone: String::new(),
            billing_address: String::new(),
            issue_date: String::new(),
            expiry_or_due_date: String::new(),
            discount: "0.00".into(),
            tax_rate: "0".into(),
            notes: String::new(),
            terms: String::new(),
            lines: vec![LineInput::default()],
        }
    }
}

#[derive(Clone, Debug)]
pub struct ValidLine {
    pub description: String,
    pub quantity: Decimal,
    pub unit_label: String,
    pub unit_price_cents: i64,
}

#[derive(Clone, Debug)]
pub struct ValidDocument {
    pub input: DocumentInput,
    pub discount_cents: i64,
    pub tax_basis_points: u32,
    pub lines: Vec<ValidLine>,
}

impl DocumentInput {
    pub fn validate(&self, kind: DocumentKind) -> Result<ValidDocument, Vec<String>> {
        let mut errors = Vec::new();
        required(&self.customer_name, "Customer name", 200, &mut errors);
        required(&self.issue_date, "Issue date", 10, &mut errors);
        bounded(&self.customer_email, "Customer email", 320, &mut errors);
        optional_email(&self.customer_email, "Customer email", &mut errors);
        bounded(&self.customer_phone, "Customer phone", 100, &mut errors);
        bounded(&self.billing_address, "Billing address", 2_000, &mut errors);
        bounded(&self.notes, "Notes", 5_000, &mut errors);
        bounded(&self.terms, "Terms", 5_000, &mut errors);
        if !valid_status(kind, &self.status) {
            errors.push("Choose a valid status.".into());
        }
        if !valid_date(&self.issue_date) {
            errors.push("Issue date must be YYYY-MM-DD.".into());
        }
        if !self.expiry_or_due_date.is_empty() && !valid_date(&self.expiry_or_due_date) {
            errors.push(format!("{} must be YYYY-MM-DD.", kind.date_label()));
        }
        if self.lines.is_empty() {
            errors.push("Add at least one line item.".into());
        }
        if self.lines.len() > MAX_LINES {
            errors.push(format!(
                "A document can have at most {MAX_LINES} line items."
            ));
        }

        let discount_cents = parse_money(&self.discount).unwrap_or_else(|| {
            errors.push("Discount must be a non-negative amount with at most two decimals.".into());
            0
        });
        let tax_basis_points = parse_tax(&self.tax_rate).unwrap_or_else(|| {
            errors.push(
                "Tax rate must be between 0 and 100 percent with at most two decimals.".into(),
            );
            0
        });
        let mut lines = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            required(
                &line.description,
                &format!("Line {} description", index + 1),
                500,
                &mut errors,
            );
            bounded(
                &line.unit_label,
                &format!("Line {} unit", index + 1),
                50,
                &mut errors,
            );
            let quantity = Decimal::from_str(line.quantity.trim())
                .ok()
                .filter(|value| !value.is_sign_negative() && value.scale() <= 6);
            let Some(quantity) = quantity else {
                errors.push(format!(
                    "Line {} quantity must be non-negative with at most six decimal places.",
                    index + 1
                ));
                continue;
            };
            let Some(unit_price_cents) = parse_money(&line.unit_price) else {
                errors.push(format!(
                    "Line {} unit price must be a non-negative amount with at most two decimals.",
                    index + 1
                ));
                continue;
            };
            lines.push(ValidLine {
                description: line.description.trim().into(),
                quantity,
                unit_label: line.unit_label.trim().into(),
                unit_price_cents,
            });
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        calculate_document(
            &lines
                .iter()
                .map(|line| LineCalculationInput {
                    quantity: line.quantity,
                    unit_price: Money::from_cents(line.unit_price_cents).unwrap_or(Money::ZERO),
                })
                .collect::<Vec<_>>(),
            Money::from_cents(discount_cents).unwrap_or(Money::ZERO),
            tax_basis_points,
        )
        .map_err(|error| vec![format!("Could not calculate totals: {error}")])?;
        Ok(ValidDocument {
            input: self.clone(),
            discount_cents,
            tax_basis_points,
            lines,
        })
    }
}

pub fn parse_money(value: &str) -> Option<i64> {
    let decimal = Decimal::from_str(value.trim()).ok()?;
    if decimal.is_sign_negative() || decimal.scale() > 2 {
        return None;
    }
    (decimal * Decimal::from(100)).try_into().ok()
}

fn parse_tax(value: &str) -> Option<u32> {
    let decimal = Decimal::from_str(value.trim()).ok()?;
    if decimal.is_sign_negative() || decimal > Decimal::from(100) || decimal.scale() > 2 {
        return None;
    }
    (decimal * Decimal::from(100)).try_into().ok()
}

fn valid_status(kind: DocumentKind, status: &str) -> bool {
    match kind {
        DocumentKind::Quote => matches!(status, "draft" | "sent" | "accepted" | "declined"),
        DocumentKind::Invoice => matches!(status, "draft" | "sent" | "paid"),
    }
}

fn valid_date(value: &str) -> bool {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
}
fn bounded(value: &str, label: &str, max: usize, errors: &mut Vec<String>) {
    if value.chars().count() > max {
        errors.push(format!("{label} must be {max} characters or fewer."));
    }
}
fn required(value: &str, label: &str, max: usize, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{label} is required."));
    }
    bounded(value, label, max, errors);
}

/// Checks one bounded mailbox address suitable for storage and delivery.
#[must_use]
pub fn valid_single_email(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > 320
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        || value.contains([',', ';'])
    {
        return false;
    }
    let mut parts = value.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if local.is_empty()
        || local.len() > 64
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&byte))
    {
        return false;
    }
    if domain.len() > 253 || !domain.contains('.') {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

fn optional_email(value: &str, label: &str, errors: &mut Vec<String>) {
    if !value.trim().is_empty() && !valid_single_email(value) {
        errors.push(format!("{label} must be one valid email address."));
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct JobInput {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub customer_name: String,
    #[serde(default)]
    pub customer_phone: String,
    #[serde(default)]
    pub customer_email: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub material: String,
    #[serde(default = "default_job_status")]
    pub status: String,
    #[serde(default)]
    pub due_date: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub quote_id: Option<String>,
}

fn default_job_status() -> String {
    JobStage::Design.as_str().into()
}

impl Default for JobInput {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            price: String::new(),
            customer_name: String::new(),
            customer_phone: String::new(),
            customer_email: String::new(),
            category: String::new(),
            material: String::new(),
            status: JobStage::Design.as_str().into(),
            due_date: String::new(),
            notes: String::new(),
            quote_id: None,
        }
    }
}

impl JobInput {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        required(&self.title, "Job title", 200, &mut errors);
        required(&self.description, "Job description", 5_000, &mut errors);
        if self.price.trim().is_empty() {
            errors.push("Job price is required.".into());
        } else if parse_money(&self.price).is_none() {
            errors.push(
                "Job price must be a non-negative amount with at most two decimal places.".into(),
            );
        }
        bounded(&self.customer_name, "Customer name", 200, &mut errors);
        bounded(&self.customer_phone, "Customer phone", 100, &mut errors);
        bounded(&self.customer_email, "Customer email", 320, &mut errors);
        bounded(&self.category, "Category", 100, &mut errors);
        bounded(&self.material, "Material", 100, &mut errors);
        bounded(&self.notes, "Notes", 5_000, &mut errors);
        if JobStage::from_str(&self.status).is_err() {
            errors.push("Choose a valid job status.".into());
        }
        if !self.due_date.is_empty() && !valid_date(&self.due_date) {
            errors.push("Due date must be YYYY-MM-DD.".into());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct JobCardUpdateInput {
    pub customer_name: String,
    #[serde(default)]
    pub customer_phone: String,
    #[serde(default)]
    pub customer_email: String,
    #[serde(default)]
    pub notes: String,
}

impl JobCardUpdateInput {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        bounded(&self.customer_name, "Customer name", 200, &mut errors);
        bounded(&self.customer_phone, "Customer phone", 100, &mut errors);
        bounded(&self.customer_email, "Customer email", 320, &mut errors);
        bounded(&self.notes, "Notes", 5_000, &mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct JobCommentInput {
    pub comment: String,
}

impl JobCommentInput {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        required(&self.comment, "Comment", 1_000, &mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct SettingsInput {
    pub business_name: String,
    pub address: String,
    pub phone: String,
    pub email: String,
    pub default_tax_rate: String,
    #[serde(default = "default_settings_turnaround_days")]
    pub default_turnaround_days: String,
    pub quote_prefix: String,
    pub invoice_prefix: String,
}

fn default_settings_turnaround_days() -> String {
    "3".into()
}

impl SettingsInput {
    pub fn validate(&self) -> Result<(u32, u16), Vec<String>> {
        let mut errors = Vec::new();
        required(&self.business_name, "Business name", 200, &mut errors);
        bounded(&self.address, "Address", 2_000, &mut errors);
        bounded(&self.phone, "Phone", 100, &mut errors);
        bounded(&self.email, "Email", 320, &mut errors);
        optional_email(&self.email, "Email", &mut errors);
        required(&self.quote_prefix, "Quote prefix", 20, &mut errors);
        required(&self.invoice_prefix, "Invoice prefix", 20, &mut errors);
        let tax = parse_tax(&self.default_tax_rate).unwrap_or_else(|| {
            errors.push("Default tax rate must be between 0 and 100 percent.".into());
            0
        });
        let turnaround_days = self
            .default_turnaround_days
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|days| (1..=365).contains(days))
            .unwrap_or_else(|| {
                errors.push("Default turnaround must be between 1 and 365 days.".into());
                1
            });
        if errors.is_empty() {
            Ok((tax, turnaround_days))
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_preserves_and_bounds_input() {
        let mut input = DocumentInput {
            customer_name: "Acme".into(),
            issue_date: "2026-08-20".into(),
            ..DocumentInput::default()
        };
        input.lines[0] = LineInput {
            description: "Posters".into(),
            quantity: "1.25".into(),
            unit_label: "each".into(),
            unit_price: "12.34".into(),
        };
        assert!(input.validate(DocumentKind::Quote).is_ok());
        input.notes = "x".repeat(5_001);
        assert!(input.validate(DocumentKind::Quote).is_err());
    }

    #[test]
    fn single_email_validation_rejects_malformed_multiple_and_newline_values() {
        for invalid in [
            "customer",
            "customer@localhost",
            "customer@@example.com",
            "customer@example.com,other@example.com",
            "customer@example.com\nBcc:other@example.com",
            ".customer@example.com",
            "customer@example..com",
        ] {
            assert!(!valid_single_email(invalid), "accepted {invalid:?}");
        }
        assert!(valid_single_email("customer+proof@example.test"));
        let local = "a".repeat(64);
        assert!(valid_single_email(&format!("{local}@example.test")));
        assert!(!valid_single_email(&format!(
            "{}@example.test",
            "a".repeat(65)
        )));
    }

    #[test]
    fn production_stages_round_trip() {
        for (value, stage) in [
            ("design", JobStage::Design),
            ("waiting", JobStage::Waiting),
            ("print", JobStage::Print),
            ("finish", JobStage::Finish),
            ("complete", JobStage::Complete),
        ] {
            assert_eq!(JobStage::from_str(value), Ok(stage));
            assert_eq!(stage.as_str(), value);
        }
        assert!(JobStage::from_str("done").is_err());
    }

    #[test]
    fn job_category_accepts_custom_product_values() {
        let input = JobInput {
            title: "Fleet graphics".into(),
            description: "Install the approved fleet graphics on both sides.".into(),
            price: "1250.00".into(),
            customer_name: "Acme".into(),
            category: "Vehicle wrap".into(),
            ..JobInput::default()
        };
        assert!(input.validate().is_ok());
    }

    #[test]
    fn job_contact_validation_accepts_arbitrary_optional_values_and_rejects_oversized_values() {
        let mut input = JobInput {
            title: "Fleet graphics".into(),
            description: "Install the approved fleet graphics on both sides.".into(),
            price: "1250.00".into(),
            customer_name: "Acme".into(),
            category: "Vehicle wrap".into(),
            customer_phone: "+1 555 0100".into(),
            customer_email: "buyer@example.test".into(),
            ..JobInput::default()
        };
        assert!(input.validate().is_ok());
        input.customer_email = "buyer at example.test".into();
        assert!(input.validate().is_ok());
        input.customer_email.clear();
        input.customer_name.clear();
        assert!(input.validate().is_ok());
        input.customer_phone = "x".repeat(101);
        assert_eq!(
            input.validate(),
            Err(vec![
                "Customer phone must be 100 characters or fewer.".into()
            ])
        );
        input.customer_phone.clear();
        input.price = "12.345".into();
        assert_eq!(
            input.validate(),
            Err(vec![
                "Job price must be a non-negative amount with at most two decimal places.".into()
            ])
        );
    }

    #[test]
    fn card_updates_and_comments_enforce_their_public_limits() {
        let mut update = JobCardUpdateInput {
            customer_name: "Acme".into(),
            customer_phone: "+1 555 0100".into(),
            customer_email: "buyer@example.test".into(),
            notes: "Use the latest proof".into(),
        };
        assert!(update.validate().is_ok());
        update.customer_email = "not an email".into();
        assert!(update.validate().is_ok());
        update.customer_phone = "x".repeat(101);
        assert!(update.validate().is_err());

        assert!(
            JobCommentInput {
                comment: "Customer approved the blue stock".into(),
            }
            .validate()
            .is_ok()
        );
        assert!(
            JobCommentInput {
                comment: " ".into(),
            }
            .validate()
            .is_err()
        );
        assert!(
            JobCommentInput {
                comment: "x".repeat(1_001),
            }
            .validate()
            .is_err()
        );
    }
}
