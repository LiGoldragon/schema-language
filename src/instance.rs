//! Rendering the per-instance schema trace through the schema encoder.
//!
//! nota's decoder captures an [`InstanceSchema`](nota::InstanceSchema)
//! while it validates a value: at every position it records the
//! [`TypeReference`](nota::TypeReference) it expected. This module projects
//! that trace into schema text. Every *reference* token is produced by
//! [`SourceReference::rendered_schema_text`] — the one schema encoder — and the
//! structural delimiters come from nota's [`Delimiter`]. There is no
//! hand-written schema printer here: the renderer only chooses delimiters and
//! delegates token rendering to the encoder.
//!
//! Two projections fall out of the same trace:
//!
//! - [`InstanceSchemaText::aligned`] — one reference token per value position
//!   (a struct shows its field *type names*, an enum shows its enum name with
//!   the realized payload's reference one level in). `Entry` renders as
//!   `{ Domains Kind Description Certainty Importance Privacy Referents }`;
//!   `DomainMatch::Partial(…)` renders as `DomainMatch.DomainScopes`.
//! - [`InstanceSchemaText::expanded`] — recurse all the way down the realized
//!   value: the root `Input::Record` renders as
//!   `Input.({ … } { Testimony Reasoning })`, with the transparent `Record` /
//!   `RecordRequest` wrappers collapsed to provenance.
//!
//! At an enum-payload position both projections collapse the variant's
//! transparent payload wrapper (the auto-generated newtype that merely names
//! the variant — `Partial`, `Record`) to provenance: the rendered token is the
//! wrapped reference, never the wrapper.

use nota::{Delimiter, InstanceSchema, InstanceSchemaBody};

use crate::SourceReference;

/// A renderer over one [`InstanceSchema`] node. Carries the node so the two
/// projection methods are verbs on the data they render.
pub struct InstanceSchemaText<'schema> {
    schema: &'schema InstanceSchema,
}

impl<'schema> InstanceSchemaText<'schema> {
    pub fn new(schema: &'schema InstanceSchema) -> Self {
        Self { schema }
    }

    /// The reference the decoder expected at this position, rendered through the
    /// schema encoder.
    pub fn reference_text(&self) -> String {
        SourceReference::from_instance_reference(self.schema.expected()).rendered_schema_text()
    }

    /// The aligned projection: one reference token per position, structs as
    /// `{ field-references }`, enums as their name with the realized payload's
    /// reference one level in.
    pub fn aligned(&self) -> String {
        match self.schema.body() {
            InstanceSchemaBody::Struct(fields) => {
                let field_texts = fields
                    .iter()
                    .map(|field| Self::new(field).reference_text())
                    .collect::<Vec<_>>();
                self.brace(field_texts)
            }
            InstanceSchemaBody::EnumPayload(Some(payload)) => {
                let payload_text = Self::new(payload).collapsed_reference();
                self.application_text(self.reference_text(), [payload_text])
            }
            // Scalar, unit enum, newtype, optional, vector, map: one token.
            _ => self.reference_text(),
        }
    }

    /// The expanded projection: recurse the realized value all the way down,
    /// collapsing the variant payload wrappers to provenance.
    pub fn expanded(&self) -> String {
        match self.schema.body() {
            InstanceSchemaBody::Scalar => self.reference_text(),
            InstanceSchemaBody::EnumPayload(None) => self.reference_text(),
            InstanceSchemaBody::EnumPayload(Some(payload)) => {
                let payload_text = Self::new(payload).collapsed_expansion();
                self.application_text(self.reference_text(), [payload_text])
            }
            InstanceSchemaBody::Newtype(inner) => {
                // A newtype keeps both facts: the wrapper name and the wrapped
                // schema one level in, as `Wrapper.inner`.
                let inner_text = Self::new(inner).expanded();
                self.application_text(self.reference_text(), [inner_text])
            }
            InstanceSchemaBody::Optional(None) => self.reference_text(),
            InstanceSchemaBody::Optional(Some(inner)) => {
                let inner_text = Self::new(inner).expanded();
                self.application_text(self.reference_text(), [inner_text])
            }
            InstanceSchemaBody::Vector(elements) => {
                if elements.is_empty() {
                    return self.reference_text();
                }
                let element_texts = elements
                    .iter()
                    .map(|element| Self::new(element).expanded())
                    .collect::<Vec<_>>();
                Delimiter::SquareBracket.wrap(element_texts)
            }
            InstanceSchemaBody::Map(pairs) => {
                if pairs.is_empty() {
                    return self.reference_text();
                }
                let mut entries = Vec::with_capacity(pairs.len() * 2);
                for (key, value) in pairs {
                    entries.push(Self::new(key).expanded());
                    entries.push(Self::new(value).expanded());
                }
                self.brace(entries)
            }
            InstanceSchemaBody::Struct(fields) => self.struct_text(fields),
        }
    }

