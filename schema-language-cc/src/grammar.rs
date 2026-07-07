//! `ReferenceGrammar` — the parenthesis-reference dispatch precedence as data.
//!
//! The whole grammar is one NOTA value the `nota` seed decodes directly:
//!
//! ```text
//! (ReferenceGrammar (Builtin Vector 1) (Builtin Optional 1) (Builtin ScopeOf 1)
//!                    (Builtin Map 2) (Builtin Bytes Atom) DeclaredMacro Application)
//! ```
//!
//! It is an ordered list of [`ReferenceForm`]s. The order *is* the dispatch
//! precedence: a generated resolver tries each form top to bottom. Nothing here
//! interprets the grammar at runtime — [`crate::dispatch`] reads a validated
//! grammar and emits the resolver as Rust.

use nota::{
    Block, BlockShape, CaptureName, MacroCandidate, PositionPredicate, StructuralMacroError,
    StructuralMacroNode, StructuralMacroNodeError, StructuralVariant,
};

/// The dispatch precedence, top to bottom. The `body` shape reads the headed
/// tail as an ordered stream of forms, so `(ReferenceGrammar <form>…)` decodes
/// straight into this list.
#[derive(Clone, Debug, Eq, PartialEq, StructuralMacroNode)]
pub enum ReferenceGrammar {
    #[shape(head = "ReferenceGrammar", body)]
    Forms(Vec<ReferenceForm>),
}

impl ReferenceGrammar {
    /// The forms in declared precedence order.
    pub fn forms(&self) -> &[ReferenceForm] {
        let Self::Forms(forms) = self;
        forms
    }
}

/// One rung of the precedence ladder.
///
/// - `Builtin` — a reserved head with a fixed arity, matched first and by name.
/// - `DeclaredMacro` — the marker meaning "consult the macro registry here".
/// - `Application` — the generic `(Foo A B…)` catch-all; must be unique and last.
#[derive(Clone, Debug, Eq, PartialEq, StructuralMacroNode)]
pub enum ReferenceForm {
    #[shape(head = "Builtin", arity = 3)]
    Builtin(BuiltinHead, BuiltinArity),
    #[shape(keyword = "DeclaredMacro")]
    DeclaredMacro,
    #[shape(keyword = "Application")]
    Application,
}

impl ReferenceForm {
    /// The built-in head this form claims, if it is a `Builtin` form.
    pub fn builtin_head(&self) -> Option<&BuiltinHead> {
        match self {
            Self::Builtin(head, _) => Some(head),
            Self::DeclaredMacro | Self::Application => None,
        }
    }
}

/// A reserved built-in head, e.g. `Vector`, `Map`, `Bytes`. Always PascalCase.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct BuiltinHead(String);

impl BuiltinHead {
    /// The head spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The head's PascalCase spelling rendered in snake_case, e.g.
    /// `ScopeOf` -> `scope_of`. Used to name the per-built-in construction
    /// method (`resolve_<snake>`) the generated dispatch calls.
    pub fn to_snake_case(&self) -> String {
        let mut snake = String::with_capacity(self.0.len() + 4);
        for (index, character) in self.0.chars().enumerate() {
            if character.is_ascii_uppercase() {
                if index != 0 {
                    snake.push('_');
                }
                snake.push(character.to_ascii_lowercase());
            } else {
                snake.push(character);
            }
        }
        snake
    }
}

impl TryFrom<&str> for BuiltinHead {
    type Error = crate::error::Error;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        Self::from_structural_nota(text)
            .map_err(|error| crate::error::Error::Decode(format!("built-in head {text}: {error}")))
    }
}

impl std::fmt::Display for BuiltinHead {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A built-in head is a single PascalCase atom, decoded as its own structural
/// node so it can sit as a typed sub-field of `(Builtin <head> <arity>)`.
impl StructuralMacroNode for BuiltinHead {
    type Error = StructuralMacroNodeError;

    fn structural_position() -> PositionPredicate {
        PositionPredicate::named("BuiltinHead")
    }

    fn structural_variants() -> Vec<StructuralVariant> {
        vec![
            BlockShape::pascal_atom(Some(CaptureName::new("field_0")))
                .into_structural_variant("BuiltinHead", "PascalCase built-in head"),
        ]
    }

