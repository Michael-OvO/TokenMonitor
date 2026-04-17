use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FloatBallExpandDirection {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FloatBallLayout {
    pub expand_direction: FloatBallExpandDirection,
}

fn main() {
    let layout = FloatBallLayout {
        expand_direction: FloatBallExpandDirection::Left,
    };
    println!("{}", serde_json::to_string(&layout).unwrap());
}
