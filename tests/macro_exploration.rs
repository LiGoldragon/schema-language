//! Macro-system exploration — covers the match-criteria taxonomy from
//! intent record 932 (Maximum).
//!
//! Per record 932: the macro engine supports MULTIPLE MATCH CRITERIA —
//! delimiter (paren/brace/square/pipe-text), internal shape (what the
//! contents look like), number of root objects, qualification-as-symbol
//! (PascalCase candidate, kebab-case, etc), and combinations of these.
//! Multiple matches dispatch on FIRST-MATCH-WINS by registration order
//! at the moment (the dispatch on the most-specific match is a TODO —
//! see report 388, "Open questions").
//!
//! Each test defines a small custom `SchemaMacroHandler` impl that exercises
//! ONE criterion in isolation, OR uses the declarative macro form to
//! express the same idea, OR uses a combination. Fixtures are tight —
//! one concept per test.

use nota::Document;
use schema_language::{
    MacroContext, MacroLibrary, MacroLibraryArtifact, MacroLibrarySourceEntry, MacroObject,
    MacroOutput, MacroPair, MacroPosition, MacroRegistry, SchemaError, SchemaMacroHandler,
    TypeDeclaration, TypeReference,
};

// ---------------------------------------------------------------------
// Scenario 1 — Delimiter-only match
//
// The macro fires when the input is a brace (or paren, square, pipe-text),
// regardless of contents. This is the simplest criterion — only the outer
// delimiter shape matters.
// ---------------------------------------------------------------------

/// Records the macro name once it has fired, so the test can prove the
/// dispatch went through. Data-bearing (carries the recorded label) —
/// no ZST namespace holder.
#[derive(Clone, Debug)]
struct DelimiterOnlyMacro {
    label: &'static str,
    expected_delimiter: Delimiter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Delimiter {
    Brace,
    Parenthesis,
    SquareBracket,
}

impl DelimiterOnlyMacro {
    fn new(label: &'static str, expected_delimiter: Delimiter) -> Self {
        Self {
            label,
            expected_delimiter,
        }
    }

    fn delimiter_matches(&self, object: MacroObject<'_>) -> bool {
        match object.block() {
            None => false,
            Some(block) => match self.expected_delimiter {
                Delimiter::Brace => block.is_brace(),
                Delimiter::Parenthesis => block.is_parenthesis(),
                Delimiter::SquareBracket => block.is_square_bracket(),
            },
        }
    }
}

impl SchemaMacroHandler for DelimiterOnlyMacro {
    fn name(&self) -> &str {
        self.label
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        position == MacroPosition::EnumVariants && self.delimiter_matches(object)
    }

    fn lower(
        &self,
        _object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        _registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        context.remember_macro(self.label);
        context.remember_position(position);
        Ok(MacroOutput::Variants(Vec::new()))
    }
}

#[test]
fn delimiter_only_match_fires_on_outer_delimiter_regardless_of_contents() {
    let document = Document::parse("{Alpha Beta Gamma}").expect("nota parses");
    let object = document.root_object_at(0).expect("root");

    let brace_macro = DelimiterOnlyMacro::new("AcceptsBrace", Delimiter::Brace);
    assert!(brace_macro.matches(MacroObject::Block(object), MacroPosition::EnumVariants));

    let paren_macro = DelimiterOnlyMacro::new("AcceptsParen", Delimiter::Parenthesis);
    assert!(!paren_macro.matches(MacroObject::Block(object), MacroPosition::EnumVariants));

    // Empty brace also fires — contents are irrelevant.
    let empty = Document::parse("{}").expect("nota parses");
    let empty_object = empty.root_object_at(0).expect("root");
    assert!(brace_macro.matches(
        MacroObject::Block(empty_object),
        MacroPosition::EnumVariants
    ));

    // SquareBracket macro fires only on square-bracket input.
    let square = Document::parse("[Alpha Beta]").expect("nota parses");
    let square_object = square.root_object_at(0).expect("root");
    let square_macro = DelimiterOnlyMacro::new("AcceptsSquare", Delimiter::SquareBracket);
    assert!(square_macro.matches(
        MacroObject::Block(square_object),
        MacroPosition::EnumVariants
    ));
    assert!(!brace_macro.matches(
        MacroObject::Block(square_object),
        MacroPosition::EnumVariants
    ));
}

// ---------------------------------------------------------------------
// Scenario 2 — Shape-match (exactly 2 children, first PascalCase)
//
// Fire only when the contents have a specific structural shape — useful
// for "named-payload pair" detection.
// ---------------------------------------------------------------------

#[derive(Clone, Debug)]
struct NamedPayloadShapeMacro {
    label: &'static str,
}

impl NamedPayloadShapeMacro {
    fn new(label: &'static str) -> Self {
        Self { label }
    }
}

impl SchemaMacroHandler for NamedPayloadShapeMacro {
    fn name(&self) -> &str {
        self.label
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        if position != MacroPosition::NamespaceDeclaration {
            return false;
        }
        let Some(block) = object.block() else {
            return false;
        };
        block.is_parenthesis()
            && block.holds_root_objects() == 2
            && block
                .root_object_at(0)
                .is_some_and(|first| first.qualifies_as_pascal_case_symbol())
    }

