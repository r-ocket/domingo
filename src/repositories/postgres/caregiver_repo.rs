//! Caregiver repository implementation

use chrono::Utc;
use uuid::Uuid;

use crate::domain::{
    Caregiver, CaregiverElderRelationship, CreateCaregiverRequest, 
    DomainError, DomainResult, Pagination, Paginated, UpdateCaregiverRequest,
    UpdateRelationshipRequest, UserRole,
};
use super::PostgresPool;

/// Repository for caregiver operations
pub struct CaregiverRepository;

impl CaregiverRepository {
    /// Create a new caregiver
    pub async fn create(
        pool: &PostgresPool,
        req: &CreateCaregiverRequest,
        password_hash: &str,
    ) -> DomainResult<Caregiver> {
        let client = pool.get().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        let row = client
            .query_one(
                r#"
                INSERT INTO caregivers (id, name, email, password_hash, role, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                RETURNING id, name, email, password_hash, role, phone, notes, stripe_customer_id, created_at, updated_at
                "#,
                &[&id, &req.name, &req.email, &password_hash, &"caregiver", &now, &now],
            )
            .await
            .map_err(|e| {
                if e.to_string().contains("unique") {
                    DomainError::Conflict("Email already exists".to_string())
                } else {
                    DomainError::Database(e.to_string())
                }
            })?;
        
        Ok(row_to_caregiver(&row))
    }
    
    /// Find caregiver by ID
    pub async fn find_by_id(pool: &PostgresPool, id: Uuid) -> DomainResult<Caregiver> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, name, email, password_hash, role, phone, notes, stripe_customer_id, created_at, updated_at
                FROM caregivers WHERE id = $1
                "#,
                &[&id],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Caregiver not found".to_string()))?;
        
        Ok(row_to_caregiver(&row))
    }
    
    /// Find caregiver by email
    pub async fn find_by_email(pool: &PostgresPool, email: &str) -> DomainResult<Caregiver> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, name, email, password_hash, role, phone, notes, stripe_customer_id, created_at, updated_at
                FROM caregivers WHERE email = $1
                "#,
                &[&email],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Caregiver not found".to_string()))?;
        
