//! Common types used across Basilica components

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Represents a geographic location profile with city, region, and country components
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationProfile {
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
}

impl LocationProfile {
    /// Create a new LocationProfile
    pub fn new(city: Option<String>, region: Option<String>, country: Option<String>) -> Self {
        Self {
            city,
            region,
            country,
        }
    }

    /// Create a LocationProfile with all components as None
    pub fn unknown() -> Self {
        Self {
            city: None,
            region: None,
            country: None,
        }
    }
}

impl FromStr for LocationProfile {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('/').collect();

        let city = parts.first().and_then(|c| {
            if c.is_empty() || *c == "Unknown" {
                None
            } else {
                Some(c.to_string())
            }
        });

        let region = parts.get(1).and_then(|r| {
            if r.is_empty() || *r == "Unknown" {
                None
            } else {
                Some(r.to_string())
            }
        });

        let country = parts.get(2).and_then(|c| {
            if c.is_empty() || *c == "Unknown" {
                None
            } else {
                Some(c.to_string())
            }
        });

        Ok(Self {
            city,
            region,
            country,
        })
    }
}

impl fmt::Display for LocationProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}/{}",
            self.city.as_deref().unwrap_or("Unknown"),
            self.region.as_deref().unwrap_or("Unknown"),
            self.country.as_deref().unwrap_or("Unknown")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_location_profile_from_str() {
        // Full location
        let location = LocationProfile::from_str("San Francisco/California/US").unwrap();
        assert_eq!(location.city, Some("San Francisco".to_string()));
        assert_eq!(location.region, Some("California".to_string()));
        assert_eq!(location.country, Some("US".to_string()));

        // Partial location with Unknown
        let location = LocationProfile::from_str("Unknown/California/US").unwrap();
        assert_eq!(location.city, None);
        assert_eq!(location.region, Some("California".to_string()));
        assert_eq!(location.country, Some("US".to_string()));

        // All Unknown
        let location = LocationProfile::from_str("Unknown/Unknown/Unknown").unwrap();
        assert_eq!(location.city, None);
        assert_eq!(location.region, None);
        assert_eq!(location.country, None);

        // Missing parts
        let location = LocationProfile::from_str("San Francisco").unwrap();
        assert_eq!(location.city, Some("San Francisco".to_string()));
        assert_eq!(location.region, None);
        assert_eq!(location.country, None);

        // Empty string
        let location = LocationProfile::from_str("").unwrap();
        assert_eq!(location.city, None);
        assert_eq!(location.region, None);
        assert_eq!(location.country, None);
    }

    #[test]
    fn test_location_profile_display() {
        let location = LocationProfile::new(
            Some("San Francisco".to_string()),
            Some("California".to_string()),
            Some("US".to_string()),
        );
        assert_eq!(location.to_string(), "San Francisco/California/US");

        let location =
            LocationProfile::new(None, Some("California".to_string()), Some("US".to_string()));
        assert_eq!(location.to_string(), "Unknown/California/US");

        let location = LocationProfile::unknown();
        assert_eq!(location.to_string(), "Unknown/Unknown/Unknown");

        let location =
            LocationProfile::new(Some("Tokyo".to_string()), None, Some("JP".to_string()));
        assert_eq!(location.to_string(), "Tokyo/Unknown/JP");
    }

    #[test]
    fn test_location_profile_roundtrip() {
        let original = "San Francisco/California/US";
        let location = LocationProfile::from_str(original).unwrap();
        assert_eq!(location.to_string(), original);

        let original_with_unknown = "Unknown/California/US";
        let location = LocationProfile::from_str(original_with_unknown).unwrap();
        assert_eq!(location.to_string(), original_with_unknown);
    }
}
