//! Ownership validation extractor for rental resources
//!
//! This extractor validates that the authenticated user owns the requested rental
//! before allowing access to rental-specific endpoints.

use axum::{
    async_trait,
    extract::{FromRequestParts, Path},
    http::{request::Parts, StatusCode},
};
use sqlx::PgPool;
use tracing::{debug, warn};

use crate::{api::middleware::Auth0Claims, server::AppState};

/// Extractor that validates rental ownership
///
/// This extractor ensures that the authenticated user owns the requested rental.
/// If the user doesn't own the rental, it returns 404 Not Found to avoid leaking
/// information about the existence of rentals owned by other users.
pub struct OwnedRental {
    pub rental_id: String,
    pub user_id: String,
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

        // Check ownership in the database
        let owns_rental = check_rental_ownership(&state.db, &rental_id, &user_id)
            .await
            .map_err(|e| {
                warn!("Database error checking rental ownership: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        if !owns_rental {
            warn!(
                "User {} attempted to access rental {} which they don't own",
                user_id, rental_id
            );
            // Return 404 to avoid leaking information about rental existence
            return Err(StatusCode::NOT_FOUND);
        }

        debug!(
            "User {} authorized to access their rental {}",
            user_id, rental_id
        );

        Ok(OwnedRental { rental_id, user_id })
    }
}

/// Check if a user owns a specific rental
async fn check_rental_ownership(
    db: &PgPool,
    rental_id: &str,
    user_id: &str,
) -> Result<bool, sqlx::Error> {
    let exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS(SELECT 1 FROM user_rentals WHERE rental_id = $1 AND user_id = $2)
        "#,
    )
    .bind(rental_id)
    .bind(user_id)
    .fetch_one(db)
    .await?;

    Ok(exists)
}

/// Helper function to extract Auth0 claims from request parts
fn get_auth0_claims_from_parts(parts: &Parts) -> Option<&Auth0Claims> {
    parts.extensions.get::<Auth0Claims>()
}

/// Store a new rental ownership record
pub async fn store_rental_ownership(
    db: &PgPool,
    rental_id: &str,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO user_rentals (rental_id, user_id)
        VALUES ($1, $2)
        "#,
    )
    .bind(rental_id)
    .bind(user_id)
    .execute(db)
    .await?;

    debug!(
        "Stored ownership record for rental {} owned by user {}",
        rental_id, user_id
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

        // Initially, user should not own the rental
        assert!(!check_rental_ownership(&db, rental_id, user_id)
            .await
            .expect("Failed to check ownership"));

        // Store ownership
        store_rental_ownership(&db, rental_id, user_id)
            .await
            .expect("Failed to store ownership");

        // Now user should own the rental
        assert!(check_rental_ownership(&db, rental_id, user_id)
            .await
            .expect("Failed to check ownership"));

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
        assert!(!check_rental_ownership(&db, rental_id, user_id)
            .await
            .expect("Failed to check ownership"));
    }
}
