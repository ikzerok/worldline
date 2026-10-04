//! serde内部tag的unit变体会忽略额外键；空struct wire变体才能执行严格字段校验。
use super::{CatalogImportField, CatalogImportType};
use serde::{Deserialize, Deserializer};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum FieldWire {
    Kind {},
    Id {},
    Display {},
    EntityType {},
    Description {},
    Property {
        key: String,
        value_type: CatalogImportType,
    },
    Ignore {},
}
impl<'de> Deserialize<'de> for CatalogImportField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match FieldWire::deserialize(deserializer)? {
            FieldWire::Kind {} => Self::Kind,
            FieldWire::Id {} => Self::Id,
            FieldWire::Display {} => Self::Display,
            FieldWire::EntityType {} => Self::EntityType,
            FieldWire::Description {} => Self::Description,
            FieldWire::Property { key, value_type } => Self::Property { key, value_type },
            FieldWire::Ignore {} => Self::Ignore,
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TypeWire {
    Text {},
    Number {},
    Bool {},
    Ref { target_kind: String },
}
impl<'de> Deserialize<'de> for CatalogImportType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match TypeWire::deserialize(deserializer)? {
            TypeWire::Text {} => Self::Text,
            TypeWire::Number {} => Self::Number,
            TypeWire::Bool {} => Self::Bool,
            TypeWire::Ref { target_kind } => Self::Ref { target_kind },
        })
    }
}
