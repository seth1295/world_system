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
    /// Canonical field identities required by a capability-derived view.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required_fields: Vec<FieldId>,
    /// Declaring capability for a capability-derived or field-backed view.
    pub capability: Option<String>,
    /// Core operator for a derived view.
    pub operator: Option<String>,
    /// Capability or topology group.
    pub group: String,
    /// Declared capability display order, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_order: Option<u32>,
    /// User-facing label from the field or capability declaration.
    pub label: String,
    /// Preserved display declaration for consumers.
    pub display: Option<Value>,
}

impl ViewDescriptor {
    pub(crate) fn accepts_field(&self, field: FieldId) -> bool {
        match (self.capability.is_some(), self.operator.is_some()) {
            (true, true) => self.required_fields.contains(&field),
            _ => true,
        }
    }
}

impl Body {
    /// Returns stored, topology-derived, and declared capability-derived views.
    pub fn views(&self) -> Vec<ViewDescriptor> {
        let mut views = Vec::new();
        let definitions = crate::capability::definitions().ok();
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
                            required_fields: Vec::new(),
                            capability: None,
                            operator: Some(id.to_owned()),
                            group: "Spatial".to_owned(),
                            display_order: None,
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
                    required_fields: Vec::new(),
                    capability: None,
                    operator: Some("topology.radial_profile".to_owned()),
                    group: "Spatial".to_owned(),
                    display_order: None,
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
            let display_order = definitions
                .as_ref()
                .and_then(|definitions| definitions.contracts.get(&field.capability))
                .and_then(capability_display_order);
            views.push(ViewDescriptor {
                id: format!("field:{}", field.id),
                domain: field.domain.clone(),
                field: Some(field.id),
                field_name: Some(field.name.clone()),
                required_fields: Vec::new(),
                capability: Some(field.capability.clone()),
                operator: None,
                group: display.get("group").and_then(Value::as_str).unwrap_or("Fields").to_owned(),
                display_order,
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
        if let Some(definitions) = definitions {
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
                let display_order = capability_display_order(contract);
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
                    let Some((domain, required_fields)) =
                        resolve_required_view_fields(needs, self.fields(), capability_domain)
                    else {
                        continue;
                    };
                    views.push(ViewDescriptor {
                        id: id.to_owned(),
                        domain,
                        field: None,
                        field_name: None,
                        required_fields,
                        capability: Some((*capability_id).to_owned()),
                        operator: Some(operator.to_owned()),
                        group: group.clone(),
                        display_order,
                        label: label.clone(),
                        display: Some(declaration.clone()),
                    });
                }
            }
        }
        sort_views(&mut views);
        views
    }
}

fn capability_display_order(contract: &Value) -> Option<u32> {
    contract
        .get("display")
        .and_then(|display| display.get("order"))
        .and_then(Value::as_u64)
        .and_then(|order| u32::try_from(order).ok())
}