    fn lower(
        &self,
        _object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        _registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        context.remember_macro(self.label);
        context.remember_position(position);
        Ok(MacroOutput::References(Vec::new()))
    }
}

#[test]
fn shape_match_requires_exact_inner_structure() {
    let yes_two_pascal = Document::parse("(Foo Bar)").expect("nota parses");
    let yes = yes_two_pascal.root_object_at(0).expect("root");

    let macro_obj = NamedPayloadShapeMacro::new("NamedPayload");
    assert!(
        macro_obj.matches(MacroObject::Block(yes), MacroPosition::NamespaceDeclaration),
        "(Foo Bar) — 2 children, first PascalCase — matches",
    );

    // 3 children — wrong count.
    let three_children = Document::parse("(Foo Bar Baz)").expect("nota parses");
    let no_count = three_children.root_object_at(0).expect("root");
    assert!(!macro_obj.matches(
        MacroObject::Block(no_count),
        MacroPosition::NamespaceDeclaration
    ));

    // 2 children but first is kebab-case — wrong shape.
    let kebab_first = Document::parse("(foo-bar Baz)").expect("nota parses");
    let no_shape = kebab_first.root_object_at(0).expect("root");
    assert!(!macro_obj.matches(
        MacroObject::Block(no_shape),
        MacroPosition::NamespaceDeclaration
    ));

    // Wrong position.
    assert!(!macro_obj.matches(MacroObject::Block(yes), MacroPosition::EnumVariants));
}

// ---------------------------------------------------------------------
// Scenario 3 — Object-count match (exactly N root objects)
//
// Fire when the input has exactly N root objects. Useful for matching
// fixed-shape records like the 5-element SchemaMacro declaration.
// ---------------------------------------------------------------------

#[derive(Clone, Debug)]
struct FiveObjectMacro {
    label: &'static str,
}

impl FiveObjectMacro {
    fn new(label: &'static str) -> Self {
        Self { label }
    }
}

impl SchemaMacroHandler for FiveObjectMacro {
    fn name(&self) -> &str {
        self.label
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        position == MacroPosition::RootInput
            && object
                .block()
                .is_some_and(|block| block.is_parenthesis() && block.holds_root_objects() == 5)
    }

