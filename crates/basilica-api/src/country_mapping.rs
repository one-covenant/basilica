//! Country name to ISO 3166-1 alpha-2 code mapping

use std::collections::HashMap;

use once_cell::sync::Lazy;

/// Convert a country name or code to its ISO 3166-1 alpha-2 code
/// Returns the input unchanged if no mapping is found (assumes it's already a code)
pub fn normalize_country_code(input: &str) -> String {
    let input_lower = input.to_lowercase();

    // Try to find in our mapping first (handles special cases like "UK" -> "GB")
    if let Some(code) = COUNTRY_MAPPINGS.get(input_lower.as_str()) {
        return code.to_string();
    }

    // If it's a 2-letter code not in our mapping, just return uppercase version
    if input.len() == 2 {
        return input.to_uppercase();
    }

    // Return the input uppercased if not found
    input.to_uppercase()
}

static COUNTRY_MAPPINGS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // Full country names (lowercase)
    m.insert("afghanistan", "AF");
    m.insert("albania", "AL");
    m.insert("algeria", "DZ");
    m.insert("andorra", "AD");
    m.insert("angola", "AO");
    m.insert("antigua and barbuda", "AG");
    m.insert("argentina", "AR");
    m.insert("armenia", "AM");
    m.insert("australia", "AU");
    m.insert("austria", "AT");
    m.insert("azerbaijan", "AZ");
    m.insert("bahamas", "BS");
    m.insert("bahrain", "BH");
    m.insert("bangladesh", "BD");
    m.insert("barbados", "BB");
    m.insert("belarus", "BY");
    m.insert("belgium", "BE");
    m.insert("belize", "BZ");
    m.insert("benin", "BJ");
    m.insert("bhutan", "BT");
    m.insert("bolivia", "BO");
    m.insert("bosnia and herzegovina", "BA");
    m.insert("botswana", "BW");
    m.insert("brazil", "BR");
    m.insert("brunei", "BN");
    m.insert("bulgaria", "BG");
    m.insert("burkina faso", "BF");
    m.insert("burundi", "BI");
    m.insert("cambodia", "KH");
    m.insert("cameroon", "CM");
    m.insert("canada", "CA");
    m.insert("cape verde", "CV");
    m.insert("central african republic", "CF");
    m.insert("chad", "TD");
    m.insert("chile", "CL");
    m.insert("china", "CN");
    m.insert("colombia", "CO");
    m.insert("comoros", "KM");
    m.insert("congo", "CG");
    m.insert("congo, democratic republic", "CD");
    m.insert("democratic republic of the congo", "CD");
    m.insert("costa rica", "CR");
    m.insert("croatia", "HR");
    m.insert("cuba", "CU");
    m.insert("cyprus", "CY");
    m.insert("czech republic", "CZ");
    m.insert("czechia", "CZ");
    m.insert("denmark", "DK");
    m.insert("djibouti", "DJ");
    m.insert("dominica", "DM");
    m.insert("dominican republic", "DO");
    m.insert("east timor", "TL");
    m.insert("ecuador", "EC");
    m.insert("egypt", "EG");
    m.insert("el salvador", "SV");
    m.insert("equatorial guinea", "GQ");
    m.insert("eritrea", "ER");
    m.insert("estonia", "EE");
    m.insert("ethiopia", "ET");
    m.insert("fiji", "FJ");
    m.insert("finland", "FI");
    m.insert("france", "FR");
    m.insert("gabon", "GA");
    m.insert("gambia", "GM");
    m.insert("georgia", "GE");
    m.insert("germany", "DE");
    m.insert("ghana", "GH");
    m.insert("greece", "GR");
    m.insert("grenada", "GD");
    m.insert("guatemala", "GT");
    m.insert("guinea", "GN");
    m.insert("guinea-bissau", "GW");
    m.insert("guyana", "GY");
    m.insert("haiti", "HT");
    m.insert("honduras", "HN");
    m.insert("hungary", "HU");
    m.insert("iceland", "IS");
    m.insert("india", "IN");
    m.insert("indonesia", "ID");
    m.insert("iran", "IR");
    m.insert("iraq", "IQ");
    m.insert("ireland", "IE");
    m.insert("israel", "IL");
    m.insert("italy", "IT");
    m.insert("ivory coast", "CI");
    m.insert("jamaica", "JM");
    m.insert("japan", "JP");
    m.insert("jordan", "JO");
    m.insert("kazakhstan", "KZ");
    m.insert("kenya", "KE");
    m.insert("kiribati", "KI");
    m.insert("korea, north", "KP");
    m.insert("korea, south", "KR");
    m.insert("north korea", "KP");
    m.insert("south korea", "KR");
    m.insert("kosovo", "XK");
    m.insert("kuwait", "KW");
    m.insert("kyrgyzstan", "KG");
    m.insert("laos", "LA");
    m.insert("latvia", "LV");
    m.insert("lebanon", "LB");
    m.insert("lesotho", "LS");
    m.insert("liberia", "LR");
    m.insert("libya", "LY");
    m.insert("liechtenstein", "LI");
    m.insert("lithuania", "LT");
    m.insert("luxembourg", "LU");
    m.insert("macedonia", "MK");
    m.insert("north macedonia", "MK");
    m.insert("madagascar", "MG");
    m.insert("malawi", "MW");
    m.insert("malaysia", "MY");
    m.insert("maldives", "MV");
    m.insert("mali", "ML");
    m.insert("malta", "MT");
    m.insert("marshall islands", "MH");
    m.insert("mauritania", "MR");
    m.insert("mauritius", "MU");
    m.insert("mexico", "MX");
    m.insert("micronesia", "FM");
    m.insert("moldova", "MD");
    m.insert("monaco", "MC");
    m.insert("mongolia", "MN");
    m.insert("montenegro", "ME");
    m.insert("morocco", "MA");
    m.insert("mozambique", "MZ");
    m.insert("myanmar", "MM");
    m.insert("burma", "MM");
    m.insert("namibia", "NA");
    m.insert("nauru", "NR");
    m.insert("nepal", "NP");
    m.insert("netherlands", "NL");
    m.insert("holland", "NL");
    m.insert("new zealand", "NZ");
    m.insert("nicaragua", "NI");
    m.insert("niger", "NE");
    m.insert("nigeria", "NG");
    m.insert("norway", "NO");
    m.insert("oman", "OM");
    m.insert("pakistan", "PK");
    m.insert("palau", "PW");
    m.insert("palestine", "PS");
    m.insert("panama", "PA");
    m.insert("papua new guinea", "PG");
    m.insert("paraguay", "PY");
    m.insert("peru", "PE");
    m.insert("philippines", "PH");
    m.insert("poland", "PL");
    m.insert("portugal", "PT");
    m.insert("qatar", "QA");
    m.insert("romania", "RO");
    m.insert("russia", "RU");
    m.insert("rwanda", "RW");
    m.insert("saint kitts and nevis", "KN");
    m.insert("saint lucia", "LC");
    m.insert("saint vincent and the grenadines", "VC");
    m.insert("samoa", "WS");
    m.insert("san marino", "SM");
    m.insert("sao tome and principe", "ST");
    m.insert("saudi arabia", "SA");
    m.insert("senegal", "SN");
    m.insert("serbia", "RS");
    m.insert("seychelles", "SC");
    m.insert("sierra leone", "SL");
    m.insert("singapore", "SG");
    m.insert("slovakia", "SK");
    m.insert("slovenia", "SI");
    m.insert("solomon islands", "SB");
    m.insert("somalia", "SO");
    m.insert("south africa", "ZA");
    m.insert("south sudan", "SS");
    m.insert("spain", "ES");
    m.insert("sri lanka", "LK");
    m.insert("sudan", "SD");
    m.insert("suriname", "SR");
    m.insert("swaziland", "SZ");
    m.insert("eswatini", "SZ");
    m.insert("sweden", "SE");
    m.insert("switzerland", "CH");
    m.insert("syria", "SY");
    m.insert("taiwan", "TW");
    m.insert("tajikistan", "TJ");
    m.insert("tanzania", "TZ");
    m.insert("thailand", "TH");
    m.insert("timor-leste", "TL");
    m.insert("togo", "TG");
    m.insert("tonga", "TO");
    m.insert("trinidad and tobago", "TT");
    m.insert("tunisia", "TN");
    m.insert("turkey", "TR");
    m.insert("turkmenistan", "TM");
    m.insert("tuvalu", "TV");
    m.insert("uganda", "UG");
    m.insert("ukraine", "UA");
    m.insert("united arab emirates", "AE");
    m.insert("uae", "AE");
    m.insert("united kingdom", "GB");
    m.insert("uk", "GB");
    m.insert("great britain", "GB");
    m.insert("england", "GB");
    m.insert("united states", "US");
    m.insert("usa", "US");
    m.insert("united states of america", "US");
    m.insert("america", "US");
    m.insert("uruguay", "UY");
    m.insert("uzbekistan", "UZ");
    m.insert("vanuatu", "VU");
    m.insert("vatican city", "VA");
    m.insert("vatican", "VA");
    m.insert("venezuela", "VE");
    m.insert("vietnam", "VN");
    m.insert("yemen", "YE");
    m.insert("zambia", "ZM");
    m.insert("zimbabwe", "ZW");

    m
});
