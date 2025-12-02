//! Medication repository implementation

use chrono::{NaiveTime, Utc};
use uuid::Uuid;

use crate::domain::{
    CreateMedicationRequest, DomainError, DomainResult, Medication,
    MedicationSchedule, MedicationWithSchedule, Pagination, Paginated,
    UpdateMedicationRequest,
};
use super::PostgresPool;

/// Repository for medication operations
pub struct MedicationRepository;

impl MedicationRepository {
    /// Create a new medication with schedules
    pub async fn create(
        pool: &PostgresPool,
        elder_id: Uuid,
        req: &CreateMedicationRequest,
    ) -> DomainResult<MedicationWithSchedule> {
        let client = pool.get().await?;
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        // Create medication
        let row = client
            .query_one(
                r#"
                INSERT INTO medications (id, elder_id, name, dosage, instructions, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                RETURNING id, elder_id, name, dosage, instructions, created_at, updated_at
                "#,
                &[&id, &elder_id, &req.name, &req.dosage, &req.instructions, &now, &now],
            )
            .await?;
        
        let medication = row_to_medication(&row);
        
        // Create schedules
        let mut schedules = Vec::new();
        for time_str in &req.schedule_times {
            let time = NaiveTime::parse_from_str(time_str, "%H:%M")
                .map_err(|_| DomainError::Validation(format!("Invalid time format: {}", time_str)))?;
            
            let schedule_id = Uuid::new_v4();
            let schedule_row = client
                .query_one(
                    r#"
                    INSERT INTO medication_schedules (id, medication_id, time_of_day, days_pattern, created_at)
                    VALUES ($1, $2, $3, $4, $5)
                    RETURNING id, medication_id, time_of_day, days_pattern, created_at
                    "#,
                    &[&schedule_id, &id, &time, &"daily", &now],
                )
                .await?;
            
            schedules.push(row_to_schedule(&schedule_row));
        }
        
        Ok(MedicationWithSchedule { medication, schedules })
    }
    
    /// Find medication by ID
    pub async fn find_by_id(pool: &PostgresPool, id: Uuid) -> DomainResult<Medication> {
        let client = pool.get().await?;
        
        let row = client
            .query_opt(
                r#"
                SELECT id, elder_id, name, dosage, instructions, created_at, updated_at
                FROM medications WHERE id = $1
                "#,
                &[&id],
            )
            .await?
            .ok_or_else(|| DomainError::NotFound("Medication not found".to_string()))?;
        
        Ok(row_to_medication(&row))
    }
    
    /// Find medication with schedules
    pub async fn find_with_schedules(pool: &PostgresPool, id: Uuid) -> DomainResult<MedicationWithSchedule> {
        let medication = Self::find_by_id(pool, id).await?;
        let schedules = Self::get_schedules(pool, id).await?;
        
        Ok(MedicationWithSchedule { medication, schedules })
    }
    
    /// Get schedules for a medication
    pub async fn get_schedules(pool: &PostgresPool, medication_id: Uuid) -> DomainResult<Vec<MedicationSchedule>> {
        let client = pool.get().await?;
        
        let rows = client
            .query(
                r#"
                SELECT id, medication_id, time_of_day, days_pattern, created_at
                FROM medication_schedules 
                WHERE medication_id = $1
                ORDER BY time_of_day
                "#,
                &[&medication_id],
            )
            .await?;
        
        Ok(rows.iter().map(row_to_schedule).collect())
    }
    
    /// List medications for an elder
    pub async fn list_by_elder(
        pool: &PostgresPool,
        elder_id: Uuid,
        pagination: &Pagination,
    ) -> DomainResult<Paginated<MedicationWithSchedule>> {
        let client = pool.get().await?;
        
        let total: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM medications WHERE elder_id = $1",
                &[&elder_id],
            )
            .await?
            .get(0);
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, name, dosage, instructions, created_at, updated_at
                FROM medications
                WHERE elder_id = $1
                ORDER BY name
                LIMIT $2 OFFSET $3
                "#,
                &[&elder_id, &pagination.limit(), &pagination.offset()],
            )
            .await?;
        
        let mut meds_with_schedules = Vec::new();
        for row in rows {
            let medication = row_to_medication(&row);
            let schedules = Self::get_schedules(pool, medication.id).await?;
            meds_with_schedules.push(MedicationWithSchedule { medication, schedules });
        }
        
