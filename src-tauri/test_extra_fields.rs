use serde::{Deserialize};

#[derive(Deserialize, Debug)]
struct Record {
    source_key: String,
    name: String,
}

fn main() {
    // Test 1: Normal case
    let json1 = r#"{"source_key":"test","name":"John"}"#;
    let result1: Result<Record, _> = serde_json::from_str(json1);
    println!("Test 1 (normal): {:?}", result1);

    // Test 2: Extra field
    let json2 = r#"{"source_key":"test","name":"John","extra":"field"}"#;
    let result2: Result<Record, _> = serde_json::from_str(json2);
    println!("Test 2 (extra field): {:?}", result2);

    // Test 3: Extra top-level field like format
    let json3 = r#"{"source_key":"test","name":"John","format":"something"}"#;
    let result3: Result<Record, _> = serde_json::from_str(json3);
    println!("Test 3 (format field): {:?}", result3);
}
