use std::collections::HashMap;
use std::sync::Arc;

use solana_pubkey::Pubkey;

use crate::models::{Idl, IdlTypeDefinition};

/// Registry for loading and managing IDL files.
#[derive(Debug, Clone)]
pub struct IdlRegistry {
    inner: Arc<IdlRegistryInner>,
}

#[derive(Debug, Clone)]
struct IdlRegistryInner {
    idls: HashMap<Pubkey, Idl>,
    types_by_program_and_name: HashMap<(Pubkey, String), IdlTypeDefinition>,
}

impl IdlRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(IdlRegistryInner {
                idls: HashMap::new(),
                types_by_program_and_name: HashMap::new(),
            }),
        }
    }

    /// Create a registry pre-populated with a single IDL and its type definitions.
    pub fn with_idl(program_id: Pubkey, idl: &Idl) -> Self {
        let mut idls = HashMap::new();
        idls.insert(program_id, idl.clone());

        let mut types_by_program_and_name = HashMap::new();
        if let Some(types) = &idl.types {
            for type_def in types {
                types_by_program_and_name
                    .insert((program_id, type_def.name.clone()), type_def.clone());
            }
        }

        Self { inner: Arc::new(IdlRegistryInner { idls, types_by_program_and_name }) }
    }

    /// Get an IDL by program ID.
    pub fn get(&self, program_id: &Pubkey) -> Option<&Idl> {
        self.inner.idls.get(program_id)
    }

    /// Get a type definition by program ID and name.
    pub fn get_type_by_program(
        &self,
        program_id: &Pubkey,
        name: &str,
    ) -> Option<&IdlTypeDefinition> {
        self.inner.types_by_program_and_name.get(&(*program_id, name.to_string()))
    }
}

impl Default for IdlRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn sample_idl() -> Idl {
        serde_json::from_str(
            r#"{
                "address": "11111111111111111111111111111111",
                "metadata": { "name": "test", "version": "0.1.0", "spec": "0.1.0" },
                "instructions": [],
                "types": [
                    {
                        "name": "Foo",
                        "type": { "kind": "struct", "fields": [{ "name": "x", "type": "u8" }] }
                    }
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn empty_registry_returns_none() {
        let reg = IdlRegistry::new();
        let key = Pubkey::new_unique();
        assert!(reg.get(&key).is_none());
        assert!(reg.get_type_by_program(&key, "Foo").is_none());
    }

    #[test]
    fn with_idl_stores_idl_and_types() {
        let idl = sample_idl();
        let pid = Pubkey::new_unique();
        let reg = IdlRegistry::with_idl(pid, &idl);

        assert!(reg.get(&pid).is_some());
        assert_eq!(reg.get(&pid).unwrap().metadata.name, "test");

        let type_def = reg.get_type_by_program(&pid, "Foo");
        assert!(type_def.is_some());
        assert_eq!(type_def.unwrap().name, "Foo");
    }

    #[test]
    fn get_type_wrong_program_returns_none() {
        let idl = sample_idl();
        let pid = Pubkey::new_unique();
        let other = Pubkey::new_unique();
        let reg = IdlRegistry::with_idl(pid, &idl);

        assert!(reg.get_type_by_program(&other, "Foo").is_none());
    }

    #[test]
    fn get_type_wrong_name_returns_none() {
        let idl = sample_idl();
        let pid = Pubkey::new_unique();
        let reg = IdlRegistry::with_idl(pid, &idl);

        assert!(reg.get_type_by_program(&pid, "Bar").is_none());
    }
}