        Ok(Paginated::new(meds_with_schedules, total, pagination))
    }
    
    /// List all medications for an elder (no pagination, for AI)
    pub async fn list_all_by_elder(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<Vec<MedicationWithSchedule>> {
        let client = pool.get().await?;
        
        let rows = client
            .query(
                r#"
                SELECT id, elder_id, name, dosage, instructions, created_at, updated_at
                FROM medications
                WHERE elder_id = $1
                ORDER BY name
                "#,
                &[&elder_id],
            )
            .await?;
        
        let mut meds_with_schedules = Vec::new();
        for row in rows {
            let medication = row_to_medication(&row);
            let schedules = Self::get_schedules(pool, medication.id).await?;
            meds_with_schedules.push(MedicationWithSchedule { medication, schedules });
        }
        
        Ok(meds_with_schedules)
    }
    
    /// Get all schedules for an elder (for reminder scheduling)
    pub async fn get_all_schedules_by_elder(
        pool: &PostgresPool,
        elder_id: Uuid,
    ) -> DomainResult<Vec<(Medication, MedicationSchedule)>> {
        let client = pool.get().await?;
        
        let rows = client
            .query(
                r#"
                SELECT m.id, m.elder_id, m.name, m.dosage, m.instructions, m.created_at, m.updated_at,
                       s.id as schedule_id, s.medication_id, s.time_of_day, s.days_pattern, s.created_at as schedule_created_at
                FROM medications m
                JOIN medication_schedules s ON m.id = s.medication_id
                WHERE m.elder_id = $1
                ORDER BY s.time_of_day
                "#,
                &[&elder_id],
            )
            .await?;
        
        let results: Vec<(Medication, MedicationSchedule)> = rows
            .iter()
            .map(|row| {
                let medication = Medication {
                    id: row.get("id"),
                    elder_id: row.get("elder_id"),
                    name: row.get("name"),
                    dosage: row.get("dosage"),
                    instructions: row.get("instructions"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                };
                let schedule = MedicationSchedule {
                    id: row.get("schedule_id"),
                    medication_id: row.get("medication_id"),
                    time_of_day: row.get("time_of_day"),
                    days_pattern: row.get("days_pattern"),
                    created_at: row.get("schedule_created_at"),
                };
                (medication, schedule)
            })
            .collect();
        
        Ok(results)
    }
    
    /// Update medication
    pub async fn update(
        pool: &PostgresPool,
        id: Uuid,
        req: &UpdateMedicationRequest,
    ) -> DomainResult<MedicationWithSchedule> {
        let current = Self::find_by_id(pool, id).await?;
        let client = pool.get().await?;
        let now = Utc::now();
        
        let name = req.name.as_ref().unwrap_or(&current.name);
        let dosage = req.dosage.as_ref().unwrap_or(&current.dosage);
        let instructions = req.instructions.clone().or(current.instructions);
        
        let row = client
            .query_one(
                r#"
                UPDATE medications 
                SET name = $2, dosage = $3, instructions = $4, updated_at = $5
                WHERE id = $1
                RETURNING id, elder_id, name, dosage, instructions, created_at, updated_at
                "#,
                &[&id, &name, &dosage, &instructions, &now],
            )
            .await?;
        
        let medication = row_to_medication(&row);
        
        // Update schedules if provided
        if let Some(schedule_times) = &req.schedule_times {
            // Delete existing schedules
            client
                .execute(
                    "DELETE FROM medication_schedules WHERE medication_id = $1",
                    &[&id],
                )
                .await?;
            
            // Create new schedules
            let mut schedules = Vec::new();
            for time_str in schedule_times {
                let time = NaiveTime::parse_from_str(time_str, "%H:%M")
                    .map_err(|_| DomainError::Validation(format!("Invalid time format: {}", time_str)))?;
                
                let schedule_id = Uuid::new_v4();
                let schedule_row = client
                    .query_one(
                        r#"
                        INSERT INTO medication_schedules (id, medication_id, time_of_day, days_pattern, created_at)
                        VALUES ($1, $2, $3, $4, $5)
                        RETURNING id, medication_id, time_of_day, days_pattern, created_at
                        "#,
                        &[&schedule_id, &id, &time, &"daily", &now],
                    )
                    .await?;
                
                schedules.push(row_to_schedule(&schedule_row));
            }
            
            return Ok(MedicationWithSchedule { medication, schedules });
        }
        
        let schedules = Self::get_schedules(pool, id).await?;
        Ok(MedicationWithSchedule { medication, schedules })
    }
    
    /// Delete medication
    pub async fn delete(pool: &PostgresPool, id: Uuid) -> DomainResult<()> {
        let client = pool.get().await?;
        
        // Delete schedules first (foreign key)
        client
            .execute(
                "DELETE FROM medication_schedules WHERE medication_id = $1",
                &[&id],
            )
            .await?;
        
        let rows_affected = client
            .execute("DELETE FROM medications WHERE id = $1", &[&id])
            .await?;
        
        if rows_affected == 0 {
            return Err(DomainError::NotFound("Medication not found".to_string()));
        }
        
        Ok(())
    }
    
    /// Count medications for an elder
    pub async fn count_by_elder(pool: &PostgresPool, elder_id: Uuid) -> DomainResult<i64> {
        let client = pool.get().await?;
        
        let count: i64 = client
            .query_one(
                "SELECT COUNT(*) FROM medications WHERE elder_id = $1",
                &[&elder_id],
            )
            .await?
            .get(0);
        
        Ok(count)
    }
}

fn row_to_medication(row: &tokio_postgres::Row) -> Medication {
    Medication {
        id: row.get("id"),
        elder_id: row.get("elder_id"),
        name: row.get("name"),
        dosage: row.get("dosage"),
        instructions: row.get("instructions"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn row_to_schedule(row: &tokio_postgres::Row) -> MedicationSchedule {
    MedicationSchedule {
        id: row.get("id"),
        medication_id: row.get("medication_id"),
        time_of_day: row.get("time_of_day"),
        days_pattern: row.get("days_pattern"),
        created_at: row.get("created_at"),
    }
}

