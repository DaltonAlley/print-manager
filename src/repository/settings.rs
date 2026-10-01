use uuid::Uuid;

use crate::domain::SettingsInput;

use super::{Repository, Settings};

impl Repository {
    pub async fn settings(&self) -> Result<Settings, sqlx::Error> {
        sqlx::query_as("SELECT business_name,address,phone,email,logo_path,default_tax_basis_points,default_turnaround_days,quote_prefix,invoice_prefix,next_quote_number,next_invoice_number FROM settings LIMIT 1").fetch_one(&self.pool).await
    }
    pub async fn ensure_settings(&self) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT OR IGNORE INTO settings (id,business_name) VALUES (?,?)")
            .bind(Uuid::new_v4().to_string())
            .bind("Print Shop")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn save_settings(
        &self,
        input: &SettingsInput,
        tax: u32,
        turnaround_days: u16,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE settings SET business_name=?,address=?,phone=?,email=?,default_tax_basis_points=?,default_turnaround_days=?,quote_prefix=?,invoice_prefix=? WHERE singleton=1")
            .bind(input.business_name.trim()).bind(input.address.trim()).bind(input.phone.trim()).bind(input.email.trim()).bind(tax).bind(turnaround_days).bind(input.quote_prefix.trim()).bind(input.invoice_prefix.trim()).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn save_logo_path(&self, logo_path: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE settings SET logo_path=? WHERE singleton=1")
            .bind(logo_path)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
