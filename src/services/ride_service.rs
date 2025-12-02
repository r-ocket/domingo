//! Ride booking service

use std::sync::Arc;
use uuid::Uuid;

use crate::clients::UberClient;
use crate::domain::{
    CreateRideRequest, DomainError, DomainResult, Pagination, Paginated,
    RideInfo, RideRequest, RideStatus, UpdateRideRequest,
};
use crate::repositories::postgres::{LocationRepository, PostgresPool, RideRequestRepository};

/// Ride booking service
pub struct RideService;

impl RideService {
    /// Book a ride to a saved location
    pub async fn book_ride(
        pool: &PostgresPool,
        uber: &Arc<UberClient>,
        elder_id: Uuid,
        location_id: Uuid,
    ) -> DomainResult<RideInfo> {
        // Get the destination location
        let destination = LocationRepository::find_by_id(pool, location_id).await?;
        
        // Get home location for pickup
        let pickup = LocationRepository::find_home(pool, elder_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("Home location not set".to_string()))?;
        
        // Get coordinates (or use a geocoding service if not available)
        let pickup_lat = pickup.latitude.unwrap_or(0.0);
        let pickup_lng = pickup.longitude.unwrap_or(0.0);
        let dropoff_lat = destination.latitude.unwrap_or(0.0);
        let dropoff_lng = destination.longitude.unwrap_or(0.0);
        
        // Request ride from Uber
        let uber_response = uber
            .request_ride(pickup_lat, pickup_lng, dropoff_lat, dropoff_lng, "")
            .await
            .map_err(|e| DomainError::ExternalService(format!("Uber error: {}", e)))?;
        
        // Create ride request record
        let create_req = CreateRideRequest {
            elder_id,
            location_id,
            pickup_address: pickup.address.clone(),
            dropoff_address: destination.address.clone(),
        };
        
        let ride = RideRequestRepository::create(pool, &create_req).await?;
        
        // Update with Uber response
        let update_req = UpdateRideRequest {
            uber_ride_id: Some(uber_response.request_id.clone()),
            status: Some(uber_response.to_domain_status()),
            driver_name: uber_response.driver.as_ref().map(|d| d.name.clone()),
            driver_phone: uber_response.driver.as_ref().map(|d| d.phone_number.clone()),
            vehicle_make: uber_response.vehicle.as_ref().map(|v| v.make.clone()),
            vehicle_model: uber_response.vehicle.as_ref().map(|v| v.model.clone()),
            vehicle_license: uber_response.vehicle.as_ref().map(|v| v.license_plate.clone()),
            eta_minutes: uber_response.eta,
            fare_estimate: None,
            fare_actual: None,
            completed_at: None,
            metadata: None,
        };
        
        let updated_ride = RideRequestRepository::update(pool, ride.id, &update_req).await?;
        
        let vehicle_description = Self::format_vehicle(&updated_ride);
        
        Ok(RideInfo {
            status: updated_ride.status,
            destination_name: destination.name,
            driver_name: updated_ride.driver_name,
            vehicle_description,
            eta_minutes: updated_ride.eta_minutes,
        })
    }
    
    /// Book a ride by location name (for AI tool)
    pub async fn book_ride_by_name(
        pool: &PostgresPool,
        uber: &Arc<UberClient>,
        elder_id: Uuid,
        location_name: &str,
    ) -> DomainResult<RideInfo> {
        // Search for the location
        let locations = LocationRepository::find_by_name(pool, elder_id, location_name).await?;
        
        if locations.is_empty() {
            return Err(DomainError::NotFound(format!(
                "No location found matching '{}'",
                location_name
            )));
        }
        
        // Use the first match
        let location = &locations[0];
        
        Self::book_ride(pool, uber, elder_id, location.id).await
    }
    
    /// Get active ride for an elder
    pub async fn get_active_ride(
        pool: &PostgresPool,
        elder_id: Uuid,
    ) -> DomainResult<Option<RideRequest>> {
        RideRequestRepository::get_active_for_elder(pool, elder_id).await
    }
    
    /// Update ride from Uber webhook
    pub async fn update_ride_from_webhook(
        pool: &PostgresPool,
        uber_ride_id: &str,
        status: RideStatus,
        metadata: Option<serde_json::Value>,
    ) -> DomainResult<RideRequest> {
        let ride = RideRequestRepository::find_by_uber_id(pool, uber_ride_id).await?;
        
        let update_req = UpdateRideRequest {
            uber_ride_id: None,
            status: Some(status),
            driver_name: None,
            driver_phone: None,
            vehicle_make: None,
            vehicle_model: None,
            vehicle_license: None,
            eta_minutes: None,
            fare_estimate: None,
            fare_actual: None,
            completed_at: if status == RideStatus::Completed {
                Some(chrono::Utc::now())
            } else {
                None
            },
            metadata,
        };
        
        RideRequestRepository::update(pool, ride.id, &update_req).await
    }
    
    /// List all rides (admin)
    pub async fn list_all_rides(
        pool: &PostgresPool,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<RideRequest>> {
        RideRequestRepository::list_all(pool, pagination).await
    }
    
    /// Get rides today count (admin stats)
    pub async fn count_rides_today(pool: &PostgresPool) -> DomainResult<i64> {
        RideRequestRepository::count_today(pool).await
    }
    
    /// Format vehicle description
    fn format_vehicle(ride: &RideRequest) -> Option<String> {
        match (&ride.vehicle_make, &ride.vehicle_model, &ride.vehicle_license) {
            (Some(make), Some(model), Some(license)) => {
                Some(format!("{} {} ({})", make, model, license))
            }
            (Some(make), Some(model), None) => Some(format!("{} {}", make, model)),
            _ => None,
        }
    }
}

