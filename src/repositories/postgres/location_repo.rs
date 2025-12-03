//! Location repository implementation

use chrono::Utc;
use uuid::Uuid;

use crate::domain::{
    CreateLocationRequest, DomainError, DomainResult, Location, LocationType,
    Pagination, Paginated, UpdateLocationRequest,
};
use super::PostgresPool;

/// Repository for location operations
pub struct LocationRepository;

impl LocationRepository {
    /// Create a new location
    pub async fn create(
        pool: &PostgresPool,
        elder_id: Uuid,
        req: &CreateLocationRequest,
    ) -> DomainResult<Location> {
        let client = pool.get().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        // If this is set as home, unset any existing home location
        if req.is_home {
            client
                .execute(
                    "UPDATE locations SET is_home = false WHERE elder_id = $1",
                    &[&elder_id],
                )
                .await?;
        }
        
        let location_type = req.location_type.to_string();
        
        let row = client
            .query_one(
                r#"
                INSERT INTO locations (id, elder_id, name, address, latitude, longitude, extra_instructions, is_home, location_type, tags, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                RETURNING id, elder_id, name, address, latitude, longitude, extra_instructions, is_home, location_type, tags, created_at, updated_at
                "#,
                &[&id, &elder_id, &req.name, &req.address, &req.latitude, &req.longitude, &req.extra_instructions, &req.is_home, &location_type, &req.tags, &now, &now],
            )
            .await?;
        
        Ok(row_to_location(&row))
    }
    
    /// Find location by ID
    pub async fn find_by_id(pool: &PostgresPool, id: Uuid) -> DomainResult<Location> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, elder_id, name, address, latitude, longitude, extra_instructions, is_home, location_type, tags, created_at, updated_at
                FROM locations WHERE id = $1
                "#,
                &[&id],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Location not found".to_string()))?;
        
        Ok(row_to_location(&row))
    }
    
    /// Find location by elder ID and name
    pub async fn find_by_name(
        pool: &PostgresPool,
        elder_id: Uuid,
        query: &str,
    ) -> DomainResult<Vec<Location>> {
        let client = pool.get().await?;
        let search_pattern = format!("%{}%", query.to_lowercase());
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, name, address, latitude, longitude, extra_instructions, is_home, location_type, tags, created_at, updated_at
                FROM locations 
                WHERE elder_id = $1 AND LOWER(name) LIKE $2
                ORDER BY name
                "#,
                &[&elder_id, &search_pattern],
            )
            .await?;
        
