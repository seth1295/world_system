//! Generic evaluation of the embedded projection of V1 capability schemas.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

const CONTRACTS: &str = include_str!("../../../schema/capability_contracts.v1.json");

pub(crate) struct Definitions {
    pub(crate) contracts: BTreeMap<String, Value>,
    pub(crate) numeric_ids: BTreeMap<String, u16>,
}

pub(crate) fn definitions() -> Result<Definitions, ()> {
    let document: Value = serde_json::from_str(CONTRACTS).map_err(|_| ())?;
    if document.get("schema").and_then(Value::as_str) != Some("veyra.capability_contracts/1") {
        return Err(());
    }
    let contracts = document.get("capabilities").and_then(Value::as_object).ok_or(())?;
    let contracts: BTreeMap<String, Value> =
        contracts.iter().map(|(id, contract)| (id.clone(), contract.clone())).collect();
    let numeric_ids = document
        .get("numeric_ids")
        .and_then(Value::as_object)
        .ok_or(())?
        .iter()
        .map(|(id, value)| {
            let numeric_id = value.as_u64().ok_or(())?.try_into().map_err(|_| ())?;
            if numeric_id == 0 {
                return Err(());
            }
            Ok((id.clone(), numeric_id))
        })
        .collect::<Result<BTreeMap<String, u16>, ()>>()?;
    if contracts.keys().any(|id| !numeric_ids.contains_key(id)) {
        return Err(());
    }
    Ok(Definitions { contracts, numeric_ids })
}

pub(crate) fn validate_dependency_graph(
    requirements: &BTreeMap<String, Vec<String>>,
) -> Result<(), ()> {
    for required in requirements.values().flatten() {
        if !requirements.contains_key(required) {
            return Err(());
        }
    }
    let mut complete = BTreeSet::new();
    let mut active = BTreeSet::new();
    for id in requirements.keys() {
        visit_dependency(id, requirements, &mut active, &mut complete)?;
    }
    Ok(())
}

fn visit_dependency(
    id: &str,
    requirements: &BTreeMap<String, Vec<String>>,
    active: &mut BTreeSet<String>,
    complete: &mut BTreeSet<String>,
) -> Result<(), ()> {
    if complete.contains(id) {
        return Ok(());
    }
    if !active.insert(id.to_owned()) {
        return Err(());
    }
    for required in requirements.get(id).ok_or(())? {
        visit_dependency(required, requirements, active, complete)?;
    }
    active.remove(id);
    complete.insert(id.to_owned());
    Ok(())
}

pub(crate) fn validate_instance(instance: &Value, schema: &Value) -> Result<(), ()> {
    let schema = schema.as_object().ok_or(())?;
    for keyword in schema.keys() {
        if !matches!(
            keyword.as_str(),
            "type"
                | "required"
                | "properties"
                | "additionalProperties"
                | "enum"
                | "const"
                | "items"
                | "title"
                | "description"
        ) && !keyword.starts_with("x-")
        {
            return Err(());
        }
    }
    if let Some(expected) = schema.get("type") {
        let valid = match expected {
            Value::String(kind) => matches_type(instance, kind),
            Value::Array(kinds) => kinds
                .iter()
                .any(|kind| kind.as_str().is_some_and(|kind| matches_type(instance, kind))),
            _ => return Err(()),
        };
        if !valid {
            return Err(());
        }
    }
    if let Some(expected) = schema.get("const")
        && instance != expected
    {
        return Err(());
    }
    if let Some(values) = schema.get("enum") {
        let values = values.as_array().ok_or(())?;
        if !values.contains(instance) {
            return Err(());
        }
    }
    if let Some(properties) = instance.as_object() {
        if let Some(required) = schema.get("required") {
            for name in required.as_array().ok_or(())? {
                let name = name.as_str().ok_or(())?;
                if !properties.contains_key(name) {
                    return Err(());
                }
            }
        }
        let property_schemas =
            schema.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
        for (name, value) in properties {
            if let Some(property_schema) = property_schemas.get(name) {
                validate_instance(value, property_schema)?;
            } else if let Some(additional) = schema.get("additionalProperties") {
                match additional {
                    Value::Bool(true) => {}
                    Value::Bool(false) => return Err(()),
                    Value::Object(_) => validate_instance(value, additional)?,
                    _ => return Err(()),
                }
            }
        }
    }
    if let (Value::Array(items), Some(item_schema)) = (instance, schema.get("items")) {
        for item in items {
            validate_instance(item, item_schema)?;
        }
    }
    Ok(())
}

fn matches_type(value: &Value, kind: &str) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "null" => value.is_null(),
        _ => false,
    }
}

pub(crate) fn reference_annotations(schema: &Value) -> Result<BTreeMap<String, String>, ()> {
    let properties = schema.get("properties").and_then(Value::as_object).ok_or(())?;
    let mut references = BTreeMap::new();
    for (name, property) in properties {
        if let Some(target) = property.get("x-veyra-reference") {
            let target = target.as_str().ok_or(())?;
            references.insert(name.clone(), target.to_owned());
        }
    }
    Ok(references)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{definitions, validate_dependency_graph, validate_instance};
    use serde_json::json;

    #[test]
    fn dependency_graph_accepts_closure_and_refuses_missing_edges_and_cycles() {
        assert!(
            validate_dependency_graph(&BTreeMap::from([
                ("base".to_owned(), vec![]),
                ("middle".to_owned(), vec!["base".to_owned()]),
                ("leaf".to_owned(), vec!["middle".to_owned()]),
            ]))
            .is_ok()
        );
        assert!(
            validate_dependency_graph(&BTreeMap::from([(
                "leaf".to_owned(),
                vec!["missing".to_owned()],
            )]))
            .is_err()
        );
        assert!(
            validate_dependency_graph(&BTreeMap::from([
                ("a".to_owned(), vec!["b".to_owned()]),
                ("b".to_owned(), vec!["a".to_owned()]),
            ]))
            .is_err()
        );
    }

    #[test]
    fn capability_parameter_schemas_enforce_required_members_and_types() {
        let schema = json!({
            "type":"object","required":["domain"],
            "properties":{"domain":{"type":"string"}},"additionalProperties":true
        });
        assert!(validate_instance(&json!({"domain":"surface"}), &schema).is_ok());
        assert!(validate_instance(&json!({}), &schema).is_err());
        assert!(validate_instance(&json!({"domain":7}), &schema).is_err());
    }

    #[test]
    fn embedded_definitions_keep_allocations_and_field_templates_together() {
        let definitions = definitions().unwrap();
        assert_eq!(definitions.numeric_ids["veyra.cap.conformance_probe/1"], 0x7ffe);
        for (id, contract) in &definitions.contracts {
            assert!(definitions.numeric_ids.contains_key(id), "missing allocation for {id}");
            let template_ids = contract["field_template_ids"].as_array().unwrap();
            let numeric_ids: Vec<_> =
                template_ids.iter().map(|value| value.as_u64().unwrap()).collect();
            let mut unique_ids = numeric_ids.clone();
            unique_ids.sort_unstable();
            unique_ids.dedup();
            assert_eq!(numeric_ids.len(), unique_ids.len(), "duplicate local ID in {id}");
        }
        assert_eq!(
            definitions.contracts["veyra.cap.solid_surface/1"]["field_template_ids"],
            json!([1])
        );
    }
}
