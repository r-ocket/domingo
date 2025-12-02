//! Ride request repository implementation

use chrono::Utc;
use uuid::Uuid;

use crate::domain::{
    CreateRideRequest, DomainError, DomainResult, Pagination, Paginated,
    RideRequest, RideStatus, UpdateRideRequest,
};
use super::PostgresPool;

/// Repository for ride request operations
pub struct RideRequestRepository;

impl RideRequestRepository {
    /// Create a new ride request
    pub async fn create(
        pool: &PostgresPool,
        req: &CreateRideRequest,
    ) -> DomainResult<RideRequest> {
        let client = pool.get().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        let row = client
            .query_one(
                r#"
                INSERT INTO ride_requests (id, elder_id, location_id, pickup_address, dropoff_address, status, requested_at, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                RETURNING id, elder_id, location_id, pickup_address, dropoff_address, uber_ride_id, status, 
                          driver_name, driver_phone, vehicle_make, vehicle_model, vehicle_license,
                          eta_minutes, fare_estimate, fare_actual, requested_at, completed_at, metadata, created_at, updated_at
                "#,
                &[&id, &req.elder_id, &req.location_id, &req.pickup_address, &req.dropoff_address, &"requested", &now, &now, &now],
            )
            .await?;
        
        Ok(row_to_ride_request(&row))
    }
    
    /// Find ride request by ID
    pub async fn find_by_id(pool: &PostgresPool, id: Uuid) -> DomainResult<RideRequest> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, elder_id, location_id, pickup_address, dropoff_address, uber_ride_id, status, 
                       driver_name, driver_phone, vehicle_make, vehicle_model, vehicle_license,
                       eta_minutes, fare_estimate, fare_actual, requested_at, completed_at, metadata, created_at, updated_at
                FROM ride_requests WHERE id = $1
                "#,
                &[&id],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Ride request not found".to_string()))?;
        
        Ok(row_to_ride_request(&row))
    }
    
    /// Find ride request by Uber ride ID
    pub async fn find_by_uber_id(pool: &PostgresPool, uber_ride_id: &str) -> DomainResult<RideRequest> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, elder_id, location_id, pickup_address, dropoff_address, uber_ride_id, status, 
                       driver_name, driver_phone, vehicle_make, vehicle_model, vehicle_license,
                       eta_minutes, fare_estimate, fare_actual, requested_at, completed_at, metadata, created_at, updated_at
                FROM ride_requests WHERE uber_ride_id = $1
                "#,
                &[&uber_ride_id],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Ride request not found".to_string()))?;
        
