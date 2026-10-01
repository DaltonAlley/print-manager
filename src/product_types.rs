/// Common shop products offered as editor suggestions while the persisted
/// category remains open to existing and shop-specific product names.
pub(crate) const STANDARD_PRODUCT_TYPES: [&str; 5] =
    ["Small print", "Banner", "Adhesive", "Poster", "Canvas"];

/// Returns a stable styling key for a job's product mark.
pub(crate) fn product_mark_key(category: &str) -> &'static str {
    if category.eq_ignore_ascii_case("small print") {
        "small-print"
    } else if category.eq_ignore_ascii_case("banner") {
        "banner"
    } else if category.eq_ignore_ascii_case("adhesive") {
        "adhesive"
    } else if category.eq_ignore_ascii_case("poster") {
        "poster"
    } else if category.eq_ignore_ascii_case("canvas") {
        "canvas"
    } else {
        "custom"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_products_have_marks_and_custom_products_have_a_fallback() {
        assert_eq!(
            STANDARD_PRODUCT_TYPES,
            ["Small print", "Banner", "Adhesive", "Poster", "Canvas"]
        );
        assert_eq!(product_mark_key("Banner"), "banner");
        assert_eq!(product_mark_key("small PRINT"), "small-print");
        assert_eq!(product_mark_key("Vehicle wrap"), "custom");
    }
}