    fn lower(
        &self,
        _object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        _registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        context.remember_macro(self.label);
        context.remember_position(position);
        Ok(MacroOutput::References(Vec::new()))
    }
}

#[test]
fn object_count_match_distinguishes_by_root_object_count() {
    let five = Document::parse("(SchemaMacro Foo NamespaceDeclaration ($X) (Type X))")
        .expect("nota parses");
    let five_object = five.root_object_at(0).expect("root");

    let macro_obj = FiveObjectMacro::new("FiveRoots");
    assert!(macro_obj.matches(MacroObject::Block(five_object), MacroPosition::RootInput));

    // 4 root objects — does not match.
    let four = Document::parse("(SchemaMacro Foo NamespaceDeclaration ($X))").expect("nota parses");
    let four_object = four.root_object_at(0).expect("root");
    assert!(!macro_obj.matches(MacroObject::Block(four_object), MacroPosition::RootInput));

    // 6 root objects — does not match.
    let six = Document::parse("(SchemaMacro Foo NamespaceDeclaration ($X) (Type X) Extra)")
        .expect("nota parses");
    let six_object = six.root_object_at(0).expect("root");
    assert!(!macro_obj.matches(MacroObject::Block(six_object), MacroPosition::RootInput));
}

#[test]
fn builtin_macro_library_round_trips_as_typed_data_and_still_executes() {
    let library = MacroLibrary::builtin().expect("builtin macros parse");
    assert_eq!(library.source_entries().len(), 5);
    assert!(
        library
            .source_entries()
            .iter()
            .all(|entry| entry.variant_name() == "SchemaMacro")
    );
    assert_eq!(library.definitions().len(), 5);
    assert!(
        library
            .definitions()
            .iter()
            .any(|definition| definition.name().as_str() == "SchemaStructDefinition")
    );

    let nota = library.to_nota_source();
    let from_nota = MacroLibrary::from_nota_source(&nota).expect("macro data reads as NOTA");
    assert_eq!(from_nota, library);

    let bytes = from_nota
        .to_binary_bytes()
        .expect("macro data archives to rkyv");
    let from_binary = MacroLibrary::from_binary_bytes(&bytes).expect("macro data reads from rkyv");
    assert_eq!(from_binary, library);

    let mut registry = MacroRegistry::new();
    for schema_macro in from_binary.into_macros() {
        registry.register_box(schema_macro);
    }

    let document = Document::parse("{ Entry { Topic Kind } }").expect("macro input parses");
    let namespace = document.root_object_at(0).expect("macro input root");
    let pair = MacroPair {
        name: namespace.root_object_at(0).expect("macro input key"),
        definition: namespace.root_object_at(1).expect("macro input value"),
    };
    let output = registry
        .lower(
            MacroObject::Pair(pair),
            MacroPosition::NamespaceDeclaration,
            &mut MacroContext::default(),
        )
        .expect("typed-data macro lowers");

    let MacroOutput::Type(TypeDeclaration::Struct(declaration)) = output else {
        panic!("expected struct type output");
    };
    assert_eq!(declaration.name.as_str(), "Entry");
    assert_eq!(declaration.fields.len(), 2);
    assert_eq!(declaration.fields[0].name.as_str(), "topic");
    assert_eq!(declaration.fields[1].name.as_str(), "kind");
}

#[test]
fn schema_macro_source_records_are_enum_variants_inside_the_library() {
    let library = MacroLibrary::builtin_source().expect("builtin macro source parses");
    let entries = library.source_entries();
    assert_eq!(entries.len(), 5);

    for entry in entries {
        match entry {
            MacroLibrarySourceEntry::SchemaMacro(definition) => {
                assert_eq!(entry.variant_name(), "SchemaMacro");
                assert!(
                    definition.name().as_str().starts_with("Schema"),
                    "source variant carries the parsed macro definition payload"
                );
            }
        }
    }
}

#[test]
fn schema_macro_artifact_records_preserve_the_source_entry_variant() {
    let library = MacroLibrary::builtin_source().expect("builtin macro source parses");
    let first_entry = library.source_entries().first().expect("first macro entry");

    match first_entry {
        MacroLibrarySourceEntry::SchemaMacro(definition) => {
            assert_eq!(first_entry.variant_name(), "SchemaMacro");
            assert_eq!(definition.name().as_str(), "SchemaStructDefinition");
        }
    }
}

#[test]
fn builtin_macro_library_artifact_is_checked_in_and_fresh() {
    let source_library = MacroLibrary::builtin_source().expect("builtin macro source parses");
    let checked_in = MacroLibraryArtifact::from_nota_source(include_str!(
        "../schemas/builtin-macros.macro-library"
    ))
    .expect("checked-in macro library artifact decodes");

    assert_eq!(
        checked_in.library(),
        &source_library,
        "schemas/builtin-macros.macro-library must be refreshed when builtin-macros.schema changes"
    );

    let runtime_library = MacroLibrary::builtin().expect("builtin artifact loads");
    assert_eq!(
        runtime_library, source_library,
        "the default macro library must load through the serialized data artifact"
    );
}

#[test]
fn macro_library_artifact_reads_and_writes_real_nota_and_binary_files() {
    let library = MacroLibrary::builtin().expect("builtin artifact loads");
    let artifact = MacroLibraryArtifact::new(library.clone());
    let paths = MacroLibraryArtifactTestPaths::new("builtin-macros");

    artifact
        .write_nota_file(paths.nota_path())
        .expect("write macro library nota artifact");
    artifact
        .write_binary_file(paths.binary_path())
        .expect("write macro library binary artifact");

    let from_nota = MacroLibraryArtifact::read_nota_file(paths.nota_path())
        .expect("read macro library nota artifact");
    let from_binary = MacroLibraryArtifact::read_binary_file(paths.binary_path())
        .expect("read macro library binary artifact");

    assert_eq!(from_nota.library(), &library);
    assert_eq!(from_binary.library(), &library);

    paths.remove();
}

#[test]
fn retired_duplicate_macro_datatype_names_do_not_return() {
    let sources = [
        include_str!("../src/declarative.rs"),
        include_str!("../src/lib.rs"),
    ];
    let retired_names = [
        "MacroLibrarySourceEntryData",
        "MacroDefinitionData",
        "MacroPatternData",
        "MacroTemplateData",
    ];

    for source in sources {
        for retired_name in retired_names {
            assert!(
                !source.contains(retired_name),
                "{retired_name} would reintroduce a source/artifact datatype split"
            );
        }
    }
}

struct MacroLibraryArtifactTestPaths {
    directory: std::path::PathBuf,
    nota_path: std::path::PathBuf,
    binary_path: std::path::PathBuf,
}

impl MacroLibraryArtifactTestPaths {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "schema-macro-library-artifact-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create macro library artifact test directory");
        Self {
            nota_path: directory.join("builtin-macros.macro-library"),
            binary_path: directory.join("builtin-macros.macro-library.rkyv"),
            directory,
        }
    }

    fn nota_path(&self) -> &std::path::Path {
        &self.nota_path
    }

    fn binary_path(&self) -> &std::path::Path {
        &self.binary_path
    }

    fn remove(&self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

// ---------------------------------------------------------------------
// Scenario 4 — Qualified-as-symbol match (PascalCase / kebab-case)
//
// Fire on a bare ATOM that qualifies as a particular case shape. Useful
// for atom-position pickers (a topic in record args, a type ref in a
// variant payload).
// ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SymbolCase {
    Pascal,
    Kebab,
    Camel,
}

