//! Capability- and topology-driven field view catalog.

use serde::Serialize;
use serde_json::Value;

use crate::body::FieldId;
use crate::io::Body;

/// A view exposed by the body core without consumer-specific ontology.
#[derive(Clone, Debug, Serialize)]
pub struct ViewDescriptor {
    /// Stable view identifier.
    pub id: String,
    /// Spatial domain used by the view.
    pub domain: String,
    /// Stored field used by a field-backed view.
    pub field: Option<FieldId>,
    /// Declared field name when field-backed.
    pub field_name: Option<String>,
    /// Declaring capability for a capability-derived or field-backed view.
    pub capability: Option<String>,
    /// Core operator for a derived view.
    pub operator: Option<String>,
    /// Capability or topology group.
    pub group: String,
    /// User-facing label from the field or capability declaration.
    pub label: String,
    /// Preserved display declaration for consumers.
    pub display: Option<Value>,
}

impl Body {
    /// Returns stored, topology-derived, and declared capability-derived views.
    pub fn views(&self) -> Vec<ViewDescriptor> {
        let mut views = Vec::new();
        for domain in self.domains() {
            match domain.topology.as_str() {
                "veyra.topo.dir_cube/1" => {
                    for (id, label) in [
                        ("topology.cube_face", "Cube face"),
                        ("topology.tile_level", "Tile level"),
                        ("topology.axial_latitude", "Axial latitude"),
                    ] {
                        views.push(ViewDescriptor {
                            id: id.to_owned(),
                            domain: domain.id.clone(),
                            field: None,
                            field_name: None,
                            capability: None,
                            operator: Some(id.to_owned()),
                            group: "Spatial".to_owned(),
                            label: label.to_owned(),
                            display: None,
                        });
                    }
                }
                "veyra.topo.radial_1d/1" => views.push(ViewDescriptor {
                    id: "topology.radial_profile".to_owned(),
                    domain: domain.id.clone(),
                    field: None,
                    field_name: None,
                    capability: None,
                    operator: Some("topology.radial_profile".to_owned()),
                    group: "Spatial".to_owned(),
                    label: "Radial profile".to_owned(),
                    display: None,
                }),
                _ => {}
            }
        }

        for field in self.fields() {
            if !self.capabilities().iter().any(|capability| capability.id == field.capability) {
                continue;
            }
            let Some(display) = field.extra.get("display").filter(|value| value.is_object()) else {
                continue;
            };
            views.push(ViewDescriptor {
                id: format!("field:{}", field.id),
                domain: field.domain.clone(),
                field: Some(field.id),
                field_name: Some(field.name.clone()),
                capability: Some(field.capability.clone()),
                operator: None,
                group: display.get("group").and_then(Value::as_str).unwrap_or("Fields").to_owned(),
                label: display
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or(&field.name)
                    .to_owned(),
                display: Some(display.clone()),
            });
        }

        let declared_capabilities: std::collections::BTreeSet<&str> =
            self.capabilities().iter().map(|capability| capability.id.as_str()).collect();
        if let Ok(definitions) = crate::capability::definitions() {
            for capability_id in &declared_capabilities {
                let Some(contract) = definitions.contracts.get(*capability_id) else {
                    continue;
                };
                if !dependencies_available(contract, &declared_capabilities) {
                    continue;
                }
                let group = contract
                    .get("display")
                    .and_then(|display| display.get("group"))
                    .and_then(Value::as_str)
                    .unwrap_or("Derived")
                    .to_owned();
                let label = contract
                    .get("display")
                    .and_then(|display| display.get("label"))
                    .and_then(Value::as_str)
                    .unwrap_or(&group)
                    .to_owned();
                let Some(derived_views) = contract.get("derived_views").and_then(Value::as_array)
                else {
                    continue;
                };
                for declaration in derived_views {
                    let (Some(id), Some(operator), Some(needs)) = (
                        declaration.get("id").and_then(Value::as_str),
                        declaration.get("op").and_then(Value::as_str),
                        declaration.get("needs").and_then(Value::as_array),
                    ) else {
                        continue;
                    };
                    let capability_domain = self
                        .capabilities()
                        .iter()
                        .find(|item| item.id == *capability_id)
                        .and_then(|item| item.params.get("domain"))
                        .and_then(Value::as_str);
                    let Some(domain) =
                        required_view_domain(needs, self.fields(), capability_domain)
                    else {
                        continue;
                    };
                    views.push(ViewDescriptor {
                        id: id.to_owned(),
                        domain,
                        field: None,
                        field_name: None,
                        capability: Some((*capability_id).to_owned()),
                        operator: Some(operator.to_owned()),
                        group: group.clone(),
                        label: label.clone(),
                        display: Some(declaration.clone()),
                    });
                }
            }
        }
        views.sort_by(|left, right| {
            (&left.group, &left.domain, &left.id).cmp(&(&right.group, &right.domain, &right.id))
        });
        views
    }
}