    /// Render a variant payload at aligned depth, collapsing the transparent
    /// payload wrapper to provenance. `Partial(DomainScopes)` collapses one
    /// newtype level to the `DomainScopes` reference. A wrapper over a struct
    /// (`Record(RecordRequest { entry justification })`) collapses both the
    /// wrapper and the struct name: the payload renders as the struct's fields
    /// in a paren group at the value's own delimiter position, each field at
    /// aligned depth.
    fn collapsed_reference(&self) -> String {
        match self.schema.body() {
            // Peel exactly the variant's transparent wrapper. If it reveals a
            // struct, the struct name is provenance too and the payload renders
            // as the field group; otherwise the revealed type is the declared
            // payload reference and renders by name.
            InstanceSchemaBody::Newtype(inner) => match inner.body() {
                InstanceSchemaBody::Struct(fields) => Self::new(inner).aligned_field_group(fields),
                _ => Self::new(inner).reference_text(),
            },
            InstanceSchemaBody::Struct(fields) => self.aligned_field_group(fields),
            _ => self.reference_text(),
        }
    }

    fn aligned_field_group(&self, fields: &'schema [InstanceSchema]) -> String {
        let field_texts = fields
            .iter()
            .map(|field| Self::new(field).aligned())
            .collect::<Vec<_>>();
        self.parenthesis(field_texts)
    }

    /// Render a variant payload expanded, collapsing the transparent payload
    /// wrapper. A wrapper over a struct (`Record(RecordRequest { … })`)
    /// contributes no token: the payload renders as the struct's field group.
    fn collapsed_expansion(&self) -> String {
        match self.schema.body() {
            InstanceSchemaBody::Newtype(inner) => Self::new(inner).collapsed_expansion(),
            InstanceSchemaBody::Struct(fields) => self.struct_text(fields),
            _ => self.expanded(),
        }
    }

    fn struct_text(&self, fields: &'schema [InstanceSchema]) -> String {
        let field_texts = fields
            .iter()
            .map(|field| Self::new(field).expanded())
            .collect::<Vec<_>>();
        self.brace(field_texts)
    }

    fn application_text<Items>(&self, head: String, items: Items) -> String
    where
        Items: IntoIterator<Item = String>,
    {
        let arguments = items.into_iter().collect::<Vec<_>>();
        match arguments.as_slice() {
            [] => head,
            [argument] => format!("{head}.{argument}"),
            _ => format!("{head}.{}", Delimiter::Parenthesis.wrap(arguments)),
        }
    }

    fn parenthesis<Items>(&self, items: Items) -> String
    where
        Items: IntoIterator<Item = String>,
    {
        Delimiter::Parenthesis.wrap(items)
    }

    /// Braces are padded (`{ a b }`) to match the schema encoder's struct
    /// spacing; parentheses and brackets are not.
    fn brace(&self, children: Vec<String>) -> String {
        if children.is_empty() {
            return format!(
                "{}{}",
                Delimiter::Brace.opening_text(),
                Delimiter::Brace.closing_text()
            );
        }
        format!(
            "{} {} {}",
            Delimiter::Brace.opening_text(),
            children.join(" "),
            Delimiter::Brace.closing_text()
        )
    }
}
