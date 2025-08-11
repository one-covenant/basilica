use crate::bdd::TestContext;
use basilica_protocol::billing::{
    ApplyCreditsRequest, GetBalanceRequest, ReleaseReservationRequest, ReserveCreditsRequest,
};
use uuid::Uuid;

#[tokio::test]
async fn test_apply_credits_increases_balance() {
    let mut context = TestContext::new().await;
    let user_id = "test_apply_credits_001";

    context.create_test_user(user_id, "100.0").await;

    let initial_balance = context.get_user_balance(user_id).await;
    assert_eq!(initial_balance, rust_decimal::Decimal::from(100));

    let transaction_id = Uuid::new_v4().to_string();
    let request = ApplyCreditsRequest {
        payment_method: String::new(),
        user_id: user_id.to_string(),
        amount: "50.0".to_string(),
        transaction_id: transaction_id.clone(),
        metadata: std::collections::HashMap::new(),
    };

    let response = context
        .client
        .apply_credits(request)
        .await
        .expect("Failed to apply credits")
        .into_inner();

    assert!(response.success, "Applying credits should succeed");
    assert_eq!(
        response.new_balance, "150",
        "New balance should be 100 + 50"
    );
    assert_eq!(
        response.credit_id, transaction_id,
        "Transaction ID should match"
    );
    assert!(response.applied_at.is_some(), "Should return timestamp");

    let final_balance = context.get_user_balance(user_id).await;
    assert_eq!(
        final_balance,
        rust_decimal::Decimal::from(150),
        "Database balance should be updated"
    );

    context.cleanup().await;
}

#[tokio::test]
async fn test_apply_negative_credits_reduces_balance() {
    let mut context = TestContext::new().await;
    let user_id = "test_negative_credits";

    context.create_test_user(user_id, "200.0").await;

    let request = ApplyCreditsRequest {
        payment_method: String::new(),
        user_id: user_id.to_string(),
        amount: "-30.0".to_string(),
        transaction_id: Uuid::new_v4().to_string(),
        metadata: std::collections::HashMap::new(),
    };

    let response = context
        .client
        .apply_credits(request)
        .await
        .expect("Failed to apply negative credits")
        .into_inner();

    assert!(response.success);
    assert_eq!(response.new_balance, "170", "Balance should be 200 - 30");

    let final_balance = context.get_user_balance(user_id).await;
    assert_eq!(final_balance, rust_decimal::Decimal::from(170));

    context.cleanup().await;
}

#[tokio::test]
async fn test_get_balance_returns_correct_amounts() {
    let mut context = TestContext::new().await;
    let user_id = "test_get_balance";

    context.create_test_user(user_id, "500.0").await;

    let reserve_request = ReserveCreditsRequest {
        user_id: user_id.to_string(),
        amount: "150.0".to_string(),
        duration_hours: 24,
        rental_id: String::new(),
    };

    context
        .client
        .reserve_credits(reserve_request)
        .await
        .expect("Failed to reserve credits");

    let request = GetBalanceRequest {
        user_id: user_id.to_string(),
    };

    let response = context
        .client
        .get_balance(request)
        .await
        .expect("Failed to get balance")
        .into_inner();

    assert_eq!(response.total_balance, "500", "Total balance should be 500");
    assert_eq!(
        response.reserved_balance, "150",
        "Reserved balance should be 150"
    );
    assert_eq!(
        response.available_balance, "350",
        "Available should be 500 - 150"
    );
    assert!(
        response.last_updated.is_some(),
        "Should have update timestamp"
    );

    context.cleanup().await;
}

#[tokio::test]
async fn test_reserve_credits_blocks_amount() {
    let mut context = TestContext::new().await;
    let user_id = "test_reserve_credits";

    context.create_test_user(user_id, "1000.0").await;

    let request = ReserveCreditsRequest {
        user_id: user_id.to_string(),
        amount: "300.0".to_string(),
        duration_hours: 48,
        rental_id: Uuid::new_v4().to_string(),
    };

    let response = context
        .client
        .reserve_credits(request)
        .await
        .expect("Failed to reserve credits")
        .into_inner();

    assert!(response.success, "Reservation should succeed");
    assert!(
        !response.reservation_id.is_empty(),
        "Should return reservation ID"
    );
    assert_eq!(response.reserved_amount, "300.0");
    assert!(
        response.reserved_until.is_some(),
        "Should have expiration time"
    );

    let reserved = context.get_reserved_balance(user_id).await;
    assert_eq!(
        reserved,
        rust_decimal::Decimal::from(300),
        "Reserved balance should be 300"
    );

    assert!(
        context.reservation_exists(&response.reservation_id).await,
        "Reservation should exist in database"
    );

    context.cleanup().await;
}

#[tokio::test]
async fn test_reserve_credits_fails_when_insufficient_balance() {
    let mut context = TestContext::new().await;
    let user_id = "test_insufficient_reserve";

    context.create_test_user(user_id, "100.0").await;

    let request = ReserveCreditsRequest {
        user_id: user_id.to_string(),
        amount: "500.0".to_string(),
        duration_hours: 10,
        rental_id: String::new(),
    };

    let result = context.client.reserve_credits(request).await;

    assert!(result.is_err(), "Should fail with insufficient balance");
    let error = result.unwrap_err();
    assert!(error.message().contains("Insufficient balance"));

    let reserved = context.get_reserved_balance(user_id).await;
    assert_eq!(
        reserved,
        rust_decimal::Decimal::ZERO,
        "No amount should be reserved"
    );

    context.cleanup().await;
}