#[derive(Clone, Debug)]
struct SymbolCaseMacro {
    label: &'static str,
    case: SymbolCase,
}

impl SymbolCaseMacro {
    fn new(label: &'static str, case: SymbolCase) -> Self {
        Self { label, case }
    }

    fn qualifies(&self, object: MacroObject<'_>) -> bool {
        match object.block() {
            None => false,
            Some(block) => match self.case {
                SymbolCase::Pascal => block.qualifies_as_pascal_case_symbol(),
                SymbolCase::Kebab => block.qualifies_as_kebab_case_symbol(),
                SymbolCase::Camel => block.qualifies_as_camel_case_symbol(),
            },
        }
    }
}

impl SchemaMacroHandler for SymbolCaseMacro {
    fn name(&self) -> &str {
        self.label
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        position == MacroPosition::EnumVariants && self.qualifies(object)
    }

    fn lower(
        &self,
        _object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        _registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        context.remember_macro(self.label);
        context.remember_position(position);
        Ok(MacroOutput::References(Vec::new()))
    }
}

#[test]
fn qualified_as_symbol_match_splits_pascal_kebab_camel() {
    let pascal = Document::parse("Decision").expect("nota parses");
    let kebab = Document::parse("schema-spirit").expect("nota parses");
    let camel = Document::parse("recordIdentifier").expect("nota parses");

    let pascal_macro = SymbolCaseMacro::new("AcceptsPascal", SymbolCase::Pascal);
    let kebab_macro = SymbolCaseMacro::new("AcceptsKebab", SymbolCase::Kebab);
    let camel_macro = SymbolCaseMacro::new("AcceptsCamel", SymbolCase::Camel);

    let pascal_block = pascal.root_object_at(0).expect("root");
    let kebab_block = kebab.root_object_at(0).expect("root");
    let camel_block = camel.root_object_at(0).expect("root");

    // PascalCase macro fires only on the PascalCase atom.
    assert!(pascal_macro.matches(
        MacroObject::Block(pascal_block),
        MacroPosition::EnumVariants
    ));
    assert!(!pascal_macro.matches(MacroObject::Block(kebab_block), MacroPosition::EnumVariants));
    assert!(!pascal_macro.matches(MacroObject::Block(camel_block), MacroPosition::EnumVariants));

    // kebab-case macro fires only on the kebab-case atom.
    assert!(kebab_macro.matches(MacroObject::Block(kebab_block), MacroPosition::EnumVariants));
    assert!(!kebab_macro.matches(
        MacroObject::Block(pascal_block),
        MacroPosition::EnumVariants
    ));

    // camelCase macro fires only on the camelCase atom.
    assert!(camel_macro.matches(MacroObject::Block(camel_block), MacroPosition::EnumVariants));
    assert!(!camel_macro.matches(
        MacroObject::Block(pascal_block),
        MacroPosition::EnumVariants
    ));
}

