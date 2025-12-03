//! Elder repository implementation

use chrono::Utc;
use uuid::Uuid;

use crate::domain::{
    CreateElderRequest, DomainError, DomainResult, Elder, ElderStatus,
    Pagination, Paginated, UpdateElderRequest,
};
use super::PostgresPool;

/// Repository for elder operations
pub struct ElderRepository;

impl ElderRepository {
    /// Create a new elder
    pub async fn create(
        pool: &PostgresPool,
        caregiver_id: Uuid,
        req: &CreateElderRequest,
    ) -> DomainResult<Elder> {
        let client = pool.get().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        let row = client
            .query_one(
                r#"
                INSERT INTO elders (id, caregiver_id, name, relationship, phone_number, timezone, language, status, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                RETURNING id, caregiver_id, name, relationship, phone_number, timezone, language, status, created_at, updated_at
                "#,
                &[&id, &caregiver_id, &req.name, &req.relationship, &req.phone_number, &req.timezone, &req.language, &"active", &now, &now],
            )
            .await?;
        
        Ok(row_to_elder(&row))
    }
    
    /// Find elder by ID
    pub async fn find_by_id(pool: &PostgresPool, id: Uuid) -> DomainResult<Elder> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, caregiver_id, name, COALESCE(relationship, 'familiar') as relationship, phone_number, timezone, language, status, created_at, updated_at
                FROM elders WHERE id = $1
                "#,
                &[&id],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Elder not found".to_string()))?;
        
        Ok(row_to_elder(&row))
    }
    
    /// Find elder by phone number (for caller ID lookup)
    pub async fn find_by_phone(pool: &PostgresPool, phone: &str) -> DomainResult<Elder> {
        let client = pool.get().await?;
        
        // Normalize phone number - remove non-digits except leading +
        let normalized = normalize_phone(phone);
        
        let row = client
            .query_opt(
                r#"
                SELECT id, caregiver_id, name, COALESCE(relationship, 'familiar') as relationship, phone_number, timezone, language, status, created_at, updated_at
                FROM elders WHERE phone_number = $1 OR phone_number = $2
                "#,
                &[&phone, &normalized],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Elder not found for this phone number".to_string()))?;
        
        Ok(row_to_elder(&row))
    }
    
    /// Find elder by caregiver ID (returns first one - use list_by_caregiver for multiple)
    pub async fn find_by_caregiver(pool: &PostgresPool, caregiver_id: Uuid) -> DomainResult<Option<Elder>> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, caregiver_id, name, COALESCE(relationship, 'familiar') as relationship, phone_number, timezone, language, status, created_at, updated_at
                FROM elders WHERE caregiver_id = $1
                ORDER BY created_at ASC
                LIMIT 1
                "#,
                &[&caregiver_id],
            )
            .await?;
        
        Ok(row.map(|r| row_to_elder(&r)))
    }
    
    /// List all elders for a caregiver (supports multiple elders per caregiver)
    pub async fn list_by_caregiver(pool: &PostgresPool, caregiver_id: Uuid) -> DomainResult<Vec<Elder>> {
        let client = pool.get().await?;
        
        let rows = client
            .query(
                r#"
                SELECT id, caregiver_id, name, COALESCE(relationship, 'familiar') as relationship, phone_number, timezone, language, status, created_at, updated_at
                FROM elders WHERE caregiver_id = $1
                ORDER BY created_at ASC
                "#,
                &[&caregiver_id],
            )
            .await?;
        
        Ok(rows.iter().map(row_to_elder).collect())
    }
    
    /// Update elder
    pub async fn update(
        pool: &PostgresPool,
        id: Uuid,
        req: &UpdateElderRequest,
    ) -> DomainResult<Elder> {
        let current = Self::find_by_id(pool, id).await?;
        let client = pool.get().await?;
        let now = Utc::now();
        
        let name = req.name.as_ref().unwrap_or(&current.name);
        let relationship = req.relationship.as_ref().unwrap_or(&current.relationship);
        let phone_number = req.phone_number.as_ref().unwrap_or(&current.phone_number);
        let timezone = req.timezone.as_ref().unwrap_or(&current.timezone);
        let language = req.language.as_ref().unwrap_or(&current.language);
        let status = req.status.unwrap_or(current.status).to_string();
        
        let row = client
            .query_one(
                r#"
                UPDATE elders 
                SET name = $2, relationship = $3, phone_number = $4, timezone = $5, language = $6, status = $7, updated_at = $8
                WHERE id = $1
                RETURNING id, caregiver_id, name, COALESCE(relationship, 'familiar') as relationship, phone_number, timezone, language, status, created_at, updated_at
                "#,
                &[&id, &name, &relationship, &phone_number, &timezone, &language, &status, &now],
            )
            .await?;
        
        Ok(row_to_elder(&row))
    }
    
    /// List all elders (admin)
    pub async fn list(
        pool: &PostgresPool,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<Elder>> {
        let client = pool.get().await?;
        
        let total: i64 = client
            .query_one("SELECT COUNT(*) FROM elders", &[])
            .await?
            .get(0);
        
        let rows = client
            .query(
                r#"
                SELECT id, caregiver_id, name, COALESCE(relationship, 'familiar') as relationship, phone_number, timezone, language, status, created_at, updated_at
                FROM elders
                ORDER BY created_at DESC
                LIMIT $1 OFFSET $2
                "#,
                &[&pagination.limit(), &pagination.offset()],
            )
            .await?;
        
        let elders: Vec<Elder> = rows.iter().map(row_to_elder).collect();
        
        Ok(Paginated::new(elders, total, pagination))
    }
    
    /// Get count of active elders
    pub async fn count_active(pool: &PostgresPool) -> DomainResult<i64> {
        let client = pool.get().await?;
        
        let count: i64 = client
            .query_one("SELECT COUNT(*) FROM elders WHERE status = 'active'", &[])
            .await?
            .get(0);
        
        Ok(count)
    }
}

fn row_to_elder(row: &tokio_postgres::Row) -> Elder {
    Elder {
        id: row.get("id"),
        caregiver_id: row.get("caregiver_id"),
        name: row.get("name"),
        relationship: row.get("relationship"),
        phone_number: row.get("phone_number"),
        timezone: row.get("timezone"),
        language: row.get("language"),
        status: row.get::<_, String>("status")
            .parse()
            .unwrap_or(ElderStatus::Active),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn normalize_phone(phone: &str) -> String {
    let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if phone.starts_with('+') {
        format!("+{}", digits)
    } else {
        digits
    }
}