#[tokio::test]
async fn test_release_reservation_charges_and_refunds() {
    let mut context = TestContext::new().await;
    let user_id = "test_release_reservation";

    context.create_test_user(user_id, "1000.0").await;

    let reserve_request = ReserveCreditsRequest {
        user_id: user_id.to_string(),
        amount: "200.0".to_string(),
        duration_hours: 24,
        rental_id: String::new(),
    };

    let reserve_response = context
        .client
        .reserve_credits(reserve_request)
        .await
        .expect("Failed to reserve credits")
        .into_inner();

    let initial_balance = context.get_user_balance(user_id).await;
    let initial_reserved = context.get_reserved_balance(user_id).await;
    assert_eq!(initial_reserved, rust_decimal::Decimal::from(200));

    let release_request = ReleaseReservationRequest {
        reservation_id: reserve_response.reservation_id,
        final_amount: "75.0".to_string(),
    };

    let release_response = context
        .client
        .release_reservation(release_request)
        .await
        .expect("Failed to release reservation")
        .into_inner();

    assert!(release_response.success, "Release should succeed");
    assert_eq!(release_response.charged_amount, "75", "Should charge 75");
    assert_eq!(
        release_response.refunded_amount, "125",
        "Should refund 125 (200 - 75)"
    );

    let final_balance = context.get_user_balance(user_id).await;
    let expected_balance = initial_balance - rust_decimal::Decimal::from(75);
    assert_eq!(
        final_balance, expected_balance,
        "Balance should be reduced by 75"
    );

    let final_reserved = context.get_reserved_balance(user_id).await;
    assert_eq!(
        final_reserved,
        rust_decimal::Decimal::ZERO,
        "Reserved should be cleared"
    );

    context.cleanup().await;
}

#[tokio::test]
async fn test_release_nonexistent_reservation_fails() {
    let mut context = TestContext::new().await;
    let user_id = "test_invalid_release";

    context.create_test_user(user_id, "1000.0").await;

    let fake_reservation_id = Uuid::new_v4().to_string();
    let request = ReleaseReservationRequest {
        reservation_id: fake_reservation_id,
        final_amount: "50.0".to_string(),
    };

    let result = context.client.release_reservation(request).await;

    assert!(result.is_err(), "Should fail for nonexistent reservation");
    let error = result.unwrap_err();
    assert!(error.message().contains("not found") || error.message().contains("Reservation"));

    context.cleanup().await;
}

#[tokio::test]
async fn test_multiple_reservations_tracked_correctly() {
    let mut context = TestContext::new().await;
    let user_id = "test_multiple_reservations";

    context.create_test_user(user_id, "2000.0").await;

    let mut reservation_ids = Vec::new();
    let amounts = vec!["100.0", "200.0", "300.0"];

    for amount in &amounts {
        let request = ReserveCreditsRequest {
            user_id: user_id.to_string(),
            amount: amount.to_string(),
            duration_hours: 12,
            rental_id: String::new(),
        };

        let response = context
            .client
            .reserve_credits(request)
            .await
            .expect("Failed to reserve credits")
            .into_inner();

        reservation_ids.push(response.reservation_id);
    }

    let total_reserved = context.get_reserved_balance(user_id).await;
    assert_eq!(
        total_reserved,
        rust_decimal::Decimal::from(600),
        "Total reserved should be 100 + 200 + 300"
    );

    let balance_response = context
        .client
        .get_balance(GetBalanceRequest {
            user_id: user_id.to_string(),
        })
        .await
        .expect("Failed to get balance")
        .into_inner();

    assert_eq!(
        balance_response.available_balance, "1400",
        "Available should be 2000 - 600"
    );

    let release_request = ReleaseReservationRequest {
        reservation_id: reservation_ids[1].clone(),
        final_amount: "150.0".to_string(),
    };

    context
        .client
        .release_reservation(release_request)
        .await
        .expect("Failed to release reservation");

    let updated_reserved = context.get_reserved_balance(user_id).await;
    assert_eq!(
        updated_reserved,
        rust_decimal::Decimal::from(400),
        "Reserved should be 600 - 200"
    );

    context.cleanup().await;
}

#[tokio::test]
async fn test_decimal_precision_preserved() {
    let mut context = TestContext::new().await;
    let user_id = "test_decimal_precision";

    context.create_test_user(user_id, "999.99").await;

    let request = ApplyCreditsRequest {
        payment_method: String::new(),
        user_id: user_id.to_string(),
        amount: "0.01".to_string(),
        transaction_id: Uuid::new_v4().to_string(),
        metadata: std::collections::HashMap::new(),
    };

    let response = context
        .client
        .apply_credits(request)
        .await
        .expect("Failed to apply credits")
        .into_inner();

    assert_eq!(
        response.new_balance, "1000",
        "Should handle decimal precision correctly"
    );

    let balance = context.get_user_balance(user_id).await;
    assert_eq!(balance, rust_decimal::Decimal::from(1000));

    let reserve_request = ReserveCreditsRequest {
        user_id: user_id.to_string(),
        amount: "333.33".to_string(),
        duration_hours: 8,
        rental_id: String::new(),
    };

    let reserve_response = context
        .client
        .reserve_credits(reserve_request)
        .await
        .expect("Failed to reserve credits")
        .into_inner();

    assert_eq!(reserve_response.reserved_amount, "333.33");

    let get_balance = context
        .client
        .get_balance(GetBalanceRequest {
            user_id: user_id.to_string(),
        })
        .await
        .expect("Failed to get balance")
        .into_inner();

    assert_eq!(
        get_balance.available_balance, "666.67",
        "Should maintain precision in calculations"
    );

    context.cleanup().await;
}