// ---------------------------------------------------------------------
// Scenario 5 — Combined criteria
//
// Brace delimiter AND every odd position qualifies as a PascalCase symbol
// AND count is even. This is the structural shape the brace-enum-sugar
// macro accepts (Name1 Payload1 Name2 Payload2 ...).
// ---------------------------------------------------------------------

#[derive(Clone, Debug)]
struct BraceNamedPairsMacro {
    label: &'static str,
}

impl BraceNamedPairsMacro {
    fn new(label: &'static str) -> Self {
        Self { label }
    }

    fn all_odd_positions_pascal_case(&self, block: &nota::Block) -> bool {
        let count = block.holds_root_objects();
        for index in (0..count).step_by(2) {
            let Some(child) = block.root_object_at(index) else {
                return false;
            };
            if !child.qualifies_as_pascal_case_symbol() {
                return false;
            }
        }
        true
    }
}

impl SchemaMacroHandler for BraceNamedPairsMacro {
    fn name(&self) -> &str {
        self.label
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        if position != MacroPosition::EnumVariants {
            return false;
        }
        let Some(block) = object.block() else {
            return false;
        };
        block.is_brace()
            && block.holds_root_objects() % 2 == 0
            && self.all_odd_positions_pascal_case(block)
    }

    fn lower(
        &self,
        _object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        _registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        context.remember_macro(self.label);
        context.remember_position(position);
        Ok(MacroOutput::Variants(Vec::new()))
    }
}

#[test]
fn combined_criteria_brace_and_even_count_and_pascal_keys() {
    let macro_obj = BraceNamedPairsMacro::new("BraceNamedPairs");

    // All three criteria satisfied — fires.
    let good = Document::parse("{ToInbox Address ToOutbox Address}").expect("nota parses");
    let good_block = good.root_object_at(0).expect("root");
    assert!(macro_obj.matches(MacroObject::Block(good_block), MacroPosition::EnumVariants));

    // Paren delimiter — fails the brace criterion.
    let wrong_delim = Document::parse("(ToInbox Address ToOutbox Address)").expect("nota parses");
    let wrong_delim_block = wrong_delim.root_object_at(0).expect("root");
    assert!(!macro_obj.matches(
        MacroObject::Block(wrong_delim_block),
        MacroPosition::EnumVariants
    ));

    // Odd count — fails the even-count criterion.
    let odd = Document::parse("{ToInbox Address Extra}").expect("nota parses");
    let odd_block = odd.root_object_at(0).expect("root");
    assert!(!macro_obj.matches(MacroObject::Block(odd_block), MacroPosition::EnumVariants));

    // kebab-case key — fails the PascalCase criterion.
    let bad_key = Document::parse("{to-inbox Address ToOutbox Address}").expect("nota parses");
    let bad_key_block = bad_key.root_object_at(0).expect("root");
    assert!(!macro_obj.matches(
        MacroObject::Block(bad_key_block),
        MacroPosition::EnumVariants
    ));
}