fn dependencies_available(contract: &Value, declared: &std::collections::BTreeSet<&str>) -> bool {
    contract.get("requires").and_then(Value::as_array).is_some_and(|dependencies| {
        dependencies.iter().all(|dependency| {
            dependency.as_str().is_some_and(|dependency| declared.contains(dependency))
        })
    })
}

fn required_view_domain(
    needs: &[Value],
    fields: &[crate::body::FieldDescriptor],
    capability_domain: Option<&str>,
) -> Option<String> {
    let names: Vec<&str> = needs.iter().filter_map(Value::as_str).collect();
    if names.len() != needs.len() || names.is_empty() {
        return None;
    }
    let first = *names.first()?;
    let domain = fields.iter().find(|field| field.name == first)?.domain.as_str();
    if capability_domain.is_some_and(|required| required != domain)
        || !names.iter().all(|name| {
            fields.iter().any(|field| field.name.as_str() == *name && field.domain == domain)
        })
    {
        return None;
    }
    Some(domain.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{dependencies_available, required_view_domain};
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn derived_view_capability_dependencies_must_be_declared() {
        let contract = json!({"requires":["cap.base/1","cap.geometry/1"]});
        let complete = BTreeSet::from(["cap.base/1", "cap.geometry/1", "cap.derived/1"]);
        let incomplete = BTreeSet::from(["cap.base/1", "cap.derived/1"]);
        assert!(dependencies_available(&contract, &complete));
        assert!(!dependencies_available(&contract, &incomplete));
        assert!(!dependencies_available(&json!({}), &complete));
    }

    #[test]
    fn derived_views_require_all_declared_fields_in_one_compatible_domain() {
        let fields: Vec<crate::body::FieldDescriptor> = serde_json::from_value(json!([
            {"id":"0x01010001","name":"field.one","capability":"cap.sample/1","domain":"domain-a","semantic":"scalar.value","persistence":"invariant","storage":{"dtype":"u16","scale":"1","offset":"0"},"native_level":0,"temporal":{"kind":"static"},"sampling":{"interp":"nearest"},"downsample":"mean","compat":"ancillary"},
            {"id":"0x01010002","name":"field.two","capability":"cap.sample/1","domain":"domain-a","semantic":"scalar.value","persistence":"invariant","storage":{"dtype":"u16","scale":"1","offset":"0"},"native_level":0,"temporal":{"kind":"static"},"sampling":{"interp":"nearest"},"downsample":"mean","compat":"ancillary"}
        ]))
        .unwrap();
        let valid_needs = json!(["field.one", "field.two"]);
        let missing_needs = json!(["field.one", "missing"]);
        let malformed_needs = json!(["field.one", 7]);
        let one_need = json!(["field.one"]);
        assert_eq!(
            required_view_domain(valid_needs.as_array().unwrap(), &fields, Some("domain-a")),
            Some("domain-a".to_owned())
        );
        assert_eq!(required_view_domain(missing_needs.as_array().unwrap(), &fields, None), None);
        assert_eq!(required_view_domain(malformed_needs.as_array().unwrap(), &fields, None), None);
        assert_eq!(
            required_view_domain(one_need.as_array().unwrap(), &fields, Some("domain-b")),
            None
        );
    }
}
