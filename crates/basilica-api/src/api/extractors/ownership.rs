//! Ownership validation extractor for rental resources
//!
//! This extractor validates that the authenticated user owns the requested rental
//! before allowing access to rental-specific endpoints.

use axum::{
    async_trait,
    extract::{FromRequestParts, Path},
    http::{request::Parts, StatusCode},
};
use sqlx::{FromRow, PgPool};
use tracing::{debug, warn};

use crate::{api::middleware::Auth0Claims, server::AppState};

/// Database row structure for user_rentals table
#[derive(Debug, FromRow)]
struct UserRentalRow {
    rental_id: String,
    user_id: String,
    ssh_credentials: Option<String>,
    #[allow(dead_code)]
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Extractor that validates rental ownership
///
/// This extractor ensures that the authenticated user owns the requested rental.
/// If the user doesn't own the rental, it returns 404 Not Found to avoid leaking
/// information about the existence of rentals owned by other users.
pub struct OwnedRental {
    pub rental_id: String,
    pub user_id: String,
    pub ssh_credentials: Option<String>,
}

#[async_trait]
impl FromRequestParts<AppState> for OwnedRental {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Extract the rental ID from the path
        let Path(rental_id): Path<String> = Path::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?;

        // Get the authenticated user's claims
        let claims = get_auth0_claims_from_parts(parts).ok_or(StatusCode::UNAUTHORIZED)?;

        let user_id = claims.sub.clone();

        // Get rental ownership details from the database
        let rental_row = get_rental_ownership(&state.db, &rental_id, &user_id)
            .await
            .map_err(|e| {
                warn!("Database error checking rental ownership: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        match rental_row {
            Some(row) => {
                debug!(
                    "User {} authorized to access their rental {}",
                    user_id, rental_id
                );
                Ok(OwnedRental {
                    rental_id: row.rental_id,
                    user_id: row.user_id,
                    ssh_credentials: row.ssh_credentials,
                })
            }
            None => {
                warn!(
                    "User {} attempted to access rental {} which they don't own",
                    user_id, rental_id
                );
                // Return 404 to avoid leaking information about rental existence
                Err(StatusCode::NOT_FOUND)
            }
        }
    }
}

/// Get rental ownership details if user owns the rental
async fn get_rental_ownership(
    db: &PgPool,
    rental_id: &str,
    user_id: &str,
) -> Result<Option<UserRentalRow>, sqlx::Error> {
    let row = sqlx::query_as::<_, UserRentalRow>(
        r#"
        SELECT rental_id, user_id, ssh_credentials, created_at
        FROM user_rentals 
        WHERE rental_id = $1 AND user_id = $2
        "#,
    )
    .bind(rental_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    Ok(row)
}

/// Helper function to extract Auth0 claims from request parts
fn get_auth0_claims_from_parts(parts: &Parts) -> Option<&Auth0Claims> {
    parts.extensions.get::<Auth0Claims>()
}

/// Store a new rental ownership record with optional SSH credentials
pub async fn store_rental_ownership(
    db: &PgPool,
    rental_id: &str,
    user_id: &str,
    ssh_credentials: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO user_rentals (rental_id, user_id, ssh_credentials)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(rental_id)
    .bind(user_id)
    .bind(ssh_credentials)
    .execute(db)
    .await?;

    debug!(
        "Stored ownership record for rental {} owned by user {} (SSH: {})",
        rental_id,
        user_id,
        ssh_credentials.is_some()
    );

    Ok(())
}

/// Get all rentals owned by a specific user
pub async fn get_user_rental_ids(db: &PgPool, user_id: &str) -> Result<Vec<String>, sqlx::Error> {
    let records: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT rental_id
        FROM user_rentals
        WHERE user_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok(records.into_iter().map(|(rental_id,)| rental_id).collect())
}

/// Delete a rental ownership record (for cleanup when rental is stopped)
pub async fn delete_rental_ownership(db: &PgPool, rental_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        DELETE FROM user_rentals
        WHERE rental_id = $1
        "#,
    )
    .bind(rental_id)
    .execute(db)
    .await?;

    debug!("Deleted ownership record for rental {}", rental_id);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires PostgreSQL to be running
    async fn test_rental_ownership_crud() {
        // Connect to test PostgreSQL database
        // This test requires DATABASE_URL to be set or PostgreSQL running locally
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://basilica:dev@localhost:5432/basilica_test".to_string());

        let db = PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to test database");

        // Run migration to create tables
        sqlx::migrate!("./migrations")
            .run(&db)
            .await
            .expect("Failed to run migrations");

        let rental_id = "test-rental-123";
        let user_id = "user-456";
        let ssh_creds = Some("ssh user@host -p 22");

        // Initially, user should not own the rental
        assert!(get_rental_ownership(&db, rental_id, user_id)
            .await
            .expect("Failed to check ownership")
            .is_none());

        // Store ownership with SSH credentials
        store_rental_ownership(&db, rental_id, user_id, ssh_creds)
            .await
            .expect("Failed to store ownership");

        // Now user should own the rental and have SSH credentials
        let ownership = get_rental_ownership(&db, rental_id, user_id)
            .await
            .expect("Failed to check ownership");
        assert!(ownership.is_some());
        let row = ownership.unwrap();
        assert_eq!(row.ssh_credentials, ssh_creds.map(String::from));

        // Get user's rentals
        let rentals = get_user_rental_ids(&db, user_id)
            .await
            .expect("Failed to get user rentals");
        assert_eq!(rentals, vec![rental_id]);

        // Delete ownership
        delete_rental_ownership(&db, rental_id)
            .await
            .expect("Failed to delete ownership");

        // User should no longer own the rental
        assert!(get_rental_ownership(&db, rental_id, user_id)
            .await
            .expect("Failed to check ownership")
            .is_none());
    }
}