// ---------------------------------------------------------------------
// Scenario 6 — First-match-wins between competing macros
//
// Two macros both accept the same input; the one registered first wins.
// This is the current dispatch shape — the report 388 "Open questions"
// flags that record 932 names "most specific match" as the intended
// dispatch, which is a future change.
// ---------------------------------------------------------------------

#[derive(Clone, Debug)]
struct AnyBraceMacro {
    label: &'static str,
}

impl AnyBraceMacro {
    fn new(label: &'static str) -> Self {
        Self { label }
    }
}

impl SchemaMacroHandler for AnyBraceMacro {
    fn name(&self) -> &str {
        self.label
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        position == MacroPosition::EnumVariants
            && object.block().is_some_and(|block| block.is_brace())
    }

    fn lower(
        &self,
        _object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        _registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        context.remember_macro(self.label);
        context.remember_position(position);
        Ok(MacroOutput::Variants(Vec::new()))
    }
}

#[test]
fn first_match_wins_by_registration_order_on_overlapping_macros() {
    // Both AnyBraceMacro instances would match brace input at EnumVariants;
    // the first one registered wins.
    let document = Document::parse("{Foo Bar Baz Quux}").expect("nota parses");
    let object = document.root_object_at(0).expect("root");

    let mut earliest_first = MacroRegistry::new();
    earliest_first.register(AnyBraceMacro::new("First"));
    earliest_first.register(AnyBraceMacro::new("Second"));

    let mut context = MacroContext::default();
    earliest_first
        .lower(
            MacroObject::Block(object),
            MacroPosition::EnumVariants,
            &mut context,
        )
        .expect("brace macro fires");
    let applied: Vec<&str> = context
        .macros_applied()
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(applied, vec!["First"], "earliest-registered macro wins");

    // Reverse the order — now "Second" wins.
    let mut latest_first = MacroRegistry::new();
    latest_first.register(AnyBraceMacro::new("Second"));
    latest_first.register(AnyBraceMacro::new("First"));

    let mut context_reversed = MacroContext::default();
    latest_first
        .lower(
            MacroObject::Block(object),
            MacroPosition::EnumVariants,
            &mut context_reversed,
        )
        .expect("brace macro fires");
    let applied_reversed: Vec<&str> = context_reversed
        .macros_applied()
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(applied_reversed, vec!["Second"], "swap order, swap winner",);
}

// ---------------------------------------------------------------------
// Scenario 7 — Position-aware dispatch
//
// Same input shape; two macros differ only by position. The registry
// dispatches based on the position the engine asks about.
// ---------------------------------------------------------------------

#[derive(Clone, Debug)]
struct PositionPinnedMacro {
    label: &'static str,
    position: MacroPosition,
}

impl PositionPinnedMacro {
    fn new(label: &'static str, position: MacroPosition) -> Self {
        Self { label, position }
    }
}

impl SchemaMacroHandler for PositionPinnedMacro {
    fn name(&self) -> &str {
        self.label
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        position == self.position && object.block().is_some_and(|block| block.is_parenthesis())
    }

    fn lower(
        &self,
        _object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        _registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        context.remember_macro(self.label);
        context.remember_position(position);
        match position {
            MacroPosition::RootInput => Ok(MacroOutput::References(Vec::new())),
            MacroPosition::EnumVariants => Ok(MacroOutput::Variants(Vec::new())),
            _ => Ok(MacroOutput::References(Vec::new())),
        }
    }
}