        Ok(row_to_ride_request(&row))
    }
    
    /// Update ride request
    pub async fn update(
        pool: &PostgresPool,
        id: Uuid,
        req: &UpdateRideRequest,
    ) -> DomainResult<RideRequest> {
        let current = Self::find_by_id(pool, id).await?;
        let client = pool.get().await?;
        let now = Utc::now();
        
        let uber_ride_id = req.uber_ride_id.clone().or(current.uber_ride_id);
        let status = req.status.unwrap_or(current.status).to_string();
        let driver_name = req.driver_name.clone().or(current.driver_name);
        let driver_phone = req.driver_phone.clone().or(current.driver_phone);
        let vehicle_make = req.vehicle_make.clone().or(current.vehicle_make);
        let vehicle_model = req.vehicle_model.clone().or(current.vehicle_model);
        let vehicle_license = req.vehicle_license.clone().or(current.vehicle_license);
        let eta_minutes = req.eta_minutes.or(current.eta_minutes);
        let fare_estimate = req.fare_estimate.clone().or(current.fare_estimate);
        let fare_actual = req.fare_actual.clone().or(current.fare_actual);
        let completed_at = req.completed_at.or(current.completed_at);
        let metadata = req.metadata.clone().or(current.metadata);
        
        let row = client
            .query_one(
                r#"
                UPDATE ride_requests 
                SET uber_ride_id = $2, status = $3, driver_name = $4, driver_phone = $5,
                    vehicle_make = $6, vehicle_model = $7, vehicle_license = $8,
                    eta_minutes = $9, fare_estimate = $10, fare_actual = $11,
                    completed_at = $12, metadata = $13, updated_at = $14
                WHERE id = $1
                RETURNING id, elder_id, location_id, pickup_address, dropoff_address, uber_ride_id, status, 
                          driver_name, driver_phone, vehicle_make, vehicle_model, vehicle_license,
                          eta_minutes, fare_estimate, fare_actual, requested_at, completed_at, metadata, created_at, updated_at
                "#,
                &[&id, &uber_ride_id, &status, &driver_name, &driver_phone, &vehicle_make, &vehicle_model,
                  &vehicle_license, &eta_minutes, &fare_estimate, &fare_actual, &completed_at, &metadata, &now],
            )
            .await?;
        
        Ok(row_to_ride_request(&row))
    }
    
    /// List ride requests for an elder
    pub async fn list_by_elder(
        pool: &PostgresPool,
        elder_id: Uuid,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<RideRequest>> {
        let client = pool.get().await?;
        
        let total: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM ride_requests WHERE elder_id = $1",
                &[&elder_id],
            )
            .await?
            .get(0);
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, location_id, pickup_address, dropoff_address, uber_ride_id, status, 
                       driver_name, driver_phone, vehicle_make, vehicle_model, vehicle_license,
                       eta_minutes, fare_estimate, fare_actual, requested_at, completed_at, metadata, created_at, updated_at
                FROM ride_requests
                WHERE elder_id = $1
                ORDER BY requested_at DESC
                LIMIT $2 OFFSET $3
                "#,
                &[&elder_id, &pagination.limit(), &pagination.offset()],
            )
            .await?;
        
        let rides: Vec<RideRequest> = rows.iter().map(row_to_ride_request).collect();
        
        Ok(Paginated::new(rides, total, pagination))
    }
    
    /// List all ride requests (admin)
    pub async fn list_all(
        pool: &PostgresPool,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<RideRequest>> {
        let client = pool.get().await?;
        
        let total: i64 = client
            .query_one("SELECT COUNT(*) FROM ride_requests", &[])
            .await?
            .get(0);
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, location_id, pickup_address, dropoff_address, uber_ride_id, status, 
                       driver_name, driver_phone, vehicle_make, vehicle_model, vehicle_license,
                       eta_minutes, fare_estimate, fare_actual, requested_at, completed_at, metadata, created_at, updated_at
                FROM ride_requests
                ORDER BY requested_at DESC
                LIMIT $1 OFFSET $2
                "#,
                &[&pagination.limit(), &pagination.offset()],
            )
            .await?;
        
        let rides: Vec<RideRequest> = rows.iter().map(row_to_ride_request).collect();
        
        Ok(Paginated::new(rides, total, pagination))
    }
    
    /// Get active ride for an elder (if any)
    pub async fn get_active_for_elder(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<Option<RideRequest>> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, elder_id, location_id, pickup_address, dropoff_address, uber_ride_id, status, 
                       driver_name, driver_phone, vehicle_make, vehicle_model, vehicle_license,
                       eta_minutes, fare_estimate, fare_actual, requested_at, completed_at, metadata, created_at, updated_at
                FROM ride_requests 
                WHERE elder_id = $1 AND status IN ('requested', 'driver_assigned', 'arriving', 'in_progress')
                ORDER BY requested_at DESC
                LIMIT 1
                "#,
                &[&elder_id],
            )
            .await?;
        
        Ok(row.map(|r| row_to_ride_request(&r)))
    }
    
    /// Count rides today (admin stats)
    pub async fn count_today(pool: &PostgresPool) -> DomainResult<i64> {
        let client = pool.get().await?;
        
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM ride_requests WHERE requested_at::date = CURRENT_DATE",
                &[],
            )
            .await?
            .get(0);
        
        Ok(count)
    }
}

fn row_to_ride_request(row: &tokio_postgres::Row) -> RideRequest {
    RideRequest {
        id: row.get("id"),
        elder_id: row.get("elder_id"),
        location_id: row.get("location_id"),
        pickup_address: row.get("pickup_address"),
        dropoff_address: row.get("dropoff_address"),
        uber_ride_id: row.get("uber_ride_id"),
        status: row.get::<_, String>("status")
            .parse()
            .unwrap_or(RideStatus::Requested),
        driver_name: row.get("driver_name"),
        driver_phone: row.get("driver_phone"),
        vehicle_make: row.get("vehicle_make"),
        vehicle_model: row.get("vehicle_model"),
        vehicle_license: row.get("vehicle_license"),
        eta_minutes: row.get("eta_minutes"),
        fare_estimate: row.get("fare_estimate"),
        fare_actual: row.get("fare_actual"),
        requested_at: row.get("requested_at"),
        completed_at: row.get("completed_at"),
        metadata: row.get("metadata"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

