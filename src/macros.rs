use nota::{Block, StructureHeader};

use crate::{Name, SchemaError};

/// The transient lowering context threaded through the public `lower_*`
/// entry points. It records the parsed document's structure header for the
/// duration of a lowering; the durable lowering output is the returned
/// [`crate::TrueSchema`] and its core projection.
#[derive(Clone, Debug, Default)]
pub struct MacroContext {
    structure_headers: Vec<StructureHeader>,
}

impl MacroContext {
    pub fn remember_structure_header(&mut self, header: StructureHeader) {
        self.structure_headers.push(header);
    }

    pub fn structure_headers(&self) -> &[StructureHeader] {
        &self.structure_headers
    }
}

pub(crate) trait BlockDebug {
    fn reemit_fallback(&self) -> String;
}

pub(crate) trait SchemaBlockExt {
    fn schema_name(&self) -> Result<Name, SchemaError>;
}

impl BlockDebug for Block {
    fn reemit_fallback(&self) -> String {
        self.demote_to_string()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{self:?}"))
    }
}

impl SchemaBlockExt for Block {
    fn schema_name(&self) -> Result<Name, SchemaError> {
        self.atom()
            .filter(|atom| atom.qualifies_as_symbol())
            .map(|atom| Name::new(atom.text()))
            .ok_or_else(|| SchemaError::ExpectedSymbol {
                found: self.reemit_fallback(),
            })
    }
}