        Ok(row_to_caregiver(&row))
    }
    
    /// Update caregiver profile
    pub async fn update(
        pool: &PostgresPool,
        id: Uuid,
        req: &UpdateCaregiverRequest,
    ) -> DomainResult<Caregiver> {
        let current = Self::find_by_id(pool, id).await?;
        let client = pool.get().await?;
        let now = Utc::now();
        
        let name = req.name.as_ref().unwrap_or(&current.name);
        let email = req.email.as_ref().unwrap_or(&current.email);
        let phone = req.phone.clone().or(current.phone);
        let notes = req.notes.clone().or(current.notes);
        
        let row = client
            .query_one(
                r#"
                UPDATE caregivers 
                SET name = $2, email = $3, phone = $4, notes = $5, updated_at = $6
                WHERE id = $1
                RETURNING id, name, email, password_hash, role, phone, notes, stripe_customer_id, created_at, updated_at
                "#,
                &[&id, &name, &email, &phone, &notes, &now],
            )
            .await
            .map_err(|e| {
                if e.to_string().contains("unique") {
                    DomainError::Conflict("Email already exists".to_string())
                } else {
                    DomainError::Database(e.to_string())
                }
            })?;
        
        Ok(row_to_caregiver(&row))
    }
    
    /// Update Stripe customer ID
    pub async fn update_stripe_customer_id(
        pool: &PostgresPool,
        id: Uuid,
        stripe_customer_id: &str,
    ) -> DomainResult<()> {
        let client = pool.get().await?;
        let now = Utc::now();
        
        client
            .execute(
                "UPDATE caregivers SET stripe_customer_id = $2, updated_at = $3 WHERE id = $1",
                &[&id, &stripe_customer_id, &now],
            )
            .await?;
        
        Ok(())
    }
    
    /// List all caregivers (admin)
    pub async fn list(
        pool: &PostgresPool,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<Caregiver>> {
        let client = pool.get().await?;
        
        let total: i64 = client
            .query_one("SELECT COUNT(*) FROM caregivers", &[])
            .await?
            .get(0);
        
        let rows = client
            .query(
                r#"
                SELECT id, name, email, password_hash, role, phone, notes, stripe_customer_id, created_at, updated_at
                FROM caregivers
                ORDER BY created_at DESC
                LIMIT $1 OFFSET $2
                "#,
                &[&pagination.limit(), &pagination.offset()],
            )
            .await?;
        
        let caregivers: Vec<Caregiver> = rows.iter().map(row_to_caregiver).collect();
        
        Ok(Paginated::new(caregivers, total, pagination))
    }
    
    /// Update caregiver role (admin only)
    pub async fn update_role(
        pool: &PostgresPool,
        id: Uuid,
        role: UserRole,
    ) -> DomainResult<()> {
        let client = pool.get().await?;
        let now = Utc::now();
        
        client
            .execute(
                "UPDATE caregivers SET role = $2, updated_at = $3 WHERE id = $1",
                &[&id, &role.to_string(), &now],
            )
            .await?;
        
        Ok(())
    }
    
    // === Relationship methods ===
    
    /// Get relationship between caregiver and elder
    pub async fn get_relationship(
        pool: &PostgresPool,
        caregiver_id: Uuid,
        elder_id: Uuid,
    ) -> DomainResult<Option<CaregiverElderRelationship>> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, caregiver_id, elder_id, relationship, notes, created_at, updated_at
                FROM caregiver_elder_relationships 
                WHERE caregiver_id = $1 AND elder_id = $2
                "#,
                &[&caregiver_id, &elder_id],
            )
            .await?;
        
        Ok(row.map(|r| row_to_relationship(&r)))
    }
    
    /// List all relationships for a caregiver
    pub async fn list_relationships(
        pool: &PostgresPool,
        caregiver_id: Uuid,
    ) -> DomainResult<Vec<CaregiverElderRelationship>> {
        let client = pool.get().await?;
        
        let rows = client
            .query(
                r#"
                SELECT id, caregiver_id, elder_id, relationship, notes, created_at, updated_at
                FROM caregiver_elder_relationships 
                WHERE caregiver_id = $1
                ORDER BY created_at
                "#,
                &[&caregiver_id],
            )
            .await?;
        
        Ok(rows.iter().map(row_to_relationship).collect())
    }
    
    /// Create or update relationship
    pub async fn upsert_relationship(
        pool: &PostgresPool,
        caregiver_id: Uuid,
        elder_id: Uuid,
        req: &UpdateRelationshipRequest,
    ) -> DomainResult<CaregiverElderRelationship> {
        let client = pool.get().await?;
        let now = Utc::now();
        
        let relationship = req.relationship.as_deref().unwrap_or("Cuidador");
        let notes = req.notes.as_ref();
        
        let row = client
            .query_one(
                r#"
                INSERT INTO caregiver_elder_relationships (caregiver_id, elder_id, relationship, notes, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $5)
                ON CONFLICT (caregiver_id, elder_id) 
                DO UPDATE SET relationship = COALESCE($3, caregiver_elder_relationships.relationship),
                              notes = COALESCE($4, caregiver_elder_relationships.notes),
                              updated_at = $5
                RETURNING id, caregiver_id, elder_id, relationship, notes, created_at, updated_at
                "#,
                &[&caregiver_id, &elder_id, &relationship, &notes, &now],
            )
            .await?;
        
        Ok(row_to_relationship(&row))
    }
}

fn row_to_caregiver(row: &tokio_postgres::Row) -> Caregiver {
    Caregiver {
        id: row.get("id"),
        name: row.get("name"),
        email: row.get("email"),
        password_hash: row.get("password_hash"),
        role: row.get::<_, String>("role")
            .parse()
            .unwrap_or(UserRole::Caregiver),
        phone: row.get("phone"),
        notes: row.get("notes"),
        stripe_customer_id: row.get("stripe_customer_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn row_to_relationship(row: &tokio_postgres::Row) -> CaregiverElderRelationship {
    CaregiverElderRelationship {
        id: row.get("id"),
        caregiver_id: row.get("caregiver_id"),
        elder_id: row.get("elder_id"),
        relationship: row.get("relationship"),
        notes: row.get("notes"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