fn sort_views(views: &mut [ViewDescriptor]) {
    views.sort_by(|left, right| {
        (left.display_order.is_none(), left.display_order.unwrap_or_default())
            .cmp(&(right.display_order.is_none(), right.display_order.unwrap_or_default()))
            .then_with(|| left.group.cmp(&right.group))
            .then_with(|| left.domain.cmp(&right.domain))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn dependencies_available(contract: &Value, declared: &std::collections::BTreeSet<&str>) -> bool {
    contract.get("requires").and_then(Value::as_array).is_some_and(|dependencies| {
        dependencies.iter().all(|dependency| {
            dependency.as_str().is_some_and(|dependency| declared.contains(dependency))
        })
    })
}

fn resolve_required_view_fields(
    needs: &[Value],
    fields: &[crate::body::FieldDescriptor],
    capability_domain: Option<&str>,
) -> Option<(String, Vec<FieldId>)> {
    let names: Vec<&str> = needs.iter().filter_map(Value::as_str).collect();
    if names.len() != needs.len() || names.is_empty() {
        return None;
    }
    let mut domain: Option<&str> = capability_domain;
    let mut field_ids = Vec::with_capacity(names.len());
    for name in names {
        let mut matches = fields.iter().filter(|field| {
            field.name == name && domain.is_none_or(|required| field.domain == required)
        });
        let field = matches.next()?;
        if matches.next().is_some() || domain.is_some_and(|required| required != field.domain) {
            return None;
        }
        domain = Some(field.domain.as_str());
        field_ids.push(field.id);
    }
    Some((domain?.to_owned(), field_ids))
}

#[cfg(test)]
mod tests {
    use super::{
        ViewDescriptor, capability_display_order, dependencies_available,
        resolve_required_view_fields, sort_views,
    };
    use crate::body::FieldId;
    use serde_json::json;
    use std::collections::BTreeSet;

    fn descriptor(
        id: &str,
        capability: Option<&str>,
        group: &str,
        order: Option<u32>,
    ) -> ViewDescriptor {
        ViewDescriptor {
            id: id.to_owned(),
            domain: "surface".to_owned(),
            field: None,
            field_name: None,
            required_fields: Vec::new(),
            capability: capability.map(|value| value.to_owned()),
            operator: Some(id.to_owned()),
            group: group.to_owned(),
            display_order: order,
            label: id.to_owned(),
            display: None,
        }
    }

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
    fn derived_views_resolve_required_fields_unambiguously() {
        let fields: Vec<crate::body::FieldDescriptor> = serde_json::from_value(json!([
            {"id":"0x01010001","name":"field.one","capability":"cap.sample/1","domain":"domain-a","semantic":"scalar.value","persistence":"invariant","storage":{"dtype":"u16","scale":"1","offset":"0"},"native_level":0,"temporal":{"kind":"static"},"sampling":{"interp":"nearest"},"downsample":"mean","compat":"ancillary"},
            {"id":"0x01010002","name":"field.two","capability":"cap.sample/1","domain":"domain-a","semantic":"scalar.value","persistence":"invariant","storage":{"dtype":"u16","scale":"1","offset":"0"},"native_level":0,"temporal":{"kind":"static"},"sampling":{"interp":"nearest"},"downsample":"mean","compat":"ancillary"}
        ]))
        .unwrap();
        let valid_needs = json!(["field.one", "field.two"]);
        let missing_needs = json!(["field.one", "missing"]);
        let malformed_needs = json!(["field.one", 7]);
        let one_need = json!(["field.one"]);
        let valid = resolve_required_view_fields(
            valid_needs.as_array().unwrap(),
            &fields,
            Some("domain-a"),
        )
        .unwrap();
        assert_eq!(
            valid,
            ("domain-a".to_owned(), vec![FieldId::new(0x0101, 1), FieldId::new(0x0101, 2)])
        );
        let bound_view = ViewDescriptor {
            id: "derived.one".to_owned(),
            domain: "domain-a".to_owned(),
            field: None,
            field_name: None,
            required_fields: valid.1.clone(),
            capability: Some("cap.sample/1".to_owned()),
            operator: Some("core.test/1".to_owned()),
            group: "Fields".to_owned(),
            display_order: None,
            label: "Derived one".to_owned(),
            display: None,
        };
        assert!(bound_view.accepts_field(FieldId::new(0x0101, 1)));
        assert!(!bound_view.accepts_field(FieldId::new(0x0101, 3)));
        assert_eq!(
            resolve_required_view_fields(missing_needs.as_array().unwrap(), &fields, None),
            None
        );
        assert_eq!(
            resolve_required_view_fields(malformed_needs.as_array().unwrap(), &fields, None),
            None
        );
        assert_eq!(
            resolve_required_view_fields(one_need.as_array().unwrap(), &fields, Some("domain-b")),
            None
        );

        let same_domain_duplicates: Vec<crate::body::FieldDescriptor> =
            serde_json::from_value(json!([
                {"id":"0x01010001","name":"field.duplicate","capability":"cap.sample/1","domain":"domain-a","semantic":"scalar.value","persistence":"invariant","storage":{"dtype":"u16","scale":"1","offset":"0"},"native_level":0,"temporal":{"kind":"static"},"sampling":{"interp":"nearest"},"downsample":"mean","compat":"ancillary"},
                {"id":"0x01010002","name":"field.duplicate","capability":"cap.sample/1","domain":"domain-a","semantic":"scalar.value","persistence":"invariant","storage":{"dtype":"u16","scale":"1","offset":"0"},"native_level":0,"temporal":{"kind":"static"},"sampling":{"interp":"nearest"},"downsample":"mean","compat":"ancillary"}
            ]))
            .unwrap();
        let duplicate_need = json!(["field.duplicate"]);
        assert_eq!(
            resolve_required_view_fields(
                duplicate_need.as_array().unwrap(),
                &same_domain_duplicates,
                Some("domain-a"),
            ),
            None
        );

        let cross_domain_duplicates: Vec<crate::body::FieldDescriptor> =
            serde_json::from_value(json!([
                {"id":"0x01010001","name":"field.duplicate","capability":"cap.sample/1","domain":"domain-a","semantic":"scalar.value","persistence":"invariant","storage":{"dtype":"u16","scale":"1","offset":"0"},"native_level":0,"temporal":{"kind":"static"},"sampling":{"interp":"nearest"},"downsample":"mean","compat":"ancillary"},
                {"id":"0x01010002","name":"field.duplicate","capability":"cap.sample/1","domain":"domain-b","semantic":"scalar.value","persistence":"invariant","storage":{"dtype":"u16","scale":"1","offset":"0"},"native_level":0,"temporal":{"kind":"static"},"sampling":{"interp":"nearest"},"downsample":"mean","compat":"ancillary"}
            ]))
            .unwrap();
        assert_eq!(
            resolve_required_view_fields(
                duplicate_need.as_array().unwrap(),
                &cross_domain_duplicates,
                None,
            ),
            None
        );
        let domain_resolved = resolve_required_view_fields(
            duplicate_need.as_array().unwrap(),
            &cross_domain_duplicates,
            Some("domain-b"),
        )
        .unwrap();
        assert_eq!(domain_resolved, ("domain-b".to_owned(), vec![FieldId::new(0x0101, 2)]));
        let cross_domain_view = ViewDescriptor {
            id: "derived.two".to_owned(),
            domain: domain_resolved.0,
            field: None,
            field_name: None,
            required_fields: domain_resolved.1,
            capability: Some("cap.sample/1".to_owned()),
            operator: Some("core.test/1".to_owned()),
            group: "Fields".to_owned(),
            display_order: None,
            label: "Derived two".to_owned(),
            display: None,
        };
        assert!(!cross_domain_view.accepts_field(FieldId::new(0x0101, 1)));
        assert!(cross_domain_view.accepts_field(FieldId::new(0x0101, 2)));
    }

    #[test]
    fn capability_display_order_is_preserved_and_drives_catalog_order() {
        let later_contract = json!({"display":{"order":20}});
        let earlier_contract = json!({"display":{"order":10}});
        let later_order = capability_display_order(&later_contract).unwrap();
        let earlier_order = capability_display_order(&earlier_contract).unwrap();

        let mut views = vec![
            descriptor("field.later", Some("cap.later/1"), "A group", Some(later_order)),
            descriptor("derived.earlier", Some("cap.earlier/1"), "Z group", Some(earlier_order)),
        ];
        sort_views(&mut views);
        assert_eq!(views[0].id, "derived.earlier");
        assert_eq!(views[1].id, "field.later");
        assert_eq!(serde_json::to_value(&views[0]).unwrap()["display_order"], 10);
    }

    #[test]
    fn missing_and_equal_display_orders_have_deterministic_ties() {
        let mut views = vec![
            descriptor("z-view", None, "Fields", Some(5)),
            descriptor("b-view", None, "Fields", None),
            descriptor("a-view", None, "Fields", Some(5)),
        ];
        sort_views(&mut views);
        assert_eq!(
            views.iter().map(|view| view.id.as_str()).collect::<Vec<_>>(),
            vec!["a-view", "z-view", "b-view"]
        );
    }
}