    fn from_structural_block(block: &Block) -> Result<Self, StructuralMacroError<Self::Error>> {
        let text = block
            .demote_to_string()
            .filter(|_| block.qualifies_as_pascal_case_symbol());
        match text {
            Some(text) => Ok(Self(text.to_owned())),
            None => Err(StructuralMacroError::MatchedNode(
                StructuralMacroNodeError::MissingCapture {
                    node: "BuiltinHead",
                    variant: "BuiltinHead",
                    capture: "field_0",
                },
            )),
        }
    }

    fn from_structural_candidate(
        candidate: MacroCandidate<'_>,
    ) -> Result<Self, StructuralMacroError<Self::Error>> {
        match candidate.blocks() {
            [block] => Self::from_structural_block(block),
            blocks => Err(StructuralMacroError::MatchedNode(
                StructuralMacroNodeError::MissingSlot {
                    node: "BuiltinHead",
                    variant: "BuiltinHead",
                    capture: "field_0",
                    slot: blocks.len(),
                },
            )),
        }
    }

    fn to_structural_nota(&self) -> String {
        self.0.clone()
    }
}

/// What a built-in head takes after it: a fixed argument count, or the literal
/// `Atom` marker for heads (like `Bytes`) that take a leaf atom, not a type
/// argument. Modeled as a typed sum rather than a bare number so the `Atom`
/// case is a first-class arm the generator can branch on, never a sentinel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinArity {
    /// `Atom` — the head consumes one bare atom leaf, not a typed argument.
    Atom,
    /// A fixed count of type arguments (the `1` in `(Builtin Vector 1)`).
    Count(ArgumentCount),
}

impl BuiltinArity {
    /// A fixed count of type arguments.
    pub fn count(arguments: usize) -> Self {
        Self::Count(ArgumentCount(arguments))
    }

    /// The total root-object count the matched parenthesis block holds: the
    /// head plus its arguments. `Atom` and a single type argument both occupy
    /// one trailing slot, so both are arity 2.
    pub fn block_object_count(&self) -> usize {
        match self {
            Self::Atom => 2,
            Self::Count(count) => count.as_block_object_count(),
        }
    }
}

/// A fixed count of type arguments a built-in head takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArgumentCount(usize);

impl ArgumentCount {
    /// The argument count value.
    pub fn value(&self) -> usize {
        self.0
    }

    /// The total objects inside the matched parenthesis: head plus arguments.
    pub fn as_block_object_count(&self) -> usize {
        self.0 + 1
    }
}

/// The arity slot is either the literal `Atom` keyword or a bare integer atom.
/// Neither shape fits the derive vocabulary (a bare number is not PascalCase
/// and has no head), so it reads its single atom leaf directly.
impl StructuralMacroNode for BuiltinArity {
    type Error = StructuralMacroNodeError;

    fn structural_position() -> PositionPredicate {
        PositionPredicate::named("BuiltinArity")
    }

    fn structural_variants() -> Vec<StructuralVariant> {
        vec![
            BlockShape::literal("Atom").into_structural_variant("Atom", "the Atom arity marker"),
            BlockShape::pascal_atom(Some(CaptureName::new("field_0")))
                .into_structural_variant("Count", "a bare integer argument count"),
        ]
    }

    fn from_structural_block(block: &Block) -> Result<Self, StructuralMacroError<Self::Error>> {
        let text = block
            .demote_to_string()
            .ok_or(StructuralMacroError::MatchedNode(
                StructuralMacroNodeError::MissingCapture {
                    node: "BuiltinArity",
                    variant: "BuiltinArity",
                    capture: "field_0",
                },
            ))?;
        if text == "Atom" {
            return Ok(Self::Atom);
        }
        let count = text.parse::<usize>().map_err(|error| {
            StructuralMacroError::MatchedNode(StructuralMacroNodeError::Field {
                node: "BuiltinArity",
                variant: "Count",
                field: 0,
                error: error.to_string(),
            })
        })?;
        Ok(Self::Count(ArgumentCount(count)))
    }

    fn from_structural_candidate(
        candidate: MacroCandidate<'_>,
    ) -> Result<Self, StructuralMacroError<Self::Error>> {
        match candidate.blocks() {
            [block] => Self::from_structural_block(block),
            blocks => Err(StructuralMacroError::MatchedNode(
                StructuralMacroNodeError::MissingSlot {
                    node: "BuiltinArity",
                    variant: "BuiltinArity",
                    capture: "field_0",
                    slot: blocks.len(),
                },
            )),
        }
    }

    fn to_structural_nota(&self) -> String {
        match self {
            Self::Atom => "Atom".to_owned(),
            Self::Count(count) => count.value().to_string(),
        }
    }
}
