use std::collections::HashMap;

fn main() {
    let mut unordered = HashMap::new();
    unordered.insert("key", 1_u8);
    let _ = 0.5_f64.sin() + f64::from(*unordered.get("key").unwrap());
}