#[test]
fn position_aware_dispatch_picks_macro_by_position_slot() {
    // Two macros — same shape (parenthesis), different positions.
    let mut registry = MacroRegistry::new();
    registry.register(PositionPinnedMacro::new(
        "InputOnly",
        MacroPosition::RootInput,
    ));
    registry.register(PositionPinnedMacro::new(
        "VariantsOnly",
        MacroPosition::EnumVariants,
    ));

    let document = Document::parse("(Foo Bar)").expect("nota parses");
    let object = document.root_object_at(0).expect("root");

    // Dispatch at RootInput → InputOnly fires.
    let mut context_input = MacroContext::default();
    registry
        .lower(
            MacroObject::Block(object),
            MacroPosition::RootInput,
            &mut context_input,
        )
        .expect("input position dispatches");
    assert_eq!(
        context_input
            .macros_applied()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["InputOnly"],
    );

    // Same input — dispatch at EnumVariants → VariantsOnly fires.
    let mut context_variants = MacroContext::default();
    registry
        .lower(
            MacroObject::Block(object),
            MacroPosition::EnumVariants,
            &mut context_variants,
        )
        .expect("variants position dispatches");
    assert_eq!(
        context_variants
            .macros_applied()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["VariantsOnly"],
    );
}

// ---------------------------------------------------------------------
// Scenario 8 — MacroObject::Pair vs MacroObject::Block
//
// Bonus: the engine wraps inputs as `MacroObject::Pair` at the
// NamespaceDeclaration position (the namespace brace is a key/value
// map, and each entry is a name+definition pair). At other positions,
// the input is a single `MacroObject::Block`. A macro must handle the
// shape it expects.
// ---------------------------------------------------------------------

#[derive(Clone, Debug)]
struct PairOnlyMacro {
    label: &'static str,
}

impl PairOnlyMacro {
    fn new(label: &'static str) -> Self {
        Self { label }
    }
}

impl SchemaMacroHandler for PairOnlyMacro {
    fn name(&self) -> &str {
        self.label
    }

    fn matches(&self, object: MacroObject<'_>, position: MacroPosition) -> bool {
        position == MacroPosition::NamespaceDeclaration && object.pair().is_some()
    }

    fn lower(
        &self,
        object: MacroObject<'_>,
        position: MacroPosition,
        context: &mut MacroContext,
        _registry: &MacroRegistry,
    ) -> Result<MacroOutput, SchemaError> {
        context.remember_macro(self.label);
        context.remember_position(position);
        let _pair = object.pair().expect("pair shape checked");
        Ok(MacroOutput::References(vec![TypeReference::new("Witness")]))
    }
}

#[test]
fn macro_object_pair_versus_block_dispatch_shapes() {
    let macro_obj = PairOnlyMacro::new("PairOnly");

    let document = Document::parse("Foo").expect("nota parses");
    let block = document.root_object_at(0).expect("root");

    // MacroObject::Block does NOT match a pair-only macro.
    assert!(!macro_obj.matches(
        MacroObject::Block(block),
        MacroPosition::NamespaceDeclaration
    ));

    // MacroObject::Pair DOES match — the name + definition halves are
    // both Block references. The engine constructs these pairs from the
    // namespace brace's even-positioned children.
    let bodies = Document::parse("Foo [Bar Baz]").expect("nota parses");
    let name = bodies.root_object_at(0).expect("name");
    let definition = bodies.root_object_at(1).expect("definition");
    let pair = MacroObject::Pair(schema_language::MacroPair { name, definition });
    assert!(macro_obj.matches(pair, MacroPosition::NamespaceDeclaration));
}

// ---------------------------------------------------------------------
// Typed macro-library codec — the bootstrap source and the expansion
// templates decode through structural macro nodes only. No positional
// record wrapper, no head-string dispatch.
// ---------------------------------------------------------------------

#[test]
fn macro_library_bootstrap_source_round_trips_through_typed_nodes() {
    let library = MacroLibrary::builtin_source().expect("builtin macro source parses");

    // decode -> encode -> decode fixpoint over the hand-authored notation.
    let source = library.to_source();
    let reparsed = MacroLibrary::from_source(&source).expect("encoded bootstrap source reparses");
    assert_eq!(reparsed, library);

    // The artifact projection round-trips through the same typed value.
    let nota = library.to_nota_source();
    let from_nota = MacroLibrary::from_nota_source(&nota).expect("artifact projection reparses");
    assert_eq!(from_nota, library);
}