        Ok(rows.iter().map(row_to_location).collect())
    }
    
    /// Find home location for an elder
    pub async fn find_home(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<Option<Location>> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, elder_id, name, address, latitude, longitude, extra_instructions, is_home, location_type, tags, created_at, updated_at
                FROM locations 
                WHERE elder_id = $1 AND is_home = true
                "#,
                &[&elder_id],
            )
            .await?;
        
        Ok(row.map(|r| row_to_location(&r)))
    }
    
    /// List locations for an elder
    pub async fn list_by_elder(
        pool: &PostgresPool,
        elder_id: Uuid,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<Location>> {
        let client = pool.get().await?;
        
        let total: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM locations WHERE elder_id = $1",
                &[&elder_id],
            )
            .await?
            .get(0);
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, name, address, latitude, longitude, extra_instructions, is_home, location_type, tags, created_at, updated_at
                FROM locations
                WHERE elder_id = $1
                ORDER BY is_home DESC, location_type, name
                LIMIT $2 OFFSET $3
                "#,
                &[&elder_id, &pagination.limit(), &pagination.offset()],
            )
            .await?;
        
        let locations: Vec<Location> = rows.iter().map(row_to_location).collect();
        
        Ok(Paginated::new(locations, total, pagination))
    }
    
    /// List all locations for an elder (no pagination, for AI)
    pub async fn list_all_by_elder(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<Vec<Location>> {
        let client = pool.get().await?;
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, name, address, latitude, longitude, extra_instructions, is_home, location_type, tags, created_at, updated_at
                FROM locations
                WHERE elder_id = $1
                ORDER BY is_home DESC, location_type, name
                "#,
                &[&elder_id],
            )
            .await?;
        
        Ok(rows.iter().map(row_to_location).collect())
    }
    
    /// List locations by type
    pub async fn list_by_type(
        pool: &PostgresPool,
        elder_id: Uuid,
        location_type: LocationType,
    ) -> DomainResult<Vec<Location>> {
        let client = pool.get().await?;
        let type_str = location_type.to_string();
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, name, address, latitude, longitude, extra_instructions, is_home, location_type, tags, created_at, updated_at
                FROM locations
                WHERE elder_id = $1 AND location_type = $2
                ORDER BY is_home DESC, name
                "#,
                &[&elder_id, &type_str],
            )
            .await?;
        
        Ok(rows.iter().map(row_to_location).collect())
    }
    
    /// Update location
    pub async fn update(
        pool: &PostgresPool,
        id: Uuid,
        req: &UpdateLocationRequest,
    ) -> DomainResult<Location> {
        let current = Self::find_by_id(pool, id).await?;
        let client = pool.get().await?;
        let now = Utc::now();
        
        // If this is being set as home, unset any existing home location
        if req.is_home == Some(true) && !current.is_home {
            client
                .execute(
                    "UPDATE locations SET is_home = false WHERE elder_id = $1",
                    &[&current.elder_id],
                )
                .await?;
        }
        
        let name = req.name.as_ref().unwrap_or(&current.name);
        let address = req.address.as_ref().unwrap_or(&current.address);
        let latitude = req.latitude.or(current.latitude);
        let longitude = req.longitude.or(current.longitude);
        let extra_instructions = req.extra_instructions.clone().or(current.extra_instructions);
        let is_home = req.is_home.unwrap_or(current.is_home);
        let location_type = req.location_type.unwrap_or(current.location_type).to_string();
        let tags = req.tags.clone().unwrap_or(current.tags);
        
        let row = client
            .query_one(
                r#"
                UPDATE locations 
                SET name = $2, address = $3, latitude = $4, longitude = $5, extra_instructions = $6, is_home = $7, location_type = $8, tags = $9, updated_at = $10
                WHERE id = $1
                RETURNING id, elder_id, name, address, latitude, longitude, extra_instructions, is_home, location_type, tags, created_at, updated_at
                "#,
                &[&id, &name, &address, &latitude, &longitude, &extra_instructions, &is_home, &location_type, &tags, &now],
            )
            .await?;
        
        Ok(row_to_location(&row))
    }
    
    /// Delete location
    pub async fn delete(pool: &PostgresPool, id: Uuid) -> DomainResult<()> {
        let client = pool.get().await?;
        
        let rows_affected = client
            .execute("DELETE FROM locations WHERE id = $1", &[&id])
            .await?;
        
        if rows_affected == 0 {
            return Err(DomainError::NotFound("Location not found".to_string()));
        }
        
        Ok(())
    }
    
    /// Count locations for an elder
    pub async fn count_by_elder(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<i64> {
        let client = pool.get().await?;
        
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM locations WHERE elder_id = $1",
                &[&elder_id],
            )
            .await?
            .get(0);
        
        Ok(count)
    }
}

fn row_to_location(row: &tokio_postgres::Row) -> Location {
    Location {
        id: row.get("id"),
        elder_id: row.get("elder_id"),
        name: row.get("name"),
        address: row.get("address"),
        latitude: row.get("latitude"),
        longitude: row.get("longitude"),
        extra_instructions: row.get("extra_instructions"),
        is_home: row.get("is_home"),
        location_type: row.get::<_, String>("location_type")
            .parse()
            .unwrap_or(LocationType::Destination),
        tags: row.get("tags"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

