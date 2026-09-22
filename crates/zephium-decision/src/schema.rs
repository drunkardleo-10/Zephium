use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::Question;

fn object(properties: Map<String, Value>) -> Value {
    let required: Vec<_> = properties.keys().cloned().collect();
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(crate) fn answers(questions: &BTreeMap<String, Question>) -> Value {
    let number = json!({"type":"number","minimum":0,"maximum":1});
    let answers = questions
        .iter()
        .map(|(key, question)| {
            let mut properties = Map::new();
            match question {
                Question::Noul { .. } => {
                    properties.insert("type".into(), json!({"type":"string","enum":["noul"]}));
                    properties.insert("noul".into(), number.clone());
                }
                Question::Choice { criteria, .. } => {
                    properties.insert("type".into(), json!({"type":"string","enum":["choice"]}));
                    properties.insert(
                        "choice".into(),
                        json!({"type":"string","enum":criteria.keys().collect::<Vec<_>>()}),
                    );
                    properties.insert("confidence".into(), number.clone());
                    properties.insert(
                        "probabilities".into(),
                        object(
                            criteria
                                .keys()
                                .map(|key| (key.clone(), number.clone()))
                                .collect(),
                        ),
                    );
                }
                Question::Score { criteria, .. } => {
                    properties.insert("type".into(), json!({"type":"string","enum":["score"]}));
                    properties.insert(
                        "score".into(),
                        json!({"type":"number","minimum":0,"maximum":criteria.len()-1}),
                    );
                    properties.insert("confidence".into(), number.clone());
                    properties.insert(
                        "probabilities".into(),
                        object(
                            criteria
                                .iter()
                                .enumerate()
                                .map(|(i, _)| (i.to_string(), number.clone()))
                                .collect(),
                        ),
                    );
                    properties.insert(
                        "legend".into(),
                        object(
                            criteria
                                .iter()
                                .enumerate()
                                .map(|(i, level)| (i.to_string(), legend_schema(level)))
                                .collect(),
                        ),
                    );
                }
            }
            (key.clone(), object(properties))
        })
        .collect();
    object(Map::from_iter([("answers".into(), object(answers))]))
}

fn legend_schema(value: &Value) -> Value {
    match value {
        Value::Object(fields) => object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), legend_schema(value)))
                .collect(),
        ),
        Value::Array(values) => {
            let items: Vec<_> = values.iter().map(legend_schema).collect();
            let items = if items.is_empty() {
                json!({"type":"null"})
            } else {
                json!({"anyOf":items})
            };
            json!({"type":"array","items":items,"minItems":values.len(),"maxItems":values.len()})
        }
        Value::Null => json!({"type":"null"}),
        Value::Bool(_) => json!({"type":"boolean","enum":[value]}),
        Value::Number(_) => json!({"type":"number","enum":[value]}),
        Value::String(_) => json!({"type":"string","enum":[value]}),
    }
}