#[test]
fn macro_library_source_rejects_malformed_definitions_with_typed_errors() {
    let unknown_head =
        MacroLibrary::from_source("(Bogus Foo NamespaceDeclaration ($X) (Fields $X))")
            .expect_err("unknown entry head is rejected");
    assert!(matches!(
        unknown_head,
        SchemaError::UnsupportedMacroNodeStructure { .. }
    ));

    let short_body = MacroLibrary::from_source("(SchemaMacro Foo NamespaceDeclaration ($X))")
        .expect_err("four-object definition is rejected");
    assert!(matches!(
        short_body,
        SchemaError::MalformedSchemaNode { .. }
    ));

    let unknown_position =
        MacroLibrary::from_source("(SchemaMacro Foo SomewhereElse ($X) (Fields $X))")
            .expect_err("unknown position keyword is rejected");
    assert!(matches!(
        unknown_position,
        SchemaError::MalformedSchemaNode { .. }
    ));
}

#[test]
fn expansion_template_enum_decodes_each_template_kind() {
    use nota::StructuralMacroNode;
    use schema_language::{MacroTemplate, TypeTemplate};

    let struct_template = MacroTemplate::from_structural_nota("(Type (Struct $Name [$*Fields]))")
        .expect("Type Struct template decodes");
    assert!(matches!(
        struct_template,
        MacroTemplate::Type(TypeTemplate::Struct(_, _))
    ));
    assert_eq!(
        struct_template.to_structural_nota(),
        "(Type (Struct $Name [$*Fields]))"
    );

    let enum_template = MacroTemplate::from_structural_nota("(Type (Enum $Name ($*Variants)))")
        .expect("Type Enum template decodes");
    assert!(matches!(
        enum_template,
        MacroTemplate::Type(TypeTemplate::Enum(_, _))
    ));
    assert_eq!(
        enum_template.to_structural_nota(),
        "(Type (Enum $Name ($*Variants)))"
    );

    let newtype_template = MacroTemplate::from_structural_nota("(Type (Newtype $Name $Reference))")
        .expect("Type Newtype template decodes");
    assert!(matches!(
        newtype_template,
        MacroTemplate::Type(TypeTemplate::Newtype(_, _))
    ));

    let fields_template =
        MacroTemplate::from_structural_nota("(Fields $*Fields)").expect("Fields template decodes");
    let MacroTemplate::Fields(field_objects) = &fields_template else {
        panic!("expected Fields template");
    };
    assert_eq!(field_objects.len(), 1);
    assert_eq!(fields_template.to_structural_nota(), "(Fields $*Fields)");

    let variants_template = MacroTemplate::from_structural_nota("(Variants Decision Correction)")
        .expect("Variants template decodes");
    let MacroTemplate::Variants(variant_objects) = &variants_template else {
        panic!("expected Variants template");
    };
    assert_eq!(variant_objects.len(), 2);
    assert_eq!(
        variants_template.to_structural_nota(),
        "(Variants Decision Correction)"
    );

    let reference_template = MacroTemplate::from_structural_nota("(Reference Vector.$Type)")
        .expect("Reference template decodes");
    assert!(matches!(reference_template, MacroTemplate::Reference(_)));
    assert_eq!(
        reference_template.to_structural_nota(),
        "(Reference Vector.$Type)"
    );
}

#[test]
fn expansion_template_enum_rejects_unknown_heads_with_typed_errors() {
    use nota::StructuralMacroNode;
    use schema_language::MacroTemplate;

    let unknown_head = MacroTemplate::from_structural_nota("(Bogus $X)")
        .expect_err("unknown template head does not decode");
    assert!(matches!(
        SchemaError::from(unknown_head),
        SchemaError::UnsupportedMacroNodeStructure { .. }
    ));

    let unknown_type_kind = MacroTemplate::from_structural_nota("(Type (Bogus $Name $Body))")
        .expect_err("unknown type-template kind does not decode");
    assert!(matches!(
        SchemaError::from(unknown_type_kind),
        SchemaError::MalformedSchemaNode { .. }
    ));
}
