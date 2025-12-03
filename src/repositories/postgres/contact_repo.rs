//! Contact repository implementation

use chrono::Utc;
use uuid::Uuid;

use crate::domain::{
    Contact, CreateContactRequest, DomainError, DomainResult,
    Pagination, Paginated, UpdateContactRequest,
};
use super::PostgresPool;

/// Repository for contact operations
pub struct ContactRepository;

impl ContactRepository {
    /// Create a new contact
    pub async fn create(
        pool: &PostgresPool,
        elder_id: Uuid,
        req: &CreateContactRequest,
    ) -> DomainResult<Contact> {
        let client = pool.get().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        let row = client
            .query_one(
                r#"
                INSERT INTO contacts (id, elder_id, name, relationship, phone, notes, is_emergency, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                RETURNING id, elder_id, name, relationship, phone, notes, is_emergency, created_at, updated_at
                "#,
                &[&id, &elder_id, &req.name, &req.relationship, &req.phone, &req.notes, &req.is_emergency, &now, &now],
            )
            .await?;
        
        Ok(row_to_contact(&row))
    }
    
    /// Find contact by ID
    pub async fn find_by_id(pool: &PostgresPool, id: Uuid) -> DomainResult<Contact> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, elder_id, name, relationship, phone, notes, is_emergency, created_at, updated_at
                FROM contacts WHERE id = $1
                "#,
                &[&id],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Contact not found".to_string()))?;
        
        Ok(row_to_contact(&row))
    }
    
    /// Find contact by elder ID and name (fuzzy search)
    pub async fn find_by_name(
        pool: &PostgresPool,
        elder_id: Uuid,
        query: &str,
    ) -> DomainResult<Vec<Contact>> {
        let client = pool.get().await?;
        let search_pattern = format!("%{}%", query.to_lowercase());
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, name, relationship, phone, notes, is_emergency, created_at, updated_at
                FROM contacts 
                WHERE elder_id = $1 AND (LOWER(name) LIKE $2 OR LOWER(relationship) LIKE $2)
                ORDER BY name
                "#,
                &[&elder_id, &search_pattern],
            )
            .await?;
        
        Ok(rows.iter().map(row_to_contact).collect())
    }
    
    /// Find emergency contacts for an elder
    pub async fn find_emergency(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<Vec<Contact>> {
        let client = pool.get().await?;
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, name, relationship, phone, notes, is_emergency, created_at, updated_at
                FROM contacts 
                WHERE elder_id = $1 AND is_emergency = true
                ORDER BY name
                "#,
                &[&elder_id],
            )
            .await?;
        
        Ok(rows.iter().map(row_to_contact).collect())
    }
    
    /// Find all contacts for an elder (no pagination)
    pub async fn find_all_by_elder(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<Vec<Contact>> {
        let client = pool.get().await?;
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, name, relationship, phone, notes, is_emergency, created_at, updated_at
                FROM contacts 
                WHERE elder_id = $1
                ORDER BY is_emergency DESC, name
                "#,
                &[&elder_id],
            )
            .await?;
        
        Ok(rows.iter().map(row_to_contact).collect())
    }
    
    /// List contacts for an elder
    pub async fn list_by_elder(
        pool: &PostgresPool,
        elder_id: Uuid,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<Contact>> {
        let client = pool.get().await?;
        
        let total: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM contacts WHERE elder_id = $1",
                &[&elder_id],
            )
            .await?
            .get(0);
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, name, relationship, phone, notes, is_emergency, created_at, updated_at
                FROM contacts
                WHERE elder_id = $1
                ORDER BY name
                LIMIT $2 OFFSET $3
                "#,
                &[&elder_id, &pagination.limit(), &pagination.offset()],
            )
            .await?;
        
        let contacts: Vec<Contact> = rows.iter().map(row_to_contact).collect();
        
        Ok(Paginated::new(contacts, total, pagination))
    }
    
    /// Update contact
    pub async fn update(
        pool: &PostgresPool,
        id: Uuid,
        req: &UpdateContactRequest,
    ) -> DomainResult<Contact> {
        let current = Self::find_by_id(pool, id).await?;
        let client = pool.get().await?;
        let now = Utc::now();
        
        let name = req.name.as_ref().unwrap_or(&current.name);
        let relationship = req.relationship.as_ref().unwrap_or(&current.relationship);
        let phone = req.phone.as_ref().unwrap_or(&current.phone);
        let notes = req.notes.clone().or(current.notes);
        let is_emergency = req.is_emergency.unwrap_or(current.is_emergency);
        
        let row = client
            .query_one(
                r#"
                UPDATE contacts 
                SET name = $2, relationship = $3, phone = $4, notes = $5, is_emergency = $6, updated_at = $7
                WHERE id = $1
                RETURNING id, elder_id, name, relationship, phone, notes, is_emergency, created_at, updated_at
                "#,
                &[&id, &name, &relationship, &phone, &notes, &is_emergency, &now],
            )
            .await?;
        
        Ok(row_to_contact(&row))
    }
    
    /// Delete contact
    pub async fn delete(pool: &PostgresPool, id: Uuid) -> DomainResult<()> {
        let client = pool.get().await?;
        
        let rows_affected = client
            .execute("DELETE FROM contacts WHERE id = $1", &[&id])
            .await?;
        
        if rows_affected == 0 {
            return Err(DomainError::NotFound("Contact not found".to_string()));
        }
        
        Ok(())
    }
    
    /// Count contacts for an elder
    pub async fn count_by_elder(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<i64> {
        let client = pool.get().await?;
        
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM contacts WHERE elder_id = $1",
                &[&elder_id],
            )
            .await?
            .get(0);
        
        Ok(count)
    }
}

fn row_to_contact(row: &tokio_postgres::Row) -> Contact {
    Contact {
        id: row.get("id"),
        elder_id: row.get("elder_id"),
        name: row.get("name"),
        relationship: row.get("relationship"),
        phone: row.get("phone"),
        notes: row.get("notes"),
        is_emergency: row.get("is_emergency"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

